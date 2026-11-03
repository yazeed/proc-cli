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

use super::held::{card_options, measure_with_known_ports, write_process, HeldPort};
use crate::core::{
    flatten_targets, guard_targets, resolve_target, Guards, NameMatch, Process, Resolved,
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
    #[arg(long = "start-time", value_name = "SECONDS")]
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
        let resolved = resolve_all(&all_targets, |target| resolve_target(target, mode));
        let warning = resolved.warning();
        let Resolved {
            processes: mut found,
            not_found,
            ..
        } = resolved;

        // Apply --in and --by: guards that rule out everything found say so
        let guards = Guards::new(&self.in_dir, &self.by_name, self.start_time);
        guard_targets(&mut found, &self.targets, &guards, mode)?;

        // Real CPU readings for what's shown, while the ports they listen on
        // and the connections to them are read: the card lists the peers, the
        // JSON carries the counts
        let peer_limit = if self.json { 0 } else { usize::MAX };
        // (a port table that cannot be read leaves the ports out)
        let held = measure_with_known_ports(&mut found, peer_limit);

        if self.json {
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

            if let Some(warning) = warning {
                printer.warning(&warning);
            }
        }

        if found.is_empty() {
            return Err(ProcError::reported(
                ExitCode::NotFound,
                format!("No process found matching '{}'", self.targets.join(" ")),
            ));
        }

        Ok(())
    }
}

/// Resolve every target. Returns the processes found, each once however many
/// targets name it, and the targets that matched nothing.
fn resolve_all(targets: &[String], resolve: impl Fn(&str) -> Result<Vec<Process>>) -> Resolved {
    let mut found = Vec::new();
    let mut not_found = Vec::new();
    let mut hints = Vec::new();
    let mut seen_pids = std::collections::HashSet::new();

    for target in targets {
        match resolve(target) {
            Ok(processes) => {
                if processes.is_empty() {
                    not_found.push(target.clone());
                } else {
                    for proc in processes {
                        // Deduplicate by PID
                        if seen_pids.insert(proc.pid) {
                            found.push(proc);
                        }
                    }
                }
            }
            Err(error) => {
                not_found.push(target.clone());
                hints.extend(error.command_line_hint());
            }
        }
    }

    Resolved {
        processes: found,
        not_found,
        hints,
    }
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
        }
    }

    fn targets(list: &[&str]) -> Vec<String> {
        list.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn a_target_that_resolves_to_nothing_is_not_found() {
        let resolved = resolve_all(&targets(&["ghost"]), |_| Ok(Vec::new()));
        assert!(resolved.processes.is_empty());
        assert_eq!(resolved.not_found, vec!["ghost"]);
    }

    #[test]
    fn a_target_that_fails_to_resolve_is_not_found() {
        let resolved = resolve_all(&targets(&["a", "b"]), |t| match t {
            "a" => Ok(vec![process(1)]),
            _ => Err(ProcError::ProcessNotFound(t.to_string())),
        });
        assert_eq!(resolved.processes.len(), 1);
        assert_eq!(resolved.not_found, vec!["b"]);
        assert!(resolved.hints.is_empty());
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
            "a" => Ok(vec![process(1), process(2)]),
            "b" => Ok(vec![process(2), process(3)]),
            _ => Ok(vec![process(1)]),
        });
        let pids: Vec<u32> = resolved.processes.iter().map(|p| p.pid).collect();
        assert_eq!(pids, vec![1, 2, 3]);
        assert!(resolved.not_found.is_empty());
        assert_eq!(resolved.warning(), None);
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
