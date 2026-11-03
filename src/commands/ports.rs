//! `proc ports` - List all listening TCP ports
//!
//! Examples:
//!   proc ports              # Show all listening TCP ports
//!   proc ports --by node     # Only ports of the processes named node
//!   proc ports --by node --cmd  # ... or with node anywhere in their command line
//!   proc ports --exposed    # Only network-accessible ports (0.0.0.0, ::)
//!   proc ports --local      # Only localhost ports (127.0.0.1, ::1)
//!   proc ports -v           # Show with executable paths

use crate::core::{
    matches_name, resolve_in_dir, sort_ports, NameMatch, Names, PortInfo, PortSortKey, Process,
};
use crate::error::Result;
use crate::ui::{out, truncate_string, Printer};
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

    /// Show verbose output (includes executable path)
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

impl PortsCommand {
    /// Executes the ports command, listing all listening TCP ports.
    pub fn execute(&self) -> Result<()> {
        let mut ports = PortInfo::get_all_listening()?;

        // Filter by process name if specified
        if let Some(ref name) = self.by_name {
            let mode = NameMatch::from_flag(self.cmd);
            let running: HashMap<u32, Process> = Process::find_all()?
                .into_iter()
                .map(|p| (p.pid, p))
                .collect();
            ports.retain(|p| port_named(p, running.get(&p.pid), name, mode));
        }

        // Filter by directory if specified
        let in_dir_filter = resolve_in_dir(&self.in_dir)?;
        if let Some(ref dir_path) = in_dir_filter {
            ports.retain(|p| {
                cwd_within(
                    Process::find_by_pid(p.pid).ok().flatten().as_ref(),
                    dir_path,
                )
            });
        }
        // A deleted directory is kept while a process still runs in it, also
        // one that listens on nothing: it is a directory, with no ports
        crate::core::require_in_dir_live(&self.in_dir, &in_dir_filter)?;

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

        // Sort ports: ties come by port, then PID
        sort_ports(&mut ports, self.sort);

        // Apply limit if specified
        if let Some(limit) = self.limit {
            ports.truncate(limit);
        }

        // In verbose mode, fetch process info for paths
        let process_map: HashMap<u32, Process> = if self.verbose {
            // One lookup per PID; a process that ended since its port was listed has no entry
            let pids: HashSet<u32> = ports.iter().map(|p| p.pid).collect();
            pids.into_iter()
                .filter_map(|pid| {
                    let proc = Process::find_by_pid(pid).ok().flatten()?;
                    Some((pid, proc))
                })
                .collect()
        } else {
            HashMap::new()
        };

        if self.json {
            self.print_json(&ports, &process_map);
        } else {
            // A failed write is recorded by `out`, which decides the exit code
            out::recorded(self.print_human(&mut out::stdout(), &ports, &process_map));
        }

        Ok(())
    }

    fn print_human(
        &self,
        out: &mut impl Write,
        ports: &[PortInfo],
        process_map: &HashMap<u32, Process>,
    ) -> std::io::Result<()> {
        if ports.is_empty() {
            return writeln!(out, "{} No listening ports found", "⚠".yellow().bold());
        }

        writeln!(
            out,
            "{} Found {} listening port{}",
            "✓".green().bold(),
            ports.len().to_string().cyan().bold(),
            if ports.len() == 1 { "" } else { "s" }
        )?;
        writeln!(out)?;

        // Header
        writeln!(
            out,
            "{:<8} {:<10} {:<8} {:<20} {:<15}",
            "PORT".bright_blue().bold(),
            "PROTO".bright_blue().bold(),
            "PID".bright_blue().bold(),
            "PROCESS".bright_blue().bold(),
            "ADDRESS".bright_blue().bold()
        )?;
        writeln!(out, "{}", "─".repeat(65).bright_black())?;

        for port in ports {
            let addr = port.address.as_deref().unwrap_or("*");
            let proto = format!("{:?}", port.protocol).to_uppercase();

            writeln!(
                out,
                "{:<8} {:<10} {:<8} {:<20} {:<15}",
                port.port.to_string().cyan().bold(),
                proto.white(),
                port.pid.to_string().cyan(),
                truncate_string(&port.process_name, 19).white(),
                addr.bright_black()
            )?;

            // In verbose mode, show path
            if self.verbose {
                if let Some(proc) = process_map.get(&port.pid) {
                    if let Some(ref path) = proc.exe_path {
                        writeln!(
                            out,
                            "         {} {}",
                            "↳".bright_black(),
                            truncate_string(path, 55).bright_black()
                        )?;
                    }
                }
            }
        }
        writeln!(out)
    }

