//! `proc on` - Port/process lookup
//!
//! Every answer is the process card (see `ui::card`): by port, one card per
//! process holding it; by PID or name, one card per process.
//!
//! Usage:
//!   proc on :3000              # What process is on port 3000?
//!   proc on :3000 :8080        # What's on several ports? (separate arguments)
//!   proc on 1234               # What is PID 1234 and what ports is it using?
//!   proc on node               # What ports are the processes named node listening on?
//!   proc on node --cmd         # ... or with node anywhere in their command line
//!   proc on node --in .        # Node processes in cwd and their ports
//!
//! Commas still work (`proc on :3000,:8080`) but are deprecated.

use super::held::{
    card_options, measure_with_ports, shown_ports, write_process, write_process_card,
    ConnectionReader, HeldPort,
};
use crate::core::connections::Connections;
use crate::core::port::{holders_in, visible_holders, SocketReading, UnseenPort};
use crate::core::target::{includes_note, left_out_warning};
use crate::core::{
    flatten_targets, nothing_resolved, parse_target, resolve_in_dir, ruled_out_by, Ancestor,
    Ancestors, Exposure, Guards, NameMatch, PortInfo, Process, Resolved, TargetType,
};
use crate::error::{ExitCode, ProcError, Result};
use crate::out_println;
use crate::ui::card::CardOptions;
use crate::ui::{out, Printer};
use clap::Args;
use colored::*;
use serde::Serialize;
use std::io::Write;

/// Peers the JSON of a port lists at most (`peers_truncated` says it cut)
const JSON_PEER_LIMIT: usize = 50;

/// Show what's on a port, or what a process is and what ports it is on, as one card
#[derive(Args, Debug, Clone)]
pub struct OnCommand {
    /// Target(s): :port, PID, or process name (separate arguments for several)
    #[arg(required = true, num_args = 1.., value_parser = crate::core::validate_target_arg)]
    pub target: Vec<String>,

    /// Filter by directory; takes the next word as the directory unless it starts with '-'
    /// (bare --in is the current directory, --in=DIR is explicit)
    #[arg(long = "in", short = 'i', num_args = 0..=1, default_missing_value = ".")]
    pub in_dir: Option<String>,

    /// Filter by process name
    #[arg(long = "by", short = 'b')]
    pub by_name: Option<String>,

    /// Match name targets and --by as a substring of the process name or full command line (like pkill -f); the headline then says `matching`
    #[arg(long)]
    pub cmd: bool,

    /// Output as JSON
    #[arg(long, short = 'j')]
    pub json: bool,

    /// Print every value whole and every connection (default: cut to the terminal width, 5 peers)
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// `--in` as `execute` resolved it, absolute; every check uses this one
    #[arg(skip)]
    in_dir_path: Option<std::path::PathBuf>,
}

impl OnCommand {
    /// Executes the on command, performing bidirectional port/process lookup.
    pub fn execute(&self) -> Result<()> {
        // Resolve --in once: a relative directory needs the current one, which
        // may be gone (an error here), and every check after this one uses the
        // absolute path. A missing directory with nothing running there is a
        // typo, as on every command, not targets that are not found in it.
        let in_dir_path = resolve_in_dir(&self.in_dir)?;
        crate::core::require_in_dir_live(&self.in_dir, &in_dir_path)?;
        let resolved = OnCommand {
            in_dir_path,
            ..self.clone()
        };
        resolved.run()
    }

