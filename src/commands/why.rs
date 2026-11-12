//! `proc why` - Explain why a port is busy or trace process ancestry
//!
//! Examples:
//!   proc why :3000              # Why is port 3000 busy? (ancestry + port context)
//!   proc why node               # Show ancestry for the processes named node
//!   proc why node --cmd         # ... or with node anywhere in their command line
//!   proc why 1234               # Show ancestry for PID 1234
//!   proc why :3000 :8080        # Several targets: separate arguments
//!   proc why :3000 --json       # JSON output with port context
//!
//! Commas still work (`proc why :3000,:8080`) but are deprecated.

use super::held::{card_options, measure_with_known_ports_from, write_process_card};
use crate::core::port::SocketReading;
use crate::core::process::ProcessDetails;
use crate::core::target::{includes_note, left_out_warning, lookup_target, resolve_port_in};
use crate::core::{
    flatten_targets, nothing_resolved, parse_target, Ancestor, Ancestors, NameLookup, NameMatch,
    PortInfo, Process, ProcessStatus, Protocol, TargetType,
};
use crate::error::{ExitCode, ProcError, Result};
use crate::out_println;
use crate::ui::{out, uptime_label, Printer};
use clap::Args;
use colored::*;
use serde::Serialize;
use std::collections::HashMap;

/// The socket tables if a port target read them
fn read_if_done(reading: &std::cell::OnceCell<Option<SocketReading>>) -> Option<&SocketReading> {
    reading.get().and_then(Option::as_ref)
}

/// Trace why a port is busy or show process ancestry
#[derive(Args, Debug)]
pub struct WhyCommand {
    /// Target(s): :port, PID, or process name (separate arguments for several)
    #[arg(required = true, num_args = 1.., value_parser = crate::core::validate_target_arg)]
    pub target: Vec<String>,

    /// Match name targets as a substring of the process name or full command line (like pkill -f)
    #[arg(long)]
    pub cmd: bool,

    /// Output as JSON
    #[arg(long, short = 'j')]
    pub json: bool,

    /// Print every value whole and every connection (default: cut to the terminal width, 5 peers)
    #[arg(long, short = 'v')]
    pub verbose: bool,
}

