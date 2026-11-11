//! `proc ports` - List all listening TCP ports
//!
//! Examples:
//!   proc ports              # Show all listening TCP ports
//!   proc ports --by node     # Only ports of the processes named node
//!   proc ports --by node --cmd  # ... or with node anywhere in their command line
//!   proc ports --exposed    # Only network-accessible ports (0.0.0.0, ::)
//!   proc ports --local      # Only localhost ports (127.0.0.1, ::1)
//!   proc ports -v           # Add user, memory, executable path and the connection counts (JSON)

use crate::commands::held::{ConnectionReader, Counts};
use crate::core::port::SocketReading;
use crate::core::{
    matches_name, resolve_in_dir, sort_ports, Connections, NameMatch, Names, PortInfo, PortSortKey,
    Process,
};
use crate::error::Result;
use crate::ui::card::known;
use crate::ui::output::screen_now;
use crate::ui::table::{port_table, PortRow, Screen, TableLayout};
use crate::ui::{out, Printer};
use clap::Args;
use colored::*;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;

/// Whether the process is known and its working directory is inside `dir`
fn cwd_within(proc: Option<&Process>, dir: &Path) -> bool {
    proc.is_some_and(|p| crate::core::filters::matches_in_filter(p, dir))
}

/// Whether the process behind a port matches `--by`, the way it does in every
/// other command. A process that ended since its port was listed is known only
/// by the name the port was listed with.
fn port_named(port: &PortInfo, process: Option<&Process>, pattern: &str, mode: NameMatch) -> bool {
    match process {
        Some(process) => matches_name(pattern, &process.names(), mode),
        None => matches_name(
            pattern,
            &Names {
                name: &port.process_name,
                ..Names::default()
            },
            mode,
        ),
    }
}

/// An inclusive port range, START-END, such as 8000-9000
fn parse_port_range(s: &str) -> std::result::Result<(u16, u16), String> {
    let usage = || format!("'{s}' is not a range of ports; use START-END, e.g. 8000-9000");
    let (start, end) = s.split_once('-').ok_or_else(usage)?;
    let port = |p: &str| p.trim().parse::<u16>().ok().filter(|&p| p > 0);
    let (start, end) = match (port(start), port(end)) {
        (Some(start), Some(end)) => (start, end),
        _ => return Err(format!("{}; ports go from 1 to 65535", usage())),
    };
    if start > end {
        return Err(format!(
            "'{s}' starts after it ends; did you mean {end}-{start}?"
        ));
    }
    Ok((start, end))
}

/// List all listening TCP ports
#[derive(Args, Debug)]
pub struct PortsCommand {
    /// Filter by process name
    #[arg(long = "by", short = 'b')]
    pub by_name: Option<String>,

    /// Match --by as a substring of the process name or full command line (like pkill -f)
    #[arg(long)]
    pub cmd: bool,

    /// Filter by directory; takes the next word as the directory unless it starts with '-'
    /// (bare --in is the current directory, --in=DIR is explicit)
    #[arg(long = "in", short = 'i', num_args = 0..=1, default_missing_value = ".")]
    pub in_dir: Option<String>,

    /// Only show network-exposed ports (0.0.0.0, ::)
    #[arg(long, short = 'e')]
    pub exposed: bool,

    /// Only show localhost ports (127.0.0.1, ::1)
    #[arg(long, short = 'l')]
    pub local: bool,

    /// Output as JSON
    #[arg(long, short = 'j')]
    pub json: bool,

    /// Show verbose output (adds USER, MEM and the executable path; JSON adds the connection counts; nothing is cut)
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// Sort by: port, pid, name (all ascending; ties by port, then PID)
    #[arg(long, short = 's', value_enum, default_value_t = PortSortKey::Port)]
    pub sort: PortSortKey,

    /// Limit the number of results
    #[arg(long, short = 'n')]
    pub limit: Option<usize>,

    /// Filter ports by range (e.g., 8000-9000)
    #[arg(long, short = 'r', value_parser = parse_port_range)]
    pub range: Option<(u16, u16)>,
}