    /// `execute` once `--in` is resolved
    fn run(&self) -> Result<()> {
        let targets = flatten_targets(&self.target);

        // For single target, use original behavior
        if targets.len() == 1 {
            return match parse_target(&targets[0]) {
                TargetType::Port(port) => self.show_process_on_port(port),
                TargetType::Pid(pid) => self.show_ports_for_pid(pid),
                TargetType::Name(name) => self.show_ports_for_name(&name),
            };
        }

        if self.json {
            return self.show_multi_json(&targets);
        }

        // Multi-target handling
        let mut not_found = Vec::new();

        for target in &targets {
            match parse_target(target) {
                TargetType::Port(port) => {
                    if let Err(e) = self.show_process_on_port(port) {
                        out_println!("{} Port {}: {}", "⚠".yellow(), port, e);
                        not_found.push(target.clone());
                    }
                }
                TargetType::Pid(pid) => {
                    if let Err(e) = self.show_ports_for_pid(pid) {
                        out_println!("{} PID {}: {}", "⚠".yellow(), pid, e);
                        not_found.push(target.clone());
                    }
                }
                TargetType::Name(ref name) => {
                    if let Err(e) = self.show_ports_for_name(name) {
                        out_println!("{} '{}': {}", "⚠".yellow(), name, e);
                        not_found.push(target.clone());
                    }
                }
            }
        }

        if not_found.len() == targets.len() {
            // Each target already printed its own warning
            return Err(ProcError::reported(
                ExitCode::NotFound,
                format!("No process found matching '{}'", self.target.join(" ")),
            ));
        }

        Ok(())
    }

    /// Multi-target JSON: one envelope with a result per matched process
    fn show_multi_json(&self, targets: &[String]) -> Result<()> {
        let mut results: Vec<TargetResult> = Vec::new();
        let mut not_found: Vec<String> = Vec::new();
        // the ports whose holder proc can't see
        let mut unseen: Vec<(String, UnseenPort)> = Vec::new();
        // the processes proc runs under that a name matched and left out
        let mut left_out: Vec<(String, Ancestor)> = Vec::new();

        for target in targets {
            let found = match parse_target(target) {
                TargetType::Port(port) => self.lookup_port(port, 0, true).map(port_hits),
                TargetType::Pid(pid) => self.lookup_pid(pid).map(|found| vec![found]),
                TargetType::Name(ref name) => self.lookup_name(name).map(|(found, left)| {
                    left_out.extend(left.into_iter().map(|a| (target.clone(), a)));
                    found
                }),
            };

            match found {
                Ok(found) if !found.is_empty() => {
                    results.extend(found.into_iter().map(|(process, ports)| TargetResult {
                        target: target.clone(),
                        process,
                        ports,
                    }))
                }
                Err(ProcError::PortUnseen(port)) => {
                    unseen.push((target.clone(), port));
                    not_found.push(target.clone());
                }
                Err(ProcError::AncestorsOnly { ancestors, .. }) => {
                    left_out.extend(ancestors.into_iter().map(|a| (target.clone(), a)));
                    not_found.push(target.clone());
                }
                _ => not_found.push(target.clone()),
            }
        }

        if results.is_empty() {
            return Err(nothing_resolved(self.target.join(" "), &unseen, &left_out));
        }
        let left_out = Resolved {
            left_out,
            ..Resolved::default()
        }
        .left_out_ancestors();

        Printer::from_flags(true, self.verbose).print_json(&MultiTargetOutput {
            action: "on",
            success: true,
            count: results.len(),
            results,
            not_found,
            left_out,
        });

        Ok(())
    }

    /// Check if process matches --in filter
    fn matches_in_filter(&self, proc: &Process) -> bool {
        self.in_dir_path
            .as_ref()
            .is_none_or(|dir_path| crate::core::filters::matches_in_filter(proc, dir_path))
    }

    /// Check if process matches --by filter
    fn matches_by_filter(&self, proc: &Process) -> bool {
        if let Some(ref name) = self.by_name {
            crate::core::matches_by_filter(proc, name, self.mode())
        } else {
            true
        }
    }

    /// How names and `--by` match
    fn mode(&self) -> NameMatch {
        NameMatch::from_flag(self.cmd)
    }

    /// `--in` and `--by`, for naming the ones that rule a target out
    fn guards(&self) -> Guards {
        Guards::new(&self.in_dir, &self.by_name, None)
    }

    /// Check if process matches all filters (--in and --by)
    fn matches_filters(&self, proc: &Process) -> bool {
        self.matches_in_filter(proc) && self.matches_by_filter(proc)
    }