impl WhyCommand {
    /// Executes the why command, showing ancestry and port context.
    pub fn execute(&self) -> Result<()> {
        let printer = Printer::from_flags(self.json, self.verbose);

        let all_processes = Process::find_all()?;
        let pid_map: HashMap<u32, &Process> = all_processes.iter().map(|p| (p.pid, p)).collect();

        let targets = flatten_targets(&self.target);
        let mode = NameMatch::from_flag(self.cmd);

        // JSON results are collected across targets and printed as one document
        let mut json_results: Vec<WhyOutput> = Vec::new();
        let mut found_any = false;
        let mut first_error: Option<ProcError> = None;
        let mut not_found: Vec<String> = Vec::new();
        // the warning for each target that matched nothing, printed once the
        // loop knows something was found (otherwise the error says it), and
        // the names among them that are in command lines
        let mut warnings: Vec<String> = Vec::new();
        let mut command_line: Vec<(String, usize)> = Vec::new();
        // whether every target that matched nothing simply wasn't found (no
        // port proc can't see, nothing left out)
        let mut plain_not_found = true;
        // The processes proc runs under that a name matched: left out (with
        // `--cmd`) or kept, marked
        let mut left_out: Vec<Ancestor> = Vec::new();
        let mut included: Vec<Process> = Vec::new();
        // The socket tables, read once for every port target, every process
        // shown in the JSON and the cards: a port target needs them first, a
        // PID or a name reads them while the cards wait for the CPU. `None`
        // when they could not be read.
        let reading: std::cell::OnceCell<Option<SocketReading>> = std::cell::OnceCell::new();
        let read = || reading.get_or_init(|| SocketReading::read().ok()).as_ref();

        // What the human view prints for each target, once all are resolved: the
        // cards of every target are measured and their ports read together
        let mut shown: Vec<Option<Shown>> = Vec::new();

        for target_str in &targets {
            let parsed = parse_target(target_str);

            // Collect port context for port targets; every holder of the port is
            // traced below, the context is the port they share
            let for_port = matches!(parsed, TargetType::Port(_)).then(read).flatten();
            let port_context: Option<PortInfo> = match (&parsed, for_port) {
                (TargetType::Port(port), Some(reading)) => {
                    // The table is sorted by port then PID: the first row of the
                    // port is its first holder
                    reading.ports.iter().find(|p| p.port == *port).cloned()
                }
                _ => None,
            };

            // Resolve to processes, a port from the table already read
            let resolve = |t: &str| match (&parsed, for_port) {
                (TargetType::Port(port), Some(reading)) => {
                    resolve_port_in(*port, &reading.ports).map(NameLookup::of)
                }
                _ => lookup_target(t, mode, Ancestors::Mark),
            };
            let target_processes = match found_processes(target_str, resolve) {
                Ok(lookup) => {
                    left_out.extend(lookup.left_out);
                    if matches!(parsed, TargetType::Name(_)) {
                        included.extend(lookup.processes.iter().filter(|p| p.runs_proc).cloned());
                    }
                    lookup.processes
                }
                Err((error, warning)) => {
                    if let ProcError::AncestorsOnly { ref ancestors, .. } = error {
                        left_out.extend(ancestors.iter().cloned());
                    }
                    warnings.push(warning);
                    command_line.extend(error.command_line_match());
                    plain_not_found &= matches!(
                        error,
                        ProcError::ProcessNotFound(_) | ProcError::NameNotFound { .. }
                    );
                    first_error.get_or_insert(error);
                    not_found.push(target_str.clone());
                    shown.push(None);
                    continue;
                }
            };
            found_any = true;

            if self.json {
                json_results.extend(target_processes.iter().map(|proc| {
                    let chain = self.build_ancestry_chain(proc, &pid_map);
                    let ports = read()
                        .map(|reading| reading.ports.as_slice())
                        .unwrap_or_default()
                        .iter()
                        .filter(|p| p.pid == proc.pid)
                        .cloned()
                        .collect();
                    WhyOutput {
                        target: target_str.clone(),
                        port: port_context.as_ref().map(|p| p.port),
                        protocol: port_context.as_ref().map(|p| p.protocol),
                        process: WhyProcessInfo {
                            pid: proc.pid,
                            name: proc.name.clone(),
                            command: proc.command.clone(),
                            cwd: proc.cwd.clone(),
                            status: proc.status,
                            uptime_secs: proc.uptime_secs,
                            cpu_percent: proc.cpu_percent,
                            memory_mb: proc.memory_mb,
                            details: ProcessDetails::of(proc),
                        },
                        ports,
                        ancestry: chain,
                    }
                }));
                shown.push(None);
            } else {
                shown.push(Some(Shown {
                    target: target_str.clone(),
                    port_context,
                    processes: target_processes,
                }));
            }
        }

        left_out.sort_by_key(|a| a.pid);
        left_out.dedup_by_key(|a| a.pid);
        // nothing found, and every target simply not found: the error says it
        let only_not_found = !found_any && plain_not_found && left_out.is_empty();
        if !only_not_found {
            for warning in &warnings {
                printer.warning(warning);
            }
        }
        if let Some(warning) = left_out_warning(&left_out) {
            printer.warning(&warning);
        }

        if !self.json && !only_not_found {
            self.print_targets(&shown, &pid_map, read_if_done(&reading));
            included.sort_by_key(|p| p.pid);
            included.dedup_by_key(|p| p.pid);
            if let Some(note) = includes_note(&included) {
                printer.note(&note);
            }
        }

        if !found_any {
            let error =
                first_error.unwrap_or_else(|| ProcError::ProcessNotFound(self.target.join(" ")));
            // every target missing: the JSON lists them all, as for the other
            // commands (a port another user holds, or a name only in command
            // lines, keeps its own error)
            let error = if error.reason() == Some(crate::error::REASON_NOT_FOUND) {
                ProcError::NoneFound {
                    error: Box::new(error),
                    not_found: flatten_targets(&self.target),
                    command_line: Vec::new(),
                }
            } else {
                error
            };
            if self.json {
                return Err(error);
            }
            // said once, every target and the names in command lines, as
            // every other command says it
            if only_not_found {
                return Err(nothing_resolved(&self.target, &[], &[], &command_line));
            }
            // Humans already saw a warning per target
            return Err(ProcError::reported(
                ExitCode::from(&error),
                error.to_string(),
            ));
        }

        if self.json {
            printer.print_json(&WhyEnvelope {
                action: "why",
                success: true,
                count: json_results.len(),
                not_found,
                left_out,
                results: json_results,
            });
        }

        Ok(())
    }

