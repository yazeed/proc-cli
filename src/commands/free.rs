//! `proc free` - Free ports by killing their processes and verifying availability
//!
//! Examples:
//!   proc free :3000              # Kill process on port 3000, verify port freed
//!   proc free :3000 :8080        # Free several ports: separate arguments
//!   proc free :3000 --yes        # Skip confirmation
//!   proc free :3000 --wait 30    # Wait up to 30s for port to free
//!
//! Commas still work (`proc free :3000,:8080`) but are deprecated.

use crate::core::port::PortInfo;
use crate::core::{flatten_targets, parse_target, Process, TargetType};
use crate::error::{ExitCode, ProcError, Result};
use crate::out_println;
use crate::ui::output::ReceiptProcess;
use crate::ui::{must_confirm, plural, prompt, Printer};
use clap::Args;
use colored::*;
use serde::Serialize;

/// Free port(s): SIGTERM each holder, SIGKILL what is left after up to 5 seconds, and verify the port is available
#[derive(Args, Debug)]
pub struct FreeCommand {
    /// Port target(s): :port (separate arguments for several, e.g. :3000 :8080)
    #[arg(required = true, num_args = 1.., value_parser = crate::core::validate_target_arg)]
    pub target: Vec<String>,

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Show what would be freed without actually freeing
    #[arg(long)]
    pub dry_run: bool,

    /// Output as JSON
    #[arg(long, short = 'j')]
    pub json: bool,

    /// Add PPID, STATUS and DIR to the table of processes it lists, and cut nothing
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// Max whole seconds to wait for port to become free
    #[arg(long, default_value = "10")]
    pub wait: u64,
}

