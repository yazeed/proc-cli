//! Stop command - Graceful process termination (SIGTERM)
//!
//! Usage:
//!   proc stop 1234              # Stop PID 1234
//!   proc stop :3000             # Stop process on port 3000
//!   proc stop node              # Stop the processes named node
//!   proc stop node --cmd        # ... or with node anywhere in their command line
//!   proc stop 1234 5678         # Stop several targets: separate arguments
//!   proc stop :3000 :8080 node  # Mixed targets (port + port + name)
//!
//! Commas still work (`proc stop :3000,:8080`) but are deprecated.

#[cfg(unix)]
use crate::core::parse_signal_name;
use crate::core::{
    apply_to_each, flatten_targets, guard_targets, resolve_targets_excluding_self, Guards,
    NameMatch, Process,
};
use crate::error::{ProcError, Result};
use crate::ui::Printer;
use clap::Args;

/// Stop process(es) gracefully with SIGTERM
#[derive(Args, Debug)]
pub struct StopCommand {
    /// Target(s): process name, PID, or :port (separate arguments for several)
    #[arg(required = true, num_args = 1.., value_parser = crate::core::validate_target_arg)]
    target: Vec<String>,

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    yes: bool,

    /// Show what would be stopped without actually stopping
    #[arg(long)]
    dry_run: bool,

    /// Output as JSON
    #[arg(long, short = 'j')]
    json: bool,

    /// Add PPID, STATUS and DIR to the table of processes it lists, and cut nothing
    #[arg(long, short = 'v')]
    verbose: bool,

    /// Timeout in seconds to wait before force kill
    #[arg(long, short, default_value = "10")]
    timeout: u64,

    /// Filter by directory; takes the next word as the directory unless it starts with '-'
    /// (bare --in is the current directory, --in=DIR is explicit)
    #[arg(long = "in", short = 'i', num_args = 0..=1, default_missing_value = ".")]
    pub in_dir: Option<String>,

    /// Filter by process name
    #[arg(long = "by", short = 'b')]
    pub by_name: Option<String>,

    /// Act only if the process still has this start time (Unix seconds, `start_time`
    /// in its JSON): a PID the OS gave to a new process is ruled out. One PID target
    #[arg(long = "start-time", value_name = "SECONDS")]
    pub start_time: Option<u64>,

    /// Match name targets and --by as a substring of the process name or full command line (like pkill -f)
    #[arg(long)]
    pub cmd: bool,

    /// Initial signal to send instead of SIGTERM (e.g. HUP, USR1, INT)
    #[arg(long, short = 'S')]
    pub signal: Option<String>,
}

impl StopCommand {
    /// Executes the stop command, gracefully terminating matched processes.
    pub fn execute(&self) -> Result<()> {
        let printer =
            Printer::from_flags(self.json, self.verbose).scoped_to_dir(self.in_dir.is_some());

        // Validate --signal first, so a bad name fails the same way with or
        // without --dry-run, and before the confirmation prompt
        #[cfg(unix)]
        let custom_signal = if let Some(ref sig_name) = self.signal {
            Some(parse_signal_name(sig_name)?)
        } else {
            None
        };
        // Windows has no signals: say so rather than quietly sending a normal stop
        #[cfg(not(unix))]
        if self.signal.is_some() {
            return Err(ProcError::NotSupported(
                "stop --signal on Windows (proc stop asks the process to exit)".to_string(),
            ));
        }

        // Split each value on commas and resolve to processes
        // Use resolve_targets_excluding_self to avoid stopping ourselves
        let targets = flatten_targets(&self.target);
        let mode = NameMatch::from_flag(self.cmd);
        let resolved = resolve_targets_excluding_self(&targets, mode);

        // Warn about targets that weren't found
        if let Some(warning) = resolved.warning() {
            printer.warning(&warning);
        }
        // the JSON results list the targets that matched nothing
        let printer = printer.with_not_found(&resolved.not_found);
        let mut processes = resolved.processes;

        // Apply --in and --by: guards that rule out everything found say so
        let guards = Guards::new(&self.in_dir, &self.by_name, self.start_time);
        guard_targets(&mut processes, &self.target, &guards, mode)?;

        // Real CPU readings whenever the list is shown or returned (dry run, the
        // confirmation prompt, or the error for a missing `--yes`); only `--yes`
        // acts at once, without the wait
        if self.dry_run || !self.yes {
            Process::measure_cpu(&mut processes);
        }

        if processes.is_empty() {
            return Err(ProcError::ProcessNotFound(self.target.join(" ")));
        }

        // Dry run: just show what would be stopped
        if self.dry_run {
            printer.print_dry_run("stop", &processes);
            return Ok(());
        }

        // Confirm if not --yes
        if !printer.ask_confirm("stop", &processes, self.yes)? {
            return Ok(());
        }

        // Ask every process first, then wait for them together, so several
        // processes share one --timeout instead of taking one each
        let (signalled, mut failed) = apply_to_each(&processes, |proc| {
            #[cfg(unix)]
            if let Some(signal) = custom_signal {
                return proc.send_signal(signal);
            }
            ask_to_end(proc)
        });

        let outlived = self.wait_for_exit(&signalled);

        // Force kill whatever outlived the timeout
        let (stopped, failed_to_kill) = apply_to_each(&signalled, |proc| {
            if outlived.contains(&proc.pid) {
                proc.kill_and_wait()
            } else {
                Ok(())
            }
        });
        failed.extend(failed_to_kill);
        // the failures read in the order of the list that was approved, not
        // "signal failures, then force-kill failures"
        in_process_order(&processes, &mut failed);

        // Output results
        printer.print_action_result("stop", &processes, &stopped, &failed)
    }