    /// Prints each target that was found: its header, then for each of its
    /// processes the ancestry and the card. The processes of all the targets
    /// are measured and their ports read in one wait.
    fn print_targets(
        &self,
        shown: &[Option<Shown>],
        pid_map: &HashMap<u32, &Process>,
        read: Option<&SocketReading>,
    ) {
        let mut all: Vec<Process> = shown
            .iter()
            .flatten()
            .flat_map(|target| target.processes.iter().cloned())
            .collect();
        // the tables a port target read, else read while the CPU is measured
        let read = read.cloned();
        let table = move || match read {
            Some(reading) => Ok(reading),
            None => SocketReading::read(),
        };
        let mut held = measure_with_known_ports_from(table, &mut all, usize::MAX).into_iter();
        let mut processes = all.into_iter();
        let opts = card_options(self.verbose);

        for (i, target) in shown.iter().enumerate() {
            if i > 0 {
                out_println!();
            }
            let Some(target) = target else {
                continue;
            };
            // Print port header if applicable
            if let Some(ref port_info) = target.port_context {
                out_println!(
                    "{} Port {} ({}):",
                    "✓".green().bold(),
                    port_info.port.to_string().cyan().bold(),
                    format!("{:?}", port_info.protocol).to_uppercase()
                );
            } else {
                out_println!(
                    "{} Ancestry for '{}':",
                    "✓".green().bold(),
                    target.target.cyan()
                );
            }
            out_println!();

            for (proc, ports) in processes
                .by_ref()
                .take(target.processes.len())
                .zip(held.by_ref())
            {
                self.print_ancestry_with_context(&proc, pid_map);
                out_println!();
                // A failed write is recorded by `out`, which decides the exit code
                out::recorded(write_process_card(
                    &mut out::stdout(),
                    &proc,
                    ports.as_deref(),
                    &opts,
                ));
            }
        }
    }

    /// Build the ancestor chain from root down to target
    fn build_ancestry_chain(
        &self,
        target: &Process,
        pid_map: &HashMap<u32, &Process>,
    ) -> Vec<AncestryEntry> {
        ancestor_chain(target, pid_map)
            .into_iter()
            .map(|proc| AncestryEntry {
                pid: proc.pid,
                name: proc.name.clone(),
                command: proc.command.clone(),
                cwd: proc.cwd.clone(),
                status: proc.status,
                uptime_secs: proc.uptime_secs,
                is_target: proc.pid == target.pid,
                cpu_percent: proc.cpu_percent,
                memory_mb: proc.memory_mb,
                details: ProcessDetails::of(proc),
            })
            .collect()
    }

    /// Print ancestry tree with working directory and command context
    fn print_ancestry_with_context(&self, target: &Process, pid_map: &HashMap<u32, &Process>) {
        let chain = ancestor_chain(target, pid_map);

        for (i, proc) in chain.iter().enumerate() {
            let is_target = proc.pid == target.pid;
            let indent = "    ".repeat(i);
            let connector = if i == 0 { "" } else { "└── " };

            let symbol = status_indicator(proc.status);

            // Build the command summary (skip exe name, show args)
            let cmd_summary = proc
                .command
                .as_ref()
                .and_then(|c| {
                    let parts: Vec<&str> = c.split_whitespace().collect();
                    if parts.len() > 1 {
                        Some(parts[1..].join(" "))
                    } else {
                        None
                    }
                })
                .unwrap_or_default();

            if is_target {
                let cmd_part = if cmd_summary.is_empty() {
                    String::new()
                } else {
                    format!(" {}", cmd_summary)
                };
                out_println!(
                    "{}{}{} {} [{}]{}{}  {}",
                    indent.bright_black(),
                    connector.bright_black(),
                    symbol,
                    proc.name.cyan().bold(),
                    proc.pid.to_string().cyan().bold(),
                    cmd_part,
                    uptime_label(proc.uptime_secs).bright_black(),
                    "← target".yellow()
                );
            } else {
                let cmd_part = if cmd_summary.is_empty() {
                    String::new()
                } else {
                    format!(" {}", cmd_summary)
                };
                out_println!(
                    "{}{}{} {} [{}]{}{}",
                    indent.bright_black(),
                    connector.bright_black(),
                    symbol,
                    proc.name.white(),
                    proc.pid.to_string().cyan(),
                    cmd_part.bright_black(),
                    uptime_label(proc.uptime_secs).bright_black()
                );
            }
        }
    }
}

