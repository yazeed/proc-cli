//! Info command - Get detailed process information
//!
//! Every process is the card (see `ui::card`), with the ports it listens on.
//!
//! Usage:
//!   proc info 1234              # Info for PID
//!   proc info :3000             # Info for process on port 3000
//!   proc info node              # Info for processes named node
//!   proc info node --cmd        # ... or with node anywhere in their command line
//!   proc info :3000 :8080       # Info for several targets: separate arguments
//!   proc info :3000 1234 node   # Mixed targets (port + PID + name)
//!
//! Commas still work (`proc info :3000,:8080`) but are deprecated.

use super::held::{card_options, measure_with_known_ports_from, write_process, HeldPort};
use crate::core::port::SocketReading;
use crate::core::target::{lookup_target, resolve_port_in};
use crate::core::{
    flatten_targets, guard_targets, nothing_resolved, parse_target, Ancestor, Ancestors, Guards,
    NameLookup, NameMatch, Process, Resolved, TargetType,
};
use crate::error::{ExitCode, ProcError, Result};
use crate::ui::{out, Printer};
use clap::Args;
use serde::Serialize;

/// Show detailed process information
#[derive(Args, Debug)]
pub struct InfoCommand {
    /// Target(s): PID, :port, or name (separate arguments for several)
    #[arg(required = true, num_args = 1.., value_parser = crate::core::validate_target_arg)]
    targets: Vec<String>,

    /// Output as JSON
    #[arg(long, short = 'j')]
    json: bool,

    /// Print every value whole and every connection (default: cut to the terminal width, 5 peers)
    #[arg(long, short = 'v')]
    verbose: bool,

    /// Filter by directory; takes the next word as the directory unless it starts with '-'
    /// (bare --in is the current directory, --in=DIR is explicit)
    #[arg(long = "in", short = 'i', num_args = 0..=1, default_missing_value = ".")]
    pub in_dir: Option<String>,

    /// Filter by process name
    #[arg(long = "by", short = 'b')]
    pub by_name: Option<String>,

    /// Show it only if the process still has this start time (Unix seconds, `start_time`
    /// in its JSON): a PID the OS gave to a new process is ruled out. One PID target
    #[arg(long = "start-time", value_name = "SECONDS", value_parser = crate::core::values::whole_seconds, allow_negative_numbers = true)]
    pub start_time: Option<u64>,

    /// Match name targets and --by as a substring of the process name or full command line (like pkill -f)
    #[arg(long)]
    pub cmd: bool,
}

impl InfoCommand {
    /// Executes the info command, displaying detailed process information.
    pub fn execute(&self) -> Result<()> {
        let printer = Printer::from_flags(self.json, self.verbose);

        // Separate arguments, each of which may still be a comma-separated list
        let all_targets = flatten_targets(&self.targets);

        let mode = NameMatch::from_flag(self.cmd);
        // A port target reads the socket table, once for the command: it resolves the
        // port and the cards' ports and connections come from the same reading
        let reading = all_targets
            .iter()
            .any(|target| matches!(parse_target(target), TargetType::Port(_)))
            .then(|| SocketReading::read().ok())
            .flatten();
        let resolved = resolve_all(&all_targets, |target| {
            match (parse_target(target), &reading) {
                (TargetType::Port(port), Some(reading)) => {
                    resolve_port_in(port, &reading.ports).map(NameLookup::of)
                }
                _ => lookup_target(target, mode, Ancestors::Mark),
            }
        });
        let warning = resolved.warning();
        let note = resolved.note();
        let left_out_json = resolved.left_out_ancestors();
        let only_not_found = resolved.only_not_found();
        let Resolved {
            processes: mut found,
            not_found,
            unseen,
            left_out,
            command_line,
            ..
        } = resolved;
        // what nothing matching means: not found, or a port in use by a
        // process proc can't see
        let nothing = nothing_resolved(&self.targets, &unseen, &left_out, &command_line);

        // Apply --in and --by: guards that rule out everything found say so
        let guards = Guards::new(&self.in_dir, &self.by_name, self.start_time);
        guard_targets(&mut found, &self.targets, &guards, mode)?;

        // Real CPU readings for what's shown, while the ports they listen on
        // and the connections to them are read: the card lists the peers, the
        // JSON carries the counts
        let peer_limit = if self.json { 0 } else { usize::MAX };
        // (a port table that cannot be read leaves the ports out)
        let table = move || reading.map_or_else(SocketReading::read, Ok);
        let held = measure_with_known_ports_from(table, &mut found, peer_limit);

        if self.json {
            // Nothing matched: the document is also the error, as in every other command
            let not_found_error = if found.is_empty() {
                warning.clone()
            } else {
                None
            };
            let processes: Vec<InfoProcess> = found
                .iter()
                .zip(&held)
                .map(|(process, ports)| InfoProcess {
                    process,
                    ports: ports.as_deref(),
                })
                .collect();
            printer.print_json(&InfoOutput {
                action: "info",
                success: !found.is_empty(),
                found_count: found.len(),
                not_found_count: not_found.len(),
                processes: &processes,
                not_found: &not_found,
                left_out: &left_out_json,
                error: not_found_error.as_deref(),
                exit_code: not_found_error
                    .is_some()
                    .then_some(ExitCode::from(&nothing) as i32),
                reason: not_found_error
                    .is_some()
                    .then(|| nothing.reason())
                    .flatten(),
                count: found.len(),
            });
        } else {
            let opts = card_options(self.verbose);
            for (process, ports) in found.iter().zip(&held) {
                // A failed write is recorded by `out`, which decides the exit code
                out::recorded(write_process(
                    &mut out::stdout(),
                    process,
                    ports.as_deref(),
                    &opts,
                ));
            }

            // when nothing was found at all, the error says it instead
            if let Some(warning) = warning.filter(|_| !only_not_found) {
                printer.warning(&warning);
            }
            if let Some(note) = note {
                printer.note(&note);
            }
        }

        if found.is_empty() {
            // as every other command says it: the targets, and the names among
            // them that are in command lines
            if only_not_found && !self.json {
                return Err(nothing);
            }
            let message = match &nothing {
                ProcError::PortUnseen(_) | ProcError::AncestorsOnly { .. } => nothing.to_string(),
                _ => format!("No process found matching '{}'", self.targets.join(" ")),
            };
            return Err(ProcError::reported(ExitCode::from(&nothing), message));
        }

        Ok(())
    }
}