    /// Find every listener on a port with its process, applying --in/--by.
    /// The connections to the port are read with every holder, whatever the
    /// filters keep, and while the CPU is being measured.
    fn lookup_port(&self, port: u16, limit: usize, split: bool) -> Result<PortHolders> {
        // The socket tables are read once, for the holders and for the connections
        let reading = SocketReading::read()?;
        let listeners = visible_holders(port, holders_in(reading.ports, port), PortInfo::unseen)?;
        if listeners.is_empty() {
            return Err(ProcError::PortNotFound(port));
        }

        let reading = SocketReading {
            ports: listeners.clone(),
            tables: reading.tables,
        };
        let mut reader = ConnectionReader::from_reading(reading, limit, split);
        let kept = reader.read_during(&[port], || {
            let mut holders = Vec::new();
            for listener in &listeners {
                let process = Process::find_by_pid(listener.pid)?;
                holders.push((listener.clone(), process));
            }
            self.filter_holders(port, holders)
        })?;
        Ok(PortHolders { kept, reader })
    }

    /// Apply --in/--by to the holders of a port and take the CPU reading of
    /// the ones that stay, all in one wait. A holder whose process is `None`
    /// ended after its port was listed, and is kept without one. When the
    /// filters leave no holder the port counts as not found.
    fn filter_holders(
        &self,
        port: u16,
        holders: Vec<(PortInfo, Option<Process>)>,
    ) -> Result<Vec<(PortInfo, Option<Process>)>> {
        let found: Vec<Process> = holders.iter().filter_map(|(_, p)| p.clone()).collect();
        let mut kept: Vec<(PortInfo, Option<Process>)> = holders
            .into_iter()
            .filter(|(_, process)| process.as_ref().is_none_or(|p| self.matches_filters(p)))
            .collect();
        if kept.is_empty() {
            return Err(ProcError::FilteredOut {
                target: format!(":{port}"),
                ruled_out_by: ruled_out_by(&found, &self.guards(), self.mode())?,
            });
        }

        // Real CPU reading for what's shown
        let mut live: Vec<Process> = kept.iter().filter_map(|(_, p)| p.clone()).collect();
        Process::measure_cpu(&mut live);
        let mut live = live.into_iter();
        for (_, slot) in kept.iter_mut() {
            if slot.is_some() {
                *slot = live.next();
            }
        }
        Ok(kept)
    }

    /// How many peers a lookup reads: the card lists them all, the JSON of a
    /// process or a name carries the counts alone
    fn peer_limit(&self) -> usize {
        if self.json {
            0
        } else {
            usize::MAX
        }
    }

    /// Show what process is on a specific port
    fn show_process_on_port(&self, port: u16) -> Result<()> {
        if self.json {
            let mut found = self.lookup_port(port, JSON_PEER_LIMIT, false)?;
            let printer = Printer::from_flags(true, self.verbose);
            let (first, _) = &found.kept[0];
            let processes: Vec<&Process> =
                found.kept.iter().filter_map(|(_, p)| p.as_ref()).collect();
            let output = PortLookupOutput {
                action: "on",
                query_type: "port_to_process",
                success: true,
                port: Some(first.port),
                protocol: Some(format!("{:?}", first.protocol).to_lowercase()),
                address: first.address.clone(),
                exposure: Some(first.exposure()),
                connections: found.reader.of_port(port),
                // the first holder, as before `processes` existed
                process: processes.first().copied(),
                processes: Some(processes),
                ports: None,
            };
            printer.print_json(&output);
        } else {
            let found = self.lookup_port(port, usize::MAX, true)?;
            let mut reader = found.reader;
            let holders: Vec<(HeldPort, Option<Process>)> = found
                .kept
                .into_iter()
                .map(|(info, process)| {
                    let connections = reader.of_holder(info.port, info.pid);
                    (
                        HeldPort {
                            port: info,
                            connections,
                        },
                        process,
                    )
                })
                .collect();
            // A failed write is recorded by `out`, which decides the exit code
            out::recorded(self.print_process_on_port(
                &mut out::stdout(),
                port,
                &holders,
                &card_options(self.verbose),
            ));
        }

        Ok(())
    }

