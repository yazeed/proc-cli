//! Info command - Get detailed process information
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

use crate::core::{
    flatten_targets, guard_targets, resolve_target, Guards, NameMatch, Process, Resolved,
};
use crate::error::{ExitCode, ProcError, Result};
use crate::out_print;
use crate::ui::{colorize_status, format_duration, format_memory, Printer};
use clap::Args;
use colored::*;
use serde::Serialize;
use std::fmt::Write;

/// Show detailed process information
#[derive(Args, Debug)]
pub struct InfoCommand {
    /// Target(s): PID, :port, or name (separate arguments for several)
    #[arg(required = true, num_args = 1.., value_parser = crate::core::validate_target_arg)]
    targets: Vec<String>,

    /// Output as JSON
    #[arg(long, short = 'j')]
    json: bool,

    /// Show extra details
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

        // Real CPU readings for what's shown
        Process::measure_cpu(&mut found);

        if self.json {
            printer.print_json(&InfoOutput {
                action: "info",
                success: !found.is_empty(),
                found_count: found.len(),
                not_found_count: not_found.len(),
                processes: &found,
                not_found: &not_found,
            });
        } else {
            for proc in &found {
                self.print_process_info(proc);
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

    fn print_process_info(&self, proc: &Process) {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        out_print!("{}", render_process_info(proc, self.verbose, now_secs));
    }
}

/// The block `info` prints for one process: one line per fact that is known
/// (`now_secs` is the time since the Unix epoch, for the uptime).
fn render_process_info(proc: &Process, verbose: bool, now_secs: u64) -> String {
    // Writing to a String can't fail
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} Process {}",
        "✓".green().bold(),
        proc.pid.to_string().cyan().bold()
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "  {} {}",
        "Name:".bright_black(),
        proc.name.white().bold()
    );
    let _ = writeln!(
        out,
        "  {} {}",
        "PID:".bright_black(),
        proc.pid.to_string().cyan()
    );

    if let Some(ref cwd) = proc.cwd {
        let _ = writeln!(out, "  {} {}", "Directory:".bright_black(), cwd);
    }

    if let Some(ref path) = proc.exe_path {
        let _ = writeln!(out, "  {} {}", "Path:".bright_black(), path);
    }

    if let Some(ref user) = proc.user {
        let _ = writeln!(out, "  {} {}", "User:".bright_black(), user);
    }

    if let Some(ppid) = proc.parent_pid {
        let _ = writeln!(
            out,
            "  {} {}",
            "Parent PID:".bright_black(),
            ppid.to_string().cyan()
        );
    }

    let status_str = format!("{:?}", proc.status);
    let status_colored = colorize_status(&proc.status, &status_str);
    let _ = writeln!(out, "  {} {}", "Status:".bright_black(), status_colored);

    let _ = writeln!(out, "  {} {:.1}%", "CPU:".bright_black(), proc.cpu_percent);
    let _ = writeln!(
        out,
        "  {} {}",
        "Memory:".bright_black(),
        format_memory(proc.memory_mb)
    );

    if let Some(start_time) = proc.start_time {
        let uptime = format_duration(now_secs.saturating_sub(start_time));
        let _ = writeln!(out, "  {} {}", "Uptime:".bright_black(), uptime);
    }

    if verbose {
        if let Some(ref cmd) = proc.command {
            let _ = writeln!(
                out,
                "  {} {}",
                "Command:".bright_black(),
                cmd.bright_black()
            );
        }
    }

    let _ = writeln!(out);
    out
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

#[derive(Serialize)]
struct InfoOutput<'a> {
    action: &'static str,
    success: bool,
    found_count: usize,
    not_found_count: usize,
    processes: &'a [Process],
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

    fn bare(pid: u32) -> Process {
        Process {
            name: "worker".to_string(),
            ..process(pid)
        }
    }

    fn render(proc: &Process, verbose: bool, now_secs: u64) -> String {
        colored::control::set_override(false);
        render_process_info(proc, verbose, now_secs)
    }

    #[test]
    fn a_process_with_nothing_known_shows_only_what_always_is() {
        let out = render(&bare(7), true, 1_000);
        assert_eq!(
            out,
            "✓ Process 7\n\n  Name: worker\n  PID: 7\n  Status: Sleeping\n  CPU: 0.0%\n  Memory: 0KB\n\n"
        );
    }

    #[test]
    fn every_known_fact_gets_a_line() {
        let proc = Process {
            cwd: Some("/work".to_string()),
            exe_path: Some("/bin/worker".to_string()),
            user: Some("zee".to_string()),
            parent_pid: Some(1),
            command: Some("/bin/worker --flag".to_string()),
            start_time: Some(940),
            ..bare(7)
        };
        let out = render(&proc, false, 1_000);
        for line in [
            "  Directory: /work\n",
            "  Path: /bin/worker\n",
            "  User: zee\n",
            "  Parent PID: 1\n",
            "  Uptime: 1m 0s\n",
        ] {
            assert!(out.contains(line), "{line:?} missing from\n{out}");
        }
        // The command line is for --verbose
        assert!(!out.contains("Command:"), "{out}");
        let verbose = render(&proc, true, 1_000);
        assert!(
            verbose.contains("  Command: /bin/worker --flag\n"),
            "{verbose}"
        );
    }

    #[test]
    fn a_start_time_in_the_future_reads_as_no_time_at_all() {
        // A clock behind the process's start must not underflow
        let proc = Process {
            start_time: Some(5_000),
            ..bare(7)
        };
        let out = render(&proc, false, 1_000);
        assert!(out.contains("  Uptime: 0s\n"), "{out}");
    }
}