/// Resolve every target. Returns the processes found, each once however many
/// targets name it, the targets that matched nothing, and the processes proc
/// runs under that a name kept (marked) or left out.
fn resolve_all(targets: &[String], resolve: impl Fn(&str) -> Result<NameLookup>) -> Resolved {
    let mut resolved = Resolved::default();
    let mut seen_pids = std::collections::HashSet::new();

    for target in targets {
        match resolve(target) {
            Ok(lookup) => {
                if lookup.processes.is_empty() {
                    resolved.not_found.push(target.clone());
                }
                let by_name = matches!(parse_target(target), TargetType::Name(_));
                for proc in lookup.processes {
                    // Deduplicate by PID
                    if seen_pids.insert(proc.pid) {
                        if by_name && proc.runs_proc {
                            resolved.included.push(Ancestor::from(&proc));
                        }
                        resolved.processes.push(proc);
                    }
                }
                for ancestor in lookup.left_out {
                    resolved.left_out.push((target.clone(), ancestor));
                }
            }
            Err(error) => {
                match error {
                    ProcError::PortUnseen(port) => resolved.unseen.push((target.clone(), port)),
                    ProcError::AncestorsOnly { ref ancestors, .. } => {
                        for ancestor in ancestors {
                            resolved.left_out.push((target.clone(), ancestor.clone()));
                        }
                    }
                    _ => {}
                }
                resolved.not_found.push(target.clone());
                resolved.command_line.extend(error.command_line_match());
            }
        }
    }

    resolved
}

/// A process as `info` lists it: the process, then the ports it listens on
#[derive(Serialize)]
struct InfoProcess<'a> {
    #[serde(flatten)]
    process: &'a Process,
    /// Absent where the port table could not be read
    #[serde(skip_serializing_if = "Option::is_none")]
    ports: Option<&'a [HeldPort]>,
}