impl FreeCommand {
    /// Executes the free command, killing processes and verifying ports are freed.
    pub fn execute(&self) -> Result<()> {
        let printer = Printer::from_flags(self.json, self.verbose);

        let targets = flatten_targets(&self.target);

        // Validate all targets are ports
        let mut ports: Vec<(&String, u16)> = Vec::new();
        for target in &targets {
            match parse_target(target) {
                TargetType::Port(port) => ports.push((target, port)),
                _ => {
                    return Err(ProcError::InvalidInput(format!(
                        "proc free only works with port targets (e.g. :3000), got '{}'. Use proc kill for processes.",
                        target
                    )));
                }
            }
        }

        // Resolve ports to processes: every process holding a port
        let mut port_processes: Vec<(u16, Process)> = Vec::new();
        // the ports nobody holds, as typed (":3000")
        let mut not_found: Vec<String> = Vec::new();

        for (target, port) in &ports {
            // A port nobody listens on, or whose holders have all gone, is not found
            let mut held = false;
            for listener in PortInfo::holders_seen(*port)? {
                if let Some(proc) = Process::find_by_pid(listener.pid)? {
                    port_processes.push((*port, proc));
                    held = true;
                }
            }
            if !held {
                printer.warning(&format!("No process listening on port {}", port));
                not_found.push((*target).clone());
            }
        }
        // the JSON results list the ports that matched nothing
        let printer = printer.with_not_found(&not_found);

        // Targets are required, never empty (the parser refuses "" and ","),
        // and only ports (checked above), so with no holders every port is in
        // `not_found`: all of them are already free
        if port_processes.is_empty() {
            printer.print_empty_result("free", "All specified ports are already free");
            return Ok(());
        }

        // The ports to free, once each however many processes hold them
        let target_ports: Vec<u16> = {
            let mut seen = std::collections::HashSet::new();
            port_processes
                .iter()
                .map(|(port, _)| *port)
                .filter(|port| seen.insert(*port))
                .collect()
        };

        // Deduplicate processes (multiple ports might be held by same process)
        let mut processes: Vec<Process> = {
            let mut seen = std::collections::HashSet::new();
            port_processes
                .iter()
                .filter(|(_, p)| seen.insert(p.pid))
                .map(|(_, p)| p.clone())
                .collect()
        };

        // Real CPU readings whenever the list is shown or returned (dry run,
        // the prompt, or the error for a missing `--yes`); `--yes` acts at once
        if self.dry_run || !self.yes {
            Process::measure_cpu(&mut processes);
        }

        if self.dry_run {
            printer.print_dry_run_with(
                "free",
                &processes,
                &format!(
                    "Dry run: would kill {} process{} to free {} port{}",
                    processes.len(),
                    plural(processes.len()),
                    target_ports.len(),
                    if target_ports.len() == 1 { "" } else { "s" }
                ),
            );
            return Ok(());
        }

        // Same rule everywhere: --yes, or ask; --json never skips it
        if must_confirm("free", self.yes, self.json, &processes, Some(&not_found))? {
            let question = format!(
                "Kill {} process{} to free {} port{}?",
                processes.len(),
                plural(processes.len()),
                target_ports.len(),
                if target_ports.len() == 1 { "" } else { "s" }
            );

            if !prompt::ask("free", &processes, Some(&not_found), &question, || {
                printer.print_confirmation("free", &processes)
            })? {
                printer.warning("Aborted");
                return Ok(());
            }
        }

        // SIGTERM first, then SIGKILL for what is left
        let start = std::time::Instant::now();
        let signal_failures = stop_all(
            &processes,
            std::time::Duration::from_secs(self.wait.min(5)),
            &printer,
            Process::terminate,
            Process::kill,
        );

        // Poll until ports are free
        let mut freed: Vec<u16> = Vec::new();
        let mut still_busy: Vec<u16> = Vec::new();

        // --wait covers the whole run, graceful exit included, so it is a real
        // maximum; after a force kill the port still gets a second to be released
        let poll_deadline = std::cmp::max(
            start + std::time::Duration::from_secs(self.wait),
            std::time::Instant::now() + std::time::Duration::from_secs(1),
        );

        // A port counts as free once it reads free in two looks in a row: a
        // copy of the holder (a supervisor, a forked worker) can take it back
        // within milliseconds of the holder's death, and a single look between
        // the two says "free" about a port that is in use again
        let mut was_free = false;
        loop {
            let mut all_free = true;
            freed.clear();
            still_busy.clear();

            for port in &target_ports {
                // free once nobody is left listening on it, whoever that was
                match PortInfo::holders_of(*port) {
                    Ok(holders) if holders.is_empty() => freed.push(*port),
                    _ => {
                        still_busy.push(*port);
                        all_free = false;
                    }
                }
            }

            let settled = all_free && was_free;
            was_free = all_free;
            if settled || std::time::Instant::now() >= poll_deadline {
                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(250));
        }

        // Report results
        if self.json {
            let results: Vec<FreeResult> = freed
                .iter()
                .map(|p| FreeResult {
                    port: *p,
                    freed: true,
                })
                .chain(still_busy.iter().map(|p| FreeResult {
                    port: *p,
                    freed: false,
                }))
                .collect();

            let failed = failed_holders(&processes, &signal_failures, Process::is_running);
            printer.print_json(&FreeOutput {
                action: "free",
                success: still_busy.is_empty(),
                results,
                not_found: &not_found,
                processes: &processes
                    .iter()
                    .map(|p| ReceiptProcess::of(p, &failed))
                    .collect::<Vec<_>>(),
                count: processes.len(),
            });
        } else {
            for port in &freed {
                out_println!(
                    "{} Freed port {}",
                    "✓".green().bold(),
                    port.to_string().cyan().bold()
                );
            }
            for port in &still_busy {
                out_println!(
                    "{} Port {} still in use (may be in TIME_WAIT)",
                    "✗".red().bold(),
                    port.to_string().cyan()
                );
            }
        }

        if still_busy.is_empty() {
            Ok(())
        } else {
            Err(ProcError::reported(
                ExitCode::GeneralError,
                format!(
                    "{} port{} still in use",
                    still_busy.len(),
                    if still_busy.len() == 1 { "" } else { "s" }
                ),
            ))
        }
    }
}

#[derive(Serialize)]
struct FreeOutput<'a> {
    action: &'static str,
    success: bool,
    results: Vec<FreeResult>,
    /// The ports nobody was listening on, as typed
    not_found: &'a [String],
    /// The holders it stopped, in the preview's order, each with its `outcome`
    processes: &'a [ReceiptProcess<'a>],
    /// How many of them
    count: usize,
}

#[derive(Serialize)]
struct FreeResult {
    port: u16,
    freed: bool,
}