    /// Find a process by PID and its listening ports, applying --in/--by
    fn lookup_pid(&self, pid: u32) -> Result<(Process, Vec<HeldPort>)> {
        let mut process = Process::find_by_pid(pid)?
            .ok_or_else(|| ProcError::ProcessNotFound(pid.to_string()))?;

        // Apply --in and --by filters if present
        if !self.matches_filters(&process) {
            return Err(ProcError::FilteredOut {
                target: pid.to_string(),
                ruled_out_by: ruled_out_by(&[process], &self.guards(), self.mode())?,
            });
        }

        // Real CPU reading for what's shown, while the connections are read
        let held = measure_with_ports(std::slice::from_mut(&mut process), self.peer_limit())?
            .pop()
            .unwrap_or_default();
        Ok((process, held))
    }

    /// Show what a PID is: its card, with the ports it listens on
    fn show_ports_for_pid(&self, pid: u32) -> Result<()> {
        let (process, ports) = self.lookup_pid(pid)?;

        if self.json {
            let printer = Printer::from_flags(true, self.verbose);
            let output = PortLookupOutput {
                action: "on",
                query_type: "process_to_ports",
                success: true,
                port: None,
                protocol: None,
                address: None,
                exposure: None,
                connections: None,
                process: Some(&process),
                processes: None,
                ports: Some(&ports),
            };
            printer.print_json(&output);
        } else {
            out::recorded(write_process(
                &mut out::stdout(),
                &process,
                shown_ports(&process, &ports),
                &card_options(self.verbose),
            ));
        }

        Ok(())
    }

    /// Find processes by name and their listening ports, applying --in/--by,
    /// and the processes proc runs under that the name matched and left out
    #[allow(clippy::type_complexity)]
    fn lookup_name(&self, name: &str) -> Result<(Vec<(Process, Vec<HeldPort>)>, Vec<Ancestor>)> {
        // A name that matches nothing is an error here, never an empty list. A
        // process proc runs under is kept, marked, when its name matches
        let lookup = Process::lookup_name(name, NameMatch::from_flag(self.cmd), Ancestors::Mark)?;
        let left_out = lookup.left_out;
        let mut processes = lookup.processes;

        // Apply --in and --by filters if present
        if self.in_dir.is_some() || self.by_name.is_some() {
            let found = processes.clone();
            processes.retain(|p| self.matches_filters(p));
            if processes.is_empty() {
                return Err(ProcError::FilteredOut {
                    target: name.to_string(),
                    ruled_out_by: ruled_out_by(&found, &self.guards(), self.mode())?,
                });
            }
        }

        // One reading of the port table for all of them, and real CPU
        // readings for what's shown, one wait for all of them, while the
        // connections are read
        let held = measure_with_ports(&mut processes, self.peer_limit())?;
        Ok((processes.into_iter().zip(held).collect(), left_out))
    }

    /// Show what ports processes with a given name are listening on
    fn show_ports_for_name(&self, name: &str) -> Result<()> {
        let (all_results, left_out) = self.lookup_name(name)?;

        if self.json {
            let printer = Printer::from_flags(true, self.verbose);
            let results: Vec<_> = all_results
                .iter()
                .map(|(proc, ports)| ProcessPortsJson {
                    process: proc,
                    ports,
                })
                .collect();
            let output = MultiProcessPortsOutput {
                action: "on",
                success: true,
                count: results.len(),
                results: &results,
                left_out: &left_out,
            };
            printer.print_json(&output);
        } else {
            let printer = Printer::from_flags(false, self.verbose);
            if let Some(warning) = left_out_warning(&left_out) {
                printer.warning(&warning);
            }
            out::recorded(self.print_processes_named(
                &mut out::stdout(),
                name,
                &all_results,
                &card_options(self.verbose),
            ));
            let shown: Vec<Process> = all_results.iter().map(|(p, _)| p.clone()).collect();
            if let Some(note) = includes_note(&shown) {
                printer.note(&note);
            }
        }

        Ok(())
    }