#[derive(Serialize)]
struct InfoOutput<'a> {
    action: &'static str,
    success: bool,
    found_count: usize,
    not_found_count: usize,
    processes: &'a [InfoProcess<'a>],
    not_found: &'a [String],
    /// The processes proc runs under that a target matched and that were left
    /// out; absent when there are none
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    left_out: &'a [Ancestor],
    /// With `exit_code` and `reason`, only when no target matched, as in the error of every other command
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    /// The processes found, as every other command's `count` (`found_count` is
    /// the old name for it, deprecated, removed in 3.0)
    count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ProcessStatus;

    fn process(pid: u32) -> Process {
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

    fn targets(list: &[&str]) -> Vec<String> {
        list.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn a_target_that_resolves_to_nothing_is_not_found() {
        let resolved = resolve_all(&targets(&["ghost"]), |_| Ok(NameLookup::default()));
        assert!(resolved.processes.is_empty());
        assert_eq!(resolved.not_found, vec!["ghost"]);
    }

    #[test]
    fn a_target_that_fails_to_resolve_is_not_found() {
        let resolved = resolve_all(&targets(&["a", "b"]), |t| match t {
            "a" => Ok(NameLookup::of(vec![process(1)])),
            _ => Err(ProcError::ProcessNotFound(t.to_string())),
        });
        assert_eq!(resolved.processes.len(), 1);
        assert_eq!(resolved.not_found, vec!["b"]);
        assert!(resolved.command_line.is_empty());
        assert_eq!(resolved.warning().unwrap(), "Not found: b");
    }

    #[test]
    fn a_name_in_command_lines_only_is_not_found_with_a_hint() {
        let resolved = resolve_all(&targets(&["serve"]), |t| {
            Err(ProcError::NameNotFound {
                name: t.to_string(),
                in_command_line: 2,
            })
        });
        assert_eq!(resolved.not_found, vec!["serve"]);
        assert_eq!(
            resolved.warning().unwrap(),
            "Not found: serve\n  2 processes have 'serve' in their command line (add --cmd to match those)"
        );
    }

    #[test]
    fn a_process_two_targets_name_is_found_once() {
        let resolved = resolve_all(&targets(&["a", "b", "c"]), |t| match t {
            "a" => Ok(NameLookup::of(vec![process(1), process(2)])),
            "b" => Ok(NameLookup::of(vec![process(2), process(3)])),
            _ => Ok(NameLookup::of(vec![process(1)])),
        });
        let pids: Vec<u32> = resolved.processes.iter().map(|p| p.pid).collect();
        assert_eq!(pids, vec![1, 2, 3]);
        assert!(resolved.not_found.is_empty());
        assert_eq!(resolved.warning(), None);
    }

    #[test]
    fn the_processes_proc_runs_under_are_kept_or_left_out_and_said() {
        let shell = crate::core::Ancestor {
            pid: 7,
            name: "zsh".to_string(),
        };
        let mut kept = process(7);
        kept.runs_proc = true;
        let resolved = resolve_all(&targets(&["zsh", "sh", "7"]), |t| match t {
            "zsh" => Ok(NameLookup::of(vec![kept.clone()])),
            "sh" => Err(ProcError::AncestorsOnly {
                name: t.to_string(),
                ancestors: vec![shell.clone()],
            }),
            // a PID names its process: kept, but not noted as a name's choice
            _ => Ok(NameLookup::of(vec![kept.clone()])),
        });
        assert_eq!(resolved.included, vec![Ancestor::from(&kept)]);
        assert_eq!(resolved.left_out, vec![("sh".to_string(), shell.clone())]);
        assert_eq!(resolved.not_found, vec!["sh"]);
        assert_eq!(
            resolved.warning().unwrap(),
            "Left out: zsh (PID 7), which runs this proc"
        );
    }

    #[test]
    fn a_process_is_listed_with_its_ports_beside_its_own_keys() {
        let ports = [HeldPort {
            port: crate::core::PortInfo {
                port: 4000,
                protocol: crate::core::Protocol::Tcp,
                pid: 7,
                process_name: "p7".to_string(),
                address: Some("127.0.0.1".to_string()),
            },
            connections: None,
        }];
        let proc = process(7);
        let json = serde_json::to_value(InfoProcess {
            process: &proc,
            ports: Some(&ports),
        })
        .unwrap();
        assert_eq!(json["pid"], 7);
        assert_eq!(json["ports"][0]["port"], 4000);
        assert_eq!(json["ports"][0]["exposure"], "local");
        // where the OS gives no connections the key is absent, never null
        assert!(json["ports"][0].get("connections").is_none(), "{json}");

        // a process that listens on nothing has an empty list
        let json = serde_json::to_value(InfoProcess {
            process: &proc,
            ports: Some(&[]),
        })
        .unwrap();
        assert_eq!(json["ports"], serde_json::json!([]));

        // a port table that could not be read: the key is absent, never null
        let json = serde_json::to_value(InfoProcess {
            process: &proc,
            ports: None,
        })
        .unwrap();
        assert_eq!(json["pid"], 7);
        assert!(json.get("ports").is_none(), "{json}");
    }
}