    fn print_json(&self, ports: &[PortInfo], process_map: &HashMap<u32, Process>) {
        let printer = Printer::from_flags(true, self.verbose);

        let enriched: Vec<PortWithProcess> = ports
            .iter()
            .map(|p| PortWithProcess {
                port: p,
                exe_path: process_map
                    .get(&p.pid)
                    .and_then(|proc| proc.exe_path.as_deref()),
            })
            .collect();

        printer.print_json(&PortsOutput {
            action: "ports",
            success: true,
            count: ports.len(),
            ports: enriched,
        });
    }
}

#[derive(Serialize)]
struct PortWithProcess<'a> {
    #[serde(flatten)]
    port: &'a PortInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    exe_path: Option<&'a str>,
}

#[derive(Serialize)]
struct PortsOutput<'a> {
    action: &'static str,
    success: bool,
    count: usize,
    ports: Vec<PortWithProcess<'a>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{ProcessStatus, Protocol};
    use crate::test_support::assert_stops_at_a_failed_write;

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

    #[test]
    fn verbose_listing_shows_a_path_only_when_the_process_has_one() {
        colored::control::set_override(false);
        let cmd = PortsCommand {
            by_name: None,
            cmd: false,
            in_dir: None,
            exposed: false,
            local: false,
            json: false,
            verbose: true,
            sort: PortSortKey::Port,
            limit: None,
            range: None,
        };
        let port = PortInfo {
            port: 4000,
            protocol: Protocol::Tcp,
            pid: 1,
            process_name: "x".to_string(),
            address: None,
        };
        let listing = |map: &HashMap<u32, Process>| {
            let mut out = Vec::new();
            cmd.print_human(&mut out, std::slice::from_ref(&port), map)
                .unwrap();
            String::from_utf8(out).unwrap()
        };

        // The row is there, with `*` for the missing address, in every case
        let without_entry = listing(&HashMap::new());
        let row = without_entry
            .lines()
            .find(|l| l.starts_with("4000"))
            .unwrap();
        assert!(
            row.contains("TCP") && row.trim_end().ends_with('*'),
            "{row}"
        );
        assert!(!without_entry.contains('↳'), "{without_entry}");

        let without_path = listing(&HashMap::from([(1, process(None, None))]));
        assert!(!without_path.contains('↳'), "{without_path}");

        let with_path = listing(&HashMap::from([(1, process(None, Some("/bin/x")))]));
        assert!(with_path.contains("↳ /bin/x\n"), "{with_path}");
    }

    #[test]
    fn an_empty_listing_says_so() {
        colored::control::set_override(false);
        let cmd = PortsCommand {
            by_name: None,
            cmd: false,
            in_dir: None,
            exposed: false,
            local: false,
            json: false,
            verbose: false,
            sort: PortSortKey::Port,
            limit: None,
            range: None,
        };
        let mut out = Vec::new();
        cmd.print_human(&mut out, &[], &HashMap::new()).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "⚠ No listening ports found\n"
        );
    }

    #[test]
    fn a_failed_write_ends_the_listing() {
        let cmd = PortsCommand {
            by_name: None,
            cmd: false,
            in_dir: None,
            exposed: false,
            local: false,
            json: false,
            verbose: true,
            sort: PortSortKey::Port,
            limit: None,
            range: None,
        };
        let port = PortInfo {
            port: 4000,
            protocol: Protocol::Tcp,
            pid: 1,
            process_name: "x".to_string(),
            address: Some("127.0.0.1".to_string()),
        };
        let with_path = HashMap::from([(1, process(None, Some("/bin/x")))]);
        assert_stops_at_a_failed_write(|w| {
            cmd.print_human(w, std::slice::from_ref(&port), &with_path)
        });
        assert_stops_at_a_failed_write(|w| cmd.print_human(w, &[], &HashMap::new()));
    }
}