    /// What is on `port`: the line saying so, then the card of each holder
    fn print_process_on_port(
        &self,
        out: &mut impl Write,
        port: u16,
        holders: &[(HeldPort, Option<Process>)],
        opts: &CardOptions,
    ) -> std::io::Result<()> {
        writeln!(
            out,
            "{} Port {} is used by {}{}",
            "✓".green().bold(),
            port.to_string().cyan().bold(),
            processes(holders.len()),
            exposure_suffix(holders)
        )?;
        writeln!(out)?;

        for (held, process) in holders {
            match process {
                Some(process) => {
                    write_process_card(out, process, Some(std::slice::from_ref(held)), opts)?
                }
                None => write_ended(out, held)?,
            }
        }
        Ok(())
    }

    /// The processes a name matched: how many, then a card for each
    fn print_processes_named(
        &self,
        out: &mut impl Write,
        name: &str,
        results: &[(Process, Vec<HeldPort>)],
        opts: &CardOptions,
    ) -> std::io::Result<()> {
        // `--cmd` matches the command line too, so the name is not what matched
        let how = if self.cmd { "matching" } else { "named" };
        writeln!(
            out,
            "{} {} {} '{}'",
            "✓".green().bold(),
            processes(results.len()),
            how,
            name.white().bold()
        )?;
        writeln!(out)?;
        for (process, ports) in results {
            write_process_card(out, process, shown_ports(process, ports), opts)?;
        }
        Ok(())
    }
}

/// `1 process`, `2 processes`
fn processes(count: usize) -> String {
    if count == 1 {
        "1 process".to_string()
    } else {
        format!("{count} processes")
    }
}

/// What the holders of a port share in reach: `, reachable from this machine
/// only` when every one is bound to loopback, `, reachable from the network`
/// when none is, and nothing when they differ
fn exposure_suffix(holders: &[(HeldPort, Option<Process>)]) -> &'static str {
    let local = holders
        .iter()
        .filter(|(held, _)| held.port.exposure() == Exposure::Local)
        .count();
    if local == holders.len() {
        ", reachable from this machine only"
    } else if local == 0 {
        ", reachable from the network"
    } else {
        ""
    }
}

/// A holder whose process ended after its port was listed: the two things the
/// port table knows, laid out as the card lays them
fn write_ended(out: &mut impl Write, held: &HeldPort) -> std::io::Result<()> {
    let port = &held.port;
    writeln!(
        out,
        "  {:<13}{} (PID {})",
        "Process:", port.process_name, port.pid
    )?;
    let on = port
        .address
        .as_deref()
        .map_or(String::new(), |address| format!(" on {address}"));
    writeln!(
        out,
        "  {:<13}:{} {}{on} ({})",
        "Listening:",
        port.port,
        format!("{:?}", port.protocol).to_uppercase(),
        port.exposure_words()
    )?;
    writeln!(out)
}

/// The listeners a port lookup found: the ones `--in`/`--by` keep, with their
/// processes, and every one there is
struct PortHolders {
    kept: Vec<(PortInfo, Option<Process>)>,
    /// Holds every holder of the port, kept or not, and its connections
    reader: ConnectionReader,
}

/// What a port lookup contributes to the multi-target JSON: each holder's
/// process with the port it holds. A holder whose process ended after the port
/// was listed adds nothing.
fn port_hits(found: PortHolders) -> Vec<(Process, Vec<HeldPort>)> {
    let mut reader = found.reader;
    found
        .kept
        .into_iter()
        .filter_map(|(port, process)| {
            let process = process?;
            let connections = reader.of_holder(port.port, port.pid);
            Some((process, vec![HeldPort { port, connections }]))
        })
        .collect()
}