    /// Wait up to --timeout for the processes to exit; returns the PIDs still running.
    fn wait_for_exit(&self, processes: &[Process]) -> Vec<u32> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(self.timeout);
        let mut running: Vec<&Process> = processes.iter().collect();

        loop {
            running.retain(|p| p.is_running());
            if running.is_empty() || start.elapsed() >= timeout {
                return running.iter().map(|p| p.pid).collect();
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

/// Put `failed` in the order `processes` lists its processes in. A process that
/// could not be asked to end and one that outlived the timeout and could not be
/// force-killed are found by different passes; this merges them.
fn in_process_order(processes: &[Process], failed: &mut [(Process, String)]) {
    failed.sort_by_key(|(failed, _)| {
        processes
            .iter()
            .position(|p| p.pid == failed.pid)
            .unwrap_or(usize::MAX)
    });
}

/// Ask one process to end (SIGTERM; on Windows, `taskkill` without `/F`)
fn ask_to_end(proc: &Process) -> Result<()> {
    match proc.terminate() {
        // Windows can't ask a console program to close (taskkill without /F
        // needs a window); it still gets stopped, by the force kill after
        // --timeout
        Err(_) if cfg!(not(unix)) && proc.is_running() => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{end, gone_record, idle_child, record_of};

    #[test]
    fn failures_from_both_passes_read_in_the_order_of_the_approved_list() {
        let mut child = idle_child();
        let record = record_of(&child);
        end(&mut child);
        let with_pid = |pid| Process {
            pid,
            ..record.clone()
        };
        // typed order, not PID order: 30 was typed first
        let approved = [with_pid(30), with_pid(10), with_pid(20), with_pid(40)];
        // the force-kill pass found 20; the signal pass found 40 and 30
        let mut failed = vec![
            (with_pid(40), "signal".to_string()),
            (with_pid(30), "signal".to_string()),
            (with_pid(20), "kill".to_string()),
        ];
        in_process_order(&approved, &mut failed);
        let order: Vec<(u32, &str)> = failed.iter().map(|(p, e)| (p.pid, e.as_str())).collect();
        assert_eq!(order, [(30, "signal"), (20, "kill"), (40, "signal")]);
    }

    #[test]
    fn a_process_that_is_gone_cannot_be_asked_to_end() {
        let mut child = idle_child();
        let record = gone_record(&mut child);
        // Not running, so the refusal is not the expected one a console
        // program gives on Windows: it is reported
        let result = ask_to_end(&record);
        assert!(
            matches!(&result, Err(ProcError::ProcessNotFound(_))),
            "{result:?}"
        );
    }

    #[test]
    fn a_running_process_is_asked_and_the_answer_does_not_matter_on_windows() {
        let mut child = idle_child();
        let record = record_of(&child);
        let result = ask_to_end(&record);
        end(&mut child);
        // Unix: SIGTERM was delivered. Windows: taskkill refuses a console
        // program, and that is not a failure.
        assert!(result.is_ok(), "{result:?}");
    }
}