/// The holders of `ports` as processes, read in one go: a holder that ended
/// since its port was listed has no entry
fn holders_of(ports: &[PortInfo]) -> HashMap<u32, Process> {
    let pids: Vec<u32> = ports
        .iter()
        .map(|p| p.pid)
        .collect::<HashSet<u32>>()
        .into_iter()
        .collect();
    Process::find_by_pids(&pids)
        .into_iter()
        .map(|p| (p.pid, p))
        .collect()
}

/// The connections that came in on each listener's own sockets, by port and
/// holder, from one reading of the socket table. `listening` is every listening
/// socket, so a port several processes hold is split between them. A listener
/// whose connections the OS does not give (the BSDs, or a reading that failed)
/// has no entry.
fn connections_of_each(
    listening: SocketReading,
    ports: &[PortInfo],
) -> HashMap<(u16, u32), Connections> {
    let mut reader = ConnectionReader::from_reading(listening, 0, true);
    let numbers: Vec<u16> = ports.iter().map(|p| p.port).collect();
    reader.read_now(&numbers);
    ports
        .iter()
        .filter_map(|p| Some(((p.port, p.pid), reader.of_holder(p.port, p.pid)?)))
        .collect()
}

impl PortsCommand {
    /// Executes the ports command, listing all listening TCP ports.
    pub fn execute(&self) -> Result<()> {
        // One reading of the socket tables serves the listing and, with `-v`,
        // the connections of each listener
        let reading = SocketReading::read()?;
        let mut ports = reading.ports.clone();
        // The ports other users' processes hold, which proc lists no holder
        // for. Said only when no filter by process or address is on: proc
        // can't tell whether a holder it can't see would pass one. `--range`
        // filters by number, so it applies
        let unseen =
            (self.by_name.is_none() && self.in_dir.is_none() && !self.exposed && !self.local)
                .then(|| PortInfo::listening_unseen(&reading.ports))
                .flatten()
                .map(|mut unseen| {
                    if let Some((start, end)) = self.range {
                        unseen.retain(|port| (start..=end).contains(port));
                    }
                    unseen
                });
        // `-v` counts the connections of each listener, which takes every
        // process that holds the port, whatever the filters keep
        let listening = self.verbose.then_some(reading);

        let in_dir_filter = resolve_in_dir(&self.in_dir)?;

        // Filter by address exposure
        if self.exposed {
            ports.retain(PortInfo::is_exposed);
        }

        if self.local {
            ports.retain(PortInfo::is_local);
        }

        // Filter by port range (e.g., "3000-9000"), validated when parsed
        if let Some((start, end)) = self.range {
            ports.retain(|p| p.port >= start && p.port <= end);
        }

        // The holders of the ports that are left, read once for the filters
        // by process and for the columns
        let mut holders = None;
        if self.by_name.is_some() || in_dir_filter.is_some() {
            let running = holders_of(&ports);
            // Filter by process name if specified
            if let Some(ref name) = self.by_name {
                let mode = NameMatch::from_flag(self.cmd);
                ports.retain(|p| port_named(p, running.get(&p.pid), name, mode));
            }
            // Filter by directory if specified
            if let Some(ref dir_path) = in_dir_filter {
                ports.retain(|p| cwd_within(running.get(&p.pid), dir_path));
            }
            holders = Some(running);
        }
        // A deleted directory is kept while a process still runs in it, also
        // one that listens on nothing: it is a directory, with no ports
        crate::core::require_in_dir_live(&self.in_dir, &in_dir_filter)?;

        // Sort ports: ties come by port, then PID
        sort_ports(&mut ports, self.sort);

        // Apply limit if specified
        if let Some(limit) = self.limit {
            ports.truncate(limit);
        }

        let holders = holders.unwrap_or_else(|| holders_of(&ports));
        let connections = match listening {
            Some(listening) => connections_of_each(listening, &ports),
            None => HashMap::new(),
        };

        let left = LeftOut {
            unseen: unseen.as_deref(),
            hidden_by: crate::core::port::others_hidden(),
        };
        if self.json {
            self.print_json(&ports, &holders, &connections, left);
        } else {
            let rows: Vec<PortRow> = ports
                .iter()
                .map(|port| PortRow {
                    port,
                    process: holders.get(&port.pid),
                })
                .collect();
            // A failed write is recorded by `out`, which decides the exit code
            out::recorded(self.print_human(&mut out::stdout(), &rows, screen_now(), left));
        }

        Ok(())
    }