#[derive(Serialize)]
struct PortLookupOutput<'a> {
    action: &'static str,
    query_type: &'static str,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    address: Option<String>,
    /// Of the first holder, as `address` is
    #[serde(skip_serializing_if = "Option::is_none")]
    exposure: Option<Exposure>,
    /// Every connection to the port, any holder's (a port query only)
    #[serde(skip_serializing_if = "Option::is_none")]
    connections: Option<Connections>,
    #[serde(skip_serializing_if = "Option::is_none")]
    process: Option<&'a Process>,
    /// Every process holding the port (`process` is the first of them)
    #[serde(skip_serializing_if = "Option::is_none")]
    processes: Option<Vec<&'a Process>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ports: Option<&'a [HeldPort]>,
}

#[derive(Serialize)]
struct ProcessPortsJson<'a> {
    process: &'a Process,
    ports: &'a [HeldPort],
}

#[derive(Serialize)]
struct TargetResult {
    target: String,
    process: Process,
    ports: Vec<HeldPort>,
}

#[derive(Serialize)]
struct MultiTargetOutput {
    action: &'static str,
    success: bool,
    count: usize,
    results: Vec<TargetResult>,
    not_found: Vec<String>,
    /// The processes proc runs under that a target matched and that were left
    /// out; absent when there are none
    #[serde(skip_serializing_if = "Vec::is_empty")]
    left_out: Vec<Ancestor>,
}