/// Resolve one target to the processes it names. An error carries the warning
/// to show a person: the resolve error's own text, or a plain "no process found"
/// if the target resolved to nothing.
fn found_processes(
    target: &str,
    resolve: impl Fn(&str) -> Result<NameLookup>,
) -> std::result::Result<NameLookup, (ProcError, String)> {
    match resolve(target) {
        Ok(lookup) if lookup.processes.is_empty() => Err((
            ProcError::ProcessNotFound(target.to_string()),
            format!("No process found for '{}'", target),
        )),
        Ok(lookup) => Ok(lookup),
        Err(e) => {
            let warning = e.to_string();
            Err((e, warning))
        }
    }
}

/// The target and its ancestors, from the root down to the target. Stops at a
/// parent that isn't running any more (or isn't listed) and after 100 levels,
/// so a loop in the parent links can't run forever.
fn ancestor_chain<'a>(target: &Process, pid_map: &HashMap<u32, &'a Process>) -> Vec<&'a Process> {
    let mut chain: Vec<&Process> = Vec::new();
    let mut current_pid = Some(target.pid);

    while let Some(pid) = current_pid {
        let Some(proc) = pid_map.get(&pid) else {
            break;
        };
        chain.push(proc);
        current_pid = proc.parent_pid;
        if chain.len() > 100 {
            break;
        }
    }

    chain.reverse();
    chain
}

/// The symbol in front of a process in the ancestry tree
fn status_indicator(status: ProcessStatus) -> ColoredString {
    match status {
        ProcessStatus::Running => "●".green(),
        ProcessStatus::Sleeping | ProcessStatus::Idle => "○".blue(),
        ProcessStatus::Stopped | ProcessStatus::DiskWait => "◐".yellow(),
        ProcessStatus::Zombie => "✗".red(),
        ProcessStatus::Starting => "◌".white(),
        _ => "?".white(),
    }
}

/// A target that was found, for the human view
struct Shown {
    target: String,
    /// The port a port target names
    port_context: Option<PortInfo>,
    processes: Vec<Process>,
}

#[derive(Serialize)]
struct WhyEnvelope {
    action: &'static str,
    success: bool,
    count: usize,
    /// The targets that matched nothing
    not_found: Vec<String>,
    /// The processes proc runs under that a target matched and that were left
    /// out; absent when there are none
    #[serde(skip_serializing_if = "Vec::is_empty")]
    left_out: Vec<Ancestor>,
    results: Vec<WhyOutput>,
}

#[derive(Serialize)]
struct WhyOutput {
    target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    protocol: Option<Protocol>,
    process: WhyProcessInfo,
    ports: Vec<PortInfo>,
    ancestry: Vec<AncestryEntry>,
}

#[derive(Serialize)]
struct WhyProcessInfo {
    pid: u32,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    status: ProcessStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    uptime_secs: Option<u64>,
    cpu_percent: f32,
    memory_mb: f64,
    #[serde(flatten)]
    details: ProcessDetails,
}