/// Ask each process to end (SIGTERM; on Windows a refused request for a process
/// that is still running is not a failure), give them up to `grace` to do it, then
/// force kill whatever is still running. A signal that fails is a warning, not
/// an error: the port check afterwards says whether it mattered. Returns the
/// PIDs a signal failed for, with the error text (a PID appears twice when both
/// signals failed). The signals are parameters so a test can make them fail;
/// the command passes `Process::terminate` and `Process::kill`.
fn stop_all(
    processes: &[Process],
    grace: std::time::Duration,
    printer: &Printer,
    terminate: impl Fn(&Process) -> Result<()>,
    kill: impl Fn(&Process) -> Result<()>,
) -> Vec<(u32, String)> {
    let mut failed = Vec::new();
    for proc in processes {
        match terminate(proc) {
            // Windows can't ask a console program to close (taskkill without
            // /F needs a window); the force kill below still stops it
            Err(_) if cfg!(not(unix)) && proc.is_running() => {}
            Err(e) => {
                printer.warning(&format!(
                    "Failed to ask {} [PID {}] to stop: {}",
                    proc.name, proc.pid, e
                ));
                failed.push((proc.pid, e.to_string()));
            }
            Ok(()) => {}
        }
    }

    // Wait for graceful exit
    let start = std::time::Instant::now();
    while start.elapsed() < grace {
        if processes.iter().all(|p| !p.is_running()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Force kill any remaining
    for proc in processes {
        if proc.is_running() {
            if let Err(e) = kill(proc) {
                printer.warning(&format!(
                    "Failed to kill {} [PID {}]: {}",
                    proc.name, proc.pid, e
                ));
                failed.push((proc.pid, e.to_string()));
            }
        }
    }
    failed
}

/// The holders the receipt lists as failed, with why: those still `running`
/// once the ports were checked, with the error of the last signal that failed
/// for them, or `Still running` when none did. A holder that is gone was
/// stopped, whatever a signal said on the way.
fn failed_holders(
    processes: &[Process],
    signal_failures: &[(u32, String)],
    running: impl Fn(&Process) -> bool,
) -> Vec<(Process, String)> {
    processes
        .iter()
        .filter(|p| running(p))
        .map(|p| {
            let error = signal_failures
                .iter()
                .rfind(|(pid, _)| *pid == p.pid)
                .map_or("Still running", |(_, error)| error.as_str());
            (p.clone(), error.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ProcError;

    /// This test's own process: it exists and is the process it says it is, so
    /// `stop_all` sees it as running. The signals below are stand-ins that
    /// send nothing.
    fn this_process() -> Process {
        Process::find_by_pid(std::process::id())
            .unwrap()
            .expect("this process exists")
    }

    fn refuse(_: &Process) -> Result<()> {
        Err(ProcError::SignalError("not allowed".to_string()))
    }

    #[test]
    fn signals_that_fail_are_reported_not_fatal() {
        let me = this_process();
        let printer = Printer::from_flags(true, false);
        let failed = stop_all(
            std::slice::from_ref(&me),
            std::time::Duration::ZERO,
            &printer,
            refuse,
            refuse,
        );
        // It still counts as running, so the SIGKILL is tried after the polite
        // request; on Windows that request being refused is expected
        let refusals = if cfg!(unix) { 2 } else { 1 };
        let expected = (me.pid, "Signal failed: not allowed".to_string());
        assert_eq!(failed, vec![expected; refusals]);
    }

    #[test]
    fn a_refusal_for_a_process_that_is_gone_is_a_warning_and_nothing_is_waited_for() {
        let mut child = crate::test_support::idle_child();
        let gone = crate::test_support::gone_record(&mut child);
        let printer = Printer::from_flags(true, false);
        let start = std::time::Instant::now();
        // Not running, so the refusal is not the one Windows gives a console
        // program that is still there: it is reported, on every OS. The kill
        // is never tried, and the grace period ends at once.
        let failed = stop_all(
            std::slice::from_ref(&gone),
            std::time::Duration::from_secs(30),
            &printer,
            refuse,
            // were the kill tried, it would fail too and be listed twice
            refuse,
        );
        assert_eq!(
            failed,
            vec![(gone.pid, "Signal failed: not allowed".to_string())]
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
    }

    #[test]
    fn signals_that_work_report_nothing() {
        let me = this_process();
        let printer = Printer::from_flags(true, false);
        let failed = stop_all(
            std::slice::from_ref(&me),
            std::time::Duration::ZERO,
            &printer,
            |_| Ok(()),
            |_| Ok(()),
        );
        assert!(failed.is_empty());
    }

    #[test]
    fn a_holder_that_is_still_running_is_failed_and_one_that_is_gone_is_not() {
        let record = |pid| Process {
            pid,
            ..this_process()
        };
        let all = [record(1), record(2), record(3), record(4)];
        let signals = [
            (1, "Signal failed: asking".to_string()),
            (1, "Signal failed: killing".to_string()),
            (2, "Signal failed: asking".to_string()),
        ];
        // 1 and 3 are still running; 2 is gone though a signal failed, 4 is gone
        let failed = failed_holders(&all, &signals, |p| p.pid == 1 || p.pid == 3);
        let shown: Vec<(u32, &str)> = failed.iter().map(|(p, e)| (p.pid, e.as_str())).collect();
        assert_eq!(shown, [(1, "Signal failed: killing"), (3, "Still running")]);
    }
}