#[derive(Serialize)]
struct MultiProcessPortsOutput<'a> {
    action: &'static str,
    success: bool,
    count: usize,
    results: &'a Vec<ProcessPortsJson<'a>>,
    /// The processes proc runs under that the name matched and that were left
    /// out; absent when there are none
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    left_out: &'a [Ancestor],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::held::connections_of;
    use crate::core::{ProcessStatus, Protocol};
    use crate::test_support::assert_stops_at_a_failed_write;

    fn command(in_dir: Option<&str>) -> OnCommand {
        let in_dir = in_dir.map(String::from);
        OnCommand {
            target: vec!["x".to_string()],
            // as `execute` resolves it
            in_dir_path: resolve_in_dir(&in_dir).unwrap(),
            in_dir,
            by_name: None,
            cmd: false,
            json: false,
            verbose: false,
        }
    }

    /// A process record with no working directory, which some processes really
    /// are (a zombie has none on Linux)
    fn without_cwd() -> Process {
        Process {
            pid: 1,
            name: "x".to_string(),
            exe_path: None,
            cwd: None,
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
            runs_proc: false,
        }
    }

    fn listener() -> PortInfo {
        PortInfo {
            port: 4000,
            protocol: Protocol::Tcp,
            pid: 1,
            process_name: "x".to_string(),
            address: None,
        }
    }

    #[test]
    fn a_process_with_no_working_directory_fails_in() {
        assert!(!command(Some("/")).matches_in_filter(&without_cwd()));
        assert!(command(None).matches_in_filter(&without_cwd()));
    }

    #[test]
    fn a_port_whose_process_ended_has_no_process_to_filter() {
        let holders = command(Some("/"))
            .filter_holders(4000, vec![(listener(), None)])
            .unwrap();
        assert_eq!(holders.len(), 1);
        assert_eq!(holders[0].0.port, 4000);
        assert!(holders[0].1.is_none());
    }

    #[test]
    fn the_filters_drop_the_holders_they_do_not_match() {
        let mut elsewhere = without_cwd();
        elsewhere.pid = 2;
        elsewhere.cwd = Some("/definitely/not/here".to_string());
        let mut here = without_cwd();
        here.pid = 3;
        here.cwd = Some(
            std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );

        let holders = vec![
            (listener(), Some(elsewhere)),
            (listener(), Some(here.clone())),
            (listener(), None),
        ];
        let kept = command(Some("."))
            .filter_holders(4000, holders.clone())
            .unwrap();
        let pids: Vec<_> = kept
            .iter()
            .map(|(_, p)| p.as_ref().map(|p| p.pid))
            .collect();
        assert_eq!(pids, vec![Some(3), None]);

        // nothing left: the port counts as not found
        let none = command(Some("/definitely/not/here/either"))
            .filter_holders(4000, holders[..1].to_vec());
        assert!(
            matches!(&none, Err(ProcError::FilteredOut { target, .. }) if target == ":4000"),
            "{none:?}"
        );
    }

    /// A holder as the lookups hand it to the printers
    fn held(port: PortInfo, connections: Option<Connections>) -> HeldPort {
        HeldPort { port, connections }
    }

    /// A listener on `address`, held by `pid`
    fn bound(pid: u32, address: &str) -> PortInfo {
        let mut port = listener();
        port.pid = pid;
        port.address = Some(address.to_string());
        port
    }

    /// The card as a terminal 100 columns wide draws it at a fixed time
    fn opts() -> CardOptions {
        CardOptions {
            verbose: false,
            width: 100,
            now_secs: 1_000,
        }
    }

    #[test]
    fn a_port_whose_process_ended_adds_nothing_to_the_json_results() {
        let found = |kept| PortHolders {
            kept,
            reader: ConnectionReader::new(vec![listener()], 0, true),
        };
        assert!(port_hits(found(vec![(listener(), None)])).is_empty());
        let hits = port_hits(found(vec![
            (listener(), Some(without_cwd())),
            (listener(), None),
            (listener(), Some(without_cwd())),
        ]));
        assert_eq!(hits.len(), 2);
    }

    /// What a print function writes, with colours off
    fn printed(print: impl FnOnce(&mut Vec<u8>) -> std::io::Result<()>) -> String {
        colored::control::set_override(false);
        let mut out = Vec::new();
        print(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn a_port_whose_process_ended_prints_what_the_port_table_knows() {
        // no address: the port counts as reachable, and says so
        let out = printed(|w| {
            command(None).print_process_on_port(w, 4000, &[(held(listener(), None), None)], &opts())
        });
        assert_eq!(
            out,
            "✓ Port 4000 is used by 1 process, reachable from the network\n\n\
             \x20 Process:     x (PID 1)\n\
             \x20 Listening:   :4000 TCP (address not known, treated as reachable from the network)\n\n"
        );
        let out = printed(|w| {
            command(None).print_process_on_port(
                w,
                4000,
                &[(held(bound(1, "127.0.0.1"), None), None)],
                &opts(),
            )
        });
        assert!(
            out.contains("Listening:   :4000 TCP on 127.0.0.1 (this machine only)\n\n"),
            "{out}"
        );
    }

    #[test]
    fn the_headline_says_how_far_the_holders_reach() {
        let headline = |holders: Vec<PortInfo>| {
            let held: Vec<_> = holders
                .into_iter()
                .map(|port| (held(port, None), None))
                .collect();
            let out = printed(|w| command(None).print_process_on_port(w, 4000, &held, &opts()));
            out.lines().next().unwrap().to_string()
        };
        let local = || bound(1, "127.0.0.1");
        let wide = || bound(2, "0.0.0.0");
        assert_eq!(
            headline(vec![local()]),
            "✓ Port 4000 is used by 1 process, reachable from this machine only"
        );
        assert_eq!(
            headline(vec![local(), bound(2, "::1")]),
            "✓ Port 4000 is used by 2 processes, reachable from this machine only"
        );
        assert_eq!(
            headline(vec![wide()]),
            "✓ Port 4000 is used by 1 process, reachable from the network"
        );
        assert_eq!(
            headline(vec![wide(), bound(3, "192.168.1.5")]),
            "✓ Port 4000 is used by 2 processes, reachable from the network"
        );
        // one of each: nothing to say about all of them
        assert_eq!(
            headline(vec![local(), wide()]),
            "✓ Port 4000 is used by 2 processes"
        );
    }

    #[test]
    fn a_port_with_several_holders_prints_a_card_each_with_a_blank_line_between() {
        let holders = [
            (held(bound(1, "127.0.0.1"), None), Some(detailed())),
            (held(bound(2, "::1"), None), Some(without_cwd())),
        ];
        let out = printed(|w| command(None).print_process_on_port(w, 4000, &holders, &opts()));
        assert_eq!(out.matches("  Process:").count(), 2, "{out}");
        assert!(
            out.contains("Listening:   :4000 TCP on ::1 (this machine only)\n\n"),
            "{out}"
        );
        // the headline, a blank line, a card, a blank line, a card, a blank line
        let blocks: Vec<&str> = out.split("\n\n").collect();
        assert_eq!(blocks.len(), 4, "{out}");
        assert_eq!(blocks[3], "");
    }

    #[test]
    fn a_holders_card_lists_its_own_connections() {
        let holders = [(
            held(bound(1, "127.0.0.1"), Some(connections_of(1, 7))),
            Some(detailed()),
        )];
        let out = printed(|w| command(None).print_process_on_port(w, 4000, &holders, &opts()));
        assert!(out.contains("Connections: 7 established\n"), "{out}");
        assert!(out.contains("… and 2 more (-v for all)"), "{out}");
        assert!(out.contains("Command:     x --serve"), "{out}");

        let mut verbose = opts();
        verbose.verbose = true;
        let out = printed(|w| command(None).print_process_on_port(w, 4000, &holders, &verbose));
        assert!(out.contains("127.0.0.1:50006"), "{out}");
        assert!(!out.contains("more"), "{out}");
    }

    #[test]
    fn a_name_says_how_many_processes_matched() {
        let results = |n: usize| -> Vec<(Process, Vec<HeldPort>)> {
            (0..n).map(|_| (without_cwd(), Vec::new())).collect()
        };
        let named = |cmd: bool, n: usize| {
            let mut c = command(None);
            c.cmd = cmd;
            printed(|w| c.print_processes_named(w, "x", &results(n), &opts()))
        };
        let one = named(false, 1);
        assert!(
            one.starts_with("✓ 1 process named 'x'\n\n  Process:"),
            "{one}"
        );
        let two = named(false, 2);
        assert!(two.starts_with("✓ 2 processes named 'x'\n\n"), "{two}");
        assert_eq!(two.matches("  Process:").count(), 2, "{two}");
        assert!(
            named(true, 2).starts_with("✓ 2 processes matching 'x'\n\n"),
            "--cmd matches the command line too"
        );
    }

    #[test]
    fn a_process_reads_the_peers_a_card_lists_and_the_json_only_counts() {
        let mut cmd = command(None);
        assert_eq!(cmd.peer_limit(), usize::MAX);
        cmd.json = true;
        assert_eq!(cmd.peer_limit(), 0);
    }

    /// A process with every detail the printers can show
    fn detailed() -> Process {
        let mut p = without_cwd();
        p.cwd = Some("/work/app".to_string());
        p.exe_path = Some("/usr/bin/x".to_string());
        p.command = Some("x --serve".to_string());
        p.uptime_secs = Some(90);
        p
    }

    #[test]
    fn a_failed_write_ends_the_port_lookup() {
        let cmd = command(None);
        let ports = [(
            held(bound(1, "127.0.0.1"), Some(connections_of(1, 2))),
            Some(detailed()),
        )];
        assert_stops_at_a_failed_write(|w| cmd.print_process_on_port(w, 4000, &ports, &opts()));
        // a holder whose process ended
        assert_stops_at_a_failed_write(|w| {
            cmd.print_process_on_port(w, 4000, &[(held(listener(), None), None)], &opts())
        });
    }

    #[test]
    fn a_failed_write_ends_the_name_lookup() {
        let cmd = command(None);
        let ports = vec![held(bound(1, "127.0.0.1"), Some(connections_of(1, 2)))];
        let results = [(detailed(), ports)];
        assert_stops_at_a_failed_write(|w| cmd.print_processes_named(w, "x", &results, &opts()));
    }
}