    fn print_human(
        &self,
        out: &mut impl Write,
        rows: &[PortRow],
        screen: Screen,
        left: LeftOut,
    ) -> std::io::Result<()> {
        if rows.is_empty() {
            writeln!(out, "{} No listening ports found", "⚠".yellow().bold())?;
        } else {
            writeln!(
                out,
                "{} Found {} listening port{}",
                "✓".green().bold(),
                rows.len().to_string().cyan().bold(),
                if rows.len() == 1 { "" } else { "s" }
            )?;
            writeln!(out)?;

            // `-v` cuts nothing and adds USER, MEM and PATH
            let layout = TableLayout::from_flags(screen, self.verbose, false, false);
            writeln!(out, "{}", port_table(rows, &layout))?;
            writeln!(out)?;
        }
        // The list leaves out what proc can't see, and says so
        if let Some(setting) = left.hidden_by {
            let message = format!(
                "This system hides other users' sockets ({setting}), so the list has only your own processes' ports (run with sudo to see them all)"
            );
            writeln!(out, "{} {}", "⚠".yellow().bold(), message.yellow())?;
        }
        if let Some(unseen) = left.unseen.filter(|unseen| !unseen.is_empty()) {
            let ports: Vec<String> = unseen.iter().map(u16::to_string).collect();
            let message = format!(
                "{} port{} in use by other users' processes, which proc can't see: {} (run with sudo to see them)",
                unseen.len(),
                if unseen.len() == 1 { "" } else { "s" },
                ports.join(", ")
            );
            writeln!(out, "{} {}", "⚠".yellow().bold(), message.yellow())?;
        }
        Ok(())
    }

    fn print_json(
        &self,
        ports: &[PortInfo],
        holders: &HashMap<u32, Process>,
        connections: &HashMap<(u16, u32), Connections>,
        left: LeftOut,
    ) {
        let printer = Printer::from_flags(true, self.verbose);

        let enriched: Vec<PortWithProcess> = ports
            .iter()
            .map(|p| {
                enrich(
                    p,
                    holders.get(&p.pid),
                    self.verbose,
                    connections.get(&(p.port, p.pid)),
                )
            })
            .collect();

        printer.print_json(&PortsOutput {
            action: "ports",
            success: true,
            count: ports.len(),
            ports: enriched,
            unseen_ports: left.unseen,
            hidden_by: left.hidden_by,
        });
    }
}

/// A port with what its holder is. The path is for `-v`, as it has always
/// been; a value the OS did not give is left out.
fn enrich<'a>(
    port: &'a PortInfo,
    process: Option<&'a Process>,
    verbose: bool,
    connections: Option<&'a Connections>,
) -> PortWithProcess<'a> {
    PortWithProcess {
        port,
        exe_path: process
            .filter(|_| verbose)
            .and_then(|proc| proc.exe_path.as_deref()),
        command: process.and_then(|proc| known(&proc.command)),
        user: process.and_then(|proc| known(&proc.user)),
        uptime_secs: process.and_then(|proc| proc.uptime_secs),
        // 0 is what an unread value is in `Process`: unknown, so no key
        memory_mb: process
            .filter(|proc| proc.read_by_os())
            .map(|proc| proc.memory_mb),
        connections: connections.map(Counts::of),
    }
}

/// A port as JSON: the listener (its keys and their order are those `ports` has
/// always had, with `exposure` last), then what its holder is, from the
/// holder's process; a value the OS does not give is a missing key. The
/// connection counts come only with `-v`.
#[derive(Serialize)]
struct PortWithProcess<'a> {
    #[serde(flatten)]
    port: &'a PortInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    exe_path: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    uptime_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    memory_mb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    connections: Option<Counts<'a>>,
}

#[derive(Serialize)]
struct PortsOutput<'a> {
    action: &'static str,
    success: bool,
    count: usize,
    ports: Vec<PortWithProcess<'a>>,
    /// The ports other users' processes hold, which `ports` can't list:
    /// `[]` for none, absent where proc doesn't check (Windows, the BSDs) or
    /// with a filter by process or address
    #[serde(skip_serializing_if = "Option::is_none")]
    unseen_ports: Option<&'a [u16]>,
    /// The setting through which this system hides other users' sockets
    /// (`security.bsd.see_other_uids=0`, `security.curtain=1`), so `ports` has
    /// only proc's own user's ports; absent where nothing hides them or proc is
    /// root
    #[serde(skip_serializing_if = "Option::is_none")]
    hidden_by: Option<&'static str>,
}