#[derive(Serialize)]
struct AncestryEntry {
    pid: u32,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    status: ProcessStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    uptime_secs: Option<u64>,
    is_target: bool,
    cpu_percent: f32,
    memory_mb: f64,
    #[serde(flatten)]
    details: ProcessDetails,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, parent_pid: Option<u32>) -> Process {
        Process {
            pid,
            name: format!("p{pid}"),
            exe_path: None,
            cwd: None,
            command: None,
            argv0: None,
            raw: None,
            cpu_percent: 0.0,
            memory_mb: 0.0,
            status: ProcessStatus::Sleeping,
            user: None,
            parent_pid,
            start_time: None,
            uptime_secs: None,
            parent_name: None,
            cpu_time_secs: None,
            open_files: None,
            child_count: None,
            runs_proc: false,
        }
    }

    fn pids(chain: &[&Process]) -> Vec<u32> {
        chain.iter().map(|p| p.pid).collect()
    }

    #[test]
    fn a_chain_runs_from_the_root_down_to_the_target() {
        let (root, mid, leaf) = (process(1, None), process(2, Some(1)), process(3, Some(2)));
        let map: HashMap<u32, &Process> =
            [&root, &mid, &leaf].iter().map(|p| (p.pid, *p)).collect();
        assert_eq!(pids(&ancestor_chain(&leaf, &map)), vec![1, 2, 3]);
    }

    #[test]
    fn a_chain_stops_at_a_parent_that_is_not_listed() {
        // The parent exited, or belongs to another user and can't be read
        let (mid, leaf) = (process(2, Some(1)), process(3, Some(2)));
        let map: HashMap<u32, &Process> = [&mid, &leaf].iter().map(|p| (p.pid, *p)).collect();
        assert_eq!(pids(&ancestor_chain(&leaf, &map)), vec![2, 3]);
    }

    #[test]
    fn a_target_that_is_not_listed_has_no_chain() {
        let map: HashMap<u32, &Process> = HashMap::new();
        assert!(ancestor_chain(&process(3, Some(2)), &map).is_empty());
    }

    #[test]
    fn a_loop_in_the_parent_links_ends_after_101_entries() {
        let (a, b) = (process(1, Some(2)), process(2, Some(1)));
        let map: HashMap<u32, &Process> = [&a, &b].iter().map(|p| (p.pid, *p)).collect();
        let chain = ancestor_chain(&a, &map);
        assert_eq!(chain.len(), 101);
        // Walked up from the target, so the chain ends on the target
        assert_eq!(chain.last().map(|p| p.pid), Some(1));
    }

    #[test]
    fn each_status_has_its_own_symbol() {
        let symbol = |status| (*status_indicator(status)).to_string();
        assert_eq!(symbol(ProcessStatus::Running), "●");
        assert_eq!(symbol(ProcessStatus::Sleeping), "○");
        assert_eq!(symbol(ProcessStatus::Stopped), "◐");
        assert_eq!(symbol(ProcessStatus::Zombie), "✗");
        assert_eq!(symbol(ProcessStatus::Dead), "?");
        assert_eq!(symbol(ProcessStatus::Unknown), "?");
        assert_eq!(symbol(ProcessStatus::Idle), "○");
        assert_eq!(symbol(ProcessStatus::Starting), "◌");
        assert_eq!(symbol(ProcessStatus::DiskWait), "◐");
    }

    #[test]
    fn a_target_that_resolves_to_nothing_is_not_found() {
        let (error, warning) = found_processes("ghost", |_| Ok(NameLookup::default())).unwrap_err();
        assert!(
            matches!(&error, ProcError::ProcessNotFound(t) if t == "ghost"),
            "{error:?}"
        );
        assert_eq!(warning, "No process found for 'ghost'");
    }

    #[test]
    fn a_target_that_fails_to_resolve_warns_with_the_error() {
        let (error, warning) =
            found_processes("ghost", |_| Err(ProcError::PortNotFound(1))).unwrap_err();
        assert!(matches!(error, ProcError::PortNotFound(1)), "{error:?}");
        assert_eq!(warning, ProcError::PortNotFound(1).to_string());
    }

    #[test]
    fn a_target_that_resolves_gives_its_processes() {
        let found = found_processes("p", |_| Ok(NameLookup::of(vec![process(5, None)]))).unwrap();
        assert_eq!(found.processes.len(), 1);
        assert_eq!(found.processes[0].pid, 5);
    }

    #[test]
    fn a_name_whose_matches_all_run_proc_warns_with_the_error() {
        let shell = crate::core::Ancestor {
            pid: 7,
            name: "zsh".to_string(),
        };
        let (error, warning) = found_processes("zsh", |t| {
            Err(ProcError::AncestorsOnly {
                name: t.to_string(),
                ancestors: vec![shell.clone()],
            })
        })
        .unwrap_err();
        assert_eq!(error.reason(), Some("ruled_out"));
        assert_eq!(
            warning,
            "zsh (PID 7) matches 'zsh', but proc runs under it, so it's left out"
        );
    }
}