/// What a listing leaves out because proc can't see it
#[derive(Debug, Clone, Copy, Default)]
struct LeftOut<'a> {
    /// The ports other users' processes hold (Linux and macOS); `None` where
    /// proc doesn't check, or with a filter by process or address
    unseen: Option<&'a [u16]>,
    /// The setting through which the system hides other users' sockets, when
    /// it does (the BSDs)
    hidden_by: Option<&'static str>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{ProcessStatus, Protocol};
    use crate::test_support::assert_stops_at_a_failed_write;
    use crate::ui::table::{PortRow, Screen};

    fn process(cwd: Option<&str>, exe_path: Option<&str>) -> Process {
        Process {
            pid: 1,
            name: "x".to_string(),
            exe_path: exe_path.map(String::from),
            cwd: cwd.map(String::from),
            command: None,
            argv0: None,
            raw: None,
            cpu_percent: 0.0,
            memory_mb: 0.0,
            status: ProcessStatus::Sleeping,
            user: None,
            parent_pid: None,
            start_time: None,
            uptime_secs: None,
            parent_name: None,
            cpu_time_secs: None,
            open_files: None,
            child_count: None,
        }
    }

    fn listed(name: &str) -> PortInfo {
        PortInfo {
            port: 4000,
            protocol: Protocol::Tcp,
            pid: 1,
            process_name: name.to_string(),
            address: None,
        }
    }

    #[test]
    fn a_port_is_named_by_its_process_else_by_the_name_it_was_listed_with() {
        let mut node = process(None, Some("/opt/tools/serve"));
        node.name = "node".to_string();
        node.command = Some("node app.js --engine deno".to_string());
        let port = listed("stale-name");

        // the process wins: its name, its executable, not the listing's name
        assert!(port_named(&port, Some(&node), "NODE", NameMatch::Exact));
        assert!(port_named(&port, Some(&node), "serve", NameMatch::Exact));
        assert!(!port_named(
            &port,
            Some(&node),
            "stale-name",
            NameMatch::Exact
        ));
        // part of the name, or the command line, only with --cmd
        assert!(!port_named(&port, Some(&node), "nod", NameMatch::Exact));
        assert!(!port_named(&port, Some(&node), "deno", NameMatch::Exact));
        assert!(port_named(&port, Some(&node), "nod", NameMatch::Command));
        assert!(port_named(&port, Some(&node), "deno", NameMatch::Command));

        // the process ended since the port was listed: only the listed name is left
        assert!(port_named(&port, None, "Stale-Name", NameMatch::Exact));
        assert!(!port_named(&port, None, "stale", NameMatch::Exact));
        assert!(port_named(&port, None, "stale", NameMatch::Command));
        assert!(!port_named(&port, None, "node", NameMatch::Command));
    }

    #[test]
    fn only_a_known_process_inside_the_directory_passes_in() {
        let dir = Path::new("/work");
        assert!(cwd_within(Some(&process(Some("/work/app"), None)), dir));
        assert!(!cwd_within(Some(&process(Some("/elsewhere"), None)), dir));
        // No working directory (a zombie has none), or no process at all
        // (it ended since its port was listed)
        assert!(!cwd_within(Some(&process(None, None)), dir));
        assert!(!cwd_within(None, dir));
    }

    fn listing(verbose: bool) -> PortsCommand {
        PortsCommand {
            by_name: None,
            cmd: false,
            in_dir: None,
            exposed: false,
            local: false,
            json: false,
            verbose,
            sort: PortSortKey::Port,
            limit: None,
            range: None,
        }
    }

    fn screen() -> Screen {
        Screen {
            width: 120,
            now_secs: 1_000,
        }
    }

    fn printed(cmd: &PortsCommand, rows: &[PortRow]) -> String {
        printed_with(cmd, rows, None)
    }

    fn printed_with(cmd: &PortsCommand, rows: &[PortRow], unseen: Option<&[u16]>) -> String {
        printed_leaving(
            cmd,
            rows,
            LeftOut {
                unseen,
                hidden_by: None,
            },
        )
    }

    fn printed_leaving(cmd: &PortsCommand, rows: &[PortRow], left: LeftOut) -> String {
        let mut out = Vec::new();
        cmd.print_human(&mut out, rows, screen(), left).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn the_listing_is_the_shared_table_under_a_headline() {
        colored::control::set_override(false);
        let port = PortInfo {
            port: 4000,
            protocol: Protocol::Tcp,
            pid: 1,
            process_name: "x".to_string(),
            address: Some("127.0.0.1".to_string()),
        };
        let mut holder = process(None, Some("/bin/x"));
        holder.command = Some("/bin/x --serve".to_string());
        holder.start_time = Some(970);
        let row = |process| PortRow {
            port: &port,
            process,
        };

        let out = printed(&listing(false), &[row(Some(&holder))]);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "✓ Found 1 listening port");
        assert_eq!(lines[1], "");
        assert!(lines[2].contains("EXPOSURE"), "{out}");
        assert!(
            lines[3].contains("30s") && lines[3].contains("/bin/x --serve"),
            "{out}"
        );
        assert!(!lines[2].contains("PATH"), "{out}");
        assert_eq!(lines.len(), 5, "{out}");
        assert_eq!(lines[4], "");

        // a holder that ended: the port's cells stay, the process's are dashes
        let gone = printed(&listing(true), &[row(None)]);
        let line = gone.lines().nth(3).unwrap();
        assert!(
            line.contains("4000") && line.contains("127.0.0.1"),
            "{gone}"
        );
        assert!(line.trim_end().ends_with('-'), "{gone}");
        assert!(gone.lines().nth(2).unwrap().contains("PATH"), "{gone}");
    }

    #[test]
    fn memory_the_os_refused_is_a_missing_key() {
        let port = listed("x");
        let mut holder = process(None, Some("/bin/x"));
        holder.memory_mb = 0.0;
        let json = serde_json::to_value(enrich(&port, Some(&holder), true, None)).unwrap();
        // 0 is a refusal only where the OS refuses (macOS, Windows)
        let refused = cfg!(any(target_os = "macos", windows));
        assert_eq!(json.get("memory_mb").is_none(), refused, "{json}");
        assert_eq!(json["exe_path"], "/bin/x");

        holder.memory_mb = 41.5;
        let json = serde_json::to_value(enrich(&port, Some(&holder), false, None)).unwrap();
        assert_eq!(json["memory_mb"], 41.5);
        assert!(json.get("exe_path").is_none(), "{json}");

        // a holder that ended: only the port
        let json = serde_json::to_value(enrich(&port, None, true, None)).unwrap();
        assert!(json.get("memory_mb").is_none() && json.get("command").is_none());
    }

    #[test]
    fn an_empty_listing_says_so() {
        colored::control::set_override(false);
        assert_eq!(
            printed(&listing(false), &[]),
            "⚠ No listening ports found\n"
        );
    }

    #[test]
    fn a_failed_write_ends_the_listing() {
        let port = PortInfo {
            port: 4000,
            protocol: Protocol::Tcp,
            pid: 1,
            process_name: "x".to_string(),
            address: Some("127.0.0.1".to_string()),
        };
        let holder = process(None, Some("/bin/x"));
        let cmd = listing(true);
        let rows = [PortRow {
            port: &port,
            process: Some(&holder),
        }];
        let shown = LeftOut::default();
        let unseen = LeftOut {
            unseen: Some(&[22]),
            hidden_by: None,
        };
        let hidden = LeftOut {
            unseen: None,
            hidden_by: Some("security.curtain=1"),
        };
        assert_stops_at_a_failed_write(|w| cmd.print_human(w, &rows, screen(), shown));
        assert_stops_at_a_failed_write(|w| cmd.print_human(w, &[], screen(), shown));
        assert_stops_at_a_failed_write(|w| cmd.print_human(w, &rows, screen(), unseen));
        assert_stops_at_a_failed_write(|w| cmd.print_human(w, &rows, screen(), hidden));
    }

    // A list that leaves ports out says so, after the table
    #[test]
    fn the_ports_other_users_hold_are_said_after_the_list() {
        let port = PortInfo {
            port: 3000,
            protocol: Protocol::Tcp,
            pid: 42,
            process_name: "node".to_string(),
            address: Some("127.0.0.1".to_string()),
        };
        let holder = process(None, Some("/bin/node"));
        let cmd = listing(false);
        let rows = [PortRow {
            port: &port,
            process: Some(&holder),
        }];
        let two = printed_with(&cmd, &rows, Some(&[22, 80]));
        assert!(two.contains("Found 1 listening port"), "{two}");
        assert!(
            two.trim_end().ends_with(
                "⚠ 2 ports in use by other users' processes, which proc can't see: 22, 80 (run with sudo to see them)"
            ),
            "{two}"
        );
        // one, with nothing proc can see
        let one = printed_with(&cmd, &[], Some(&[22]));
        assert!(one.starts_with("⚠ No listening ports found"), "{one}");
        assert!(
            one.contains("⚠ 1 port in use by other users' processes, which proc can't see: 22"),
            "{one}"
        );
        // none hidden, or not checked: no line
        for unseen in [Some(&[][..]), None] {
            let none = printed_with(&cmd, &rows, unseen);
            assert!(!none.contains("other users"), "{none}");
        }
    }

    // On a BSD that hides other users' sockets the list can't name what it
    // leaves out, and says why it may be short
    #[test]
    fn a_system_that_hides_other_users_sockets_is_said_after_the_list() {
        let cmd = listing(false);
        let hidden = LeftOut {
            unseen: None,
            hidden_by: Some("security.bsd.see_other_uids=0"),
        };
        let empty = printed_leaving(&cmd, &[], hidden);
        assert!(empty.starts_with("⚠ No listening ports found"), "{empty}");
        assert!(
            empty.trim_end().ends_with(
                "⚠ This system hides other users' sockets (security.bsd.see_other_uids=0), so the list has only your own processes' ports (run with sudo to see them all)"
            ),
            "{empty}"
        );
        // and in JSON, as the setting
        let json = serde_json::to_value(PortsOutput {
            action: "ports",
            success: true,
            count: 0,
            ports: Vec::new(),
            unseen_ports: None,
            hidden_by: hidden.hidden_by,
        })
        .unwrap();
        assert_eq!(json["hidden_by"], "security.bsd.see_other_uids=0");
        let shown = serde_json::to_value(PortsOutput {
            action: "ports",
            success: true,
            count: 0,
            ports: Vec::new(),
            unseen_ports: Some(&[]),
            hidden_by: None,
        })
        .unwrap();
        assert!(shown.get("hidden_by").is_none(), "{shown}");
    }

    #[test]
    fn holders_are_read_once_and_a_gone_one_is_missing() {
        let me = std::process::id();
        let mut mine = listed("x");
        mine.pid = me;
        let mut gone = listed("y");
        gone.pid = 4_000_000;
        let found = holders_of(&[mine.clone(), mine, gone]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[&me].command.is_some());
        assert!(holders_of(&[]).is_empty());
    }

    /// The connections of each listener come from one reading of the socket
    /// table; a listener nobody holds in it has none
    #[test]
    #[cfg_attr(
        any(target_os = "freebsd", target_os = "netbsd"),
        ignore = "the BSDs read no connections"
    )]
    fn each_listener_gets_the_connections_that_came_in_on_its_port() {
        let server = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = server.local_addr().unwrap().port();
        let _clients: Vec<_> = (0..2)
            .map(|_| std::net::TcpStream::connect(("127.0.0.1", port)).unwrap())
            .collect();
        let mine = PortInfo {
            port,
            protocol: Protocol::Tcp,
            pid: std::process::id(),
            process_name: "x".to_string(),
            address: Some("127.0.0.1".to_string()),
        };
        let counted = connections_of_each(
            SocketReading::of(vec![mine.clone()]),
            std::slice::from_ref(&mine),
        );
        let found = &counted[&(port, mine.pid)];
        assert_eq!(found.total, 2, "{found:?}");
        assert_eq!(found.by_state["established"], 2);
        assert!(found.peers.is_empty(), "counts only");
    }
}
