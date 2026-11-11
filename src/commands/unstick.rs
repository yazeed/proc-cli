//! Unstick command - Attempt to recover stuck processes
//!
//! Tries gentle recovery signals first. SIGINT can end a process that does not
//! handle it, with or without --force; SIGTERM and SIGKILL are sent only with
//! --force.
//!
//! Recovery sequence:
//! 1. SIGCONT (wake if stopped)
//! 2. SIGINT (interrupt, like Ctrl+C)
//!
//! With --force:
//! 3. SIGTERM (polite termination request)
//! 4. SIGKILL (force, last resort)
//!
//! Usage:
//!   proc unstick           # Find and unstick all stuck processes
//!   proc unstick :3000     # Unstick process on port 3000
//!   proc unstick 1234      # Unstick PID 1234
//!   proc unstick node      # Unstick the stuck processes named node
//!   proc unstick node --cmd   # ... or with node anywhere in their command line
//!   proc unstick :3000 1234   # Several targets: separate arguments
//!
//! Commas still work (`proc unstick :3000,1234`) but are deprecated.

use crate::core::process::ProcessDetails;
use crate::core::{
    apply_filters, flatten_targets, guard_targets, nothing_resolved,
    resolve_targets_excluding_self, Guards, NameMatch, Process, ProcessStatus, Resolved,
};
use crate::error::{ExitCode, ProcError, Result};
use crate::ui::{format_duration, must_confirm, plural, prompt, Printer};
use crate::{out_print, out_println};
use clap::Args;
use colored::*;
use serde::Serialize;
use std::time::Duration;

#[cfg(unix)]
use nix::sys::signal::Signal;

/// Attempt to recover stuck processes
#[derive(Args, Debug)]
pub struct UnstickCommand {
    /// Target(s): PID, :port, or name (separate arguments for several; omit to find all stuck)
    #[arg(num_args = 0.., value_parser = crate::core::validate_target_arg)]
    target: Vec<String>,

    /// Without a target: only stuck processes running longer than this many seconds
    #[arg(long, short, default_value = "300")]
    timeout: u64,

    /// Force termination if recovery fails
    #[arg(long, short = 'f')]
    force: bool,

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    yes: bool,

    /// Show what would be done without doing it
    #[arg(long)]
    dry_run: bool,

    /// Show verbose output
    #[arg(long, short = 'v')]
    verbose: bool,

    /// Output as JSON
    #[arg(long, short = 'j')]
    json: bool,

    /// Filter by directory; takes the next word as the directory unless it starts with '-'
    /// (bare --in is the current directory, --in=DIR is explicit)
    #[arg(long = "in", short = 'i', num_args = 0..=1, default_missing_value = ".")]
    pub in_dir: Option<String>,

    /// Filter by process name
    #[arg(long = "by", short = 'b')]
    pub by_name: Option<String>,

    /// Act only if the process still has this start time (Unix seconds, `start_time`
    /// in its JSON): a PID the OS gave to a new process is ruled out. One PID target
    #[arg(long = "start-time", value_name = "SECONDS", requires = "target")]
    pub start_time: Option<u64>,

    /// Match name targets and --by as a substring of the process name or full command line (like pkill -f)
    #[arg(long)]
    pub cmd: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Outcome {
    Recovered,  // Process unstuck and still running
    Terminated, // Gone: killed with --force, or ended by the SIGINT step
    StillStuck, // Could not recover, not terminated (no --force)
    NotStuck,   // Process wasn't stuck to begin with
    Failed(String),
}

impl UnstickCommand {
    /// Executes the unstick command, attempting to recover hung processes.
    pub fn execute(&self) -> Result<()> {
        let printer = Printer::from_flags(self.json, self.verbose);

        // Get processes to unstick, the targets that matched nothing, and the
        // processes proc runs under that a target matched and left out
        let (mut stuck, not_found, left_out) = if !self.target.is_empty() {
            // Specific targets
            let resolved = self.resolve_target_processes(&self.target)?;
            if let Some(warning) = resolved.warning() {
                printer.warning(&warning);
            }
            let left_out = resolved.left_out_ancestors();
            (resolved.processes, resolved.not_found, left_out)
        } else {
            // Auto-discover stuck processes
            let timeout = Duration::from_secs(self.timeout);
            (Process::find_stuck(timeout)?, Vec::new(), Vec::new())
        };

        // Apply --in and --by: with targets they are guards, which say so when
        // they rule out everything found; on the processes found stuck they
        // only narrow the list
        let mode = NameMatch::from_flag(self.cmd);
        if self.target.is_empty() {
            apply_filters(&mut stuck, &self.in_dir, &self.by_name, mode)?;
        } else {
            let guards = Guards::new(&self.in_dir, &self.by_name, self.start_time);
            guard_targets(&mut stuck, &self.target, &guards, mode)?;
        }

        // Only the search can come up empty: targets that match nothing, or
        // that the guards rule out, are errors above
        if stuck.is_empty() {
            if self.json {
                printer.print_json(&UnstickOutput {
                    action: "unstick",
                    success: true,
                    dry_run: self.dry_run,
                    force: self.force,
                    found: 0,
                    recovered: 0,
                    not_stuck: 0,
                    still_stuck: 0,
                    terminated: 0,
                    failed: 0,
                    not_found: &not_found,
                    left_out: &left_out,
                    processes: Vec::new(),
                    count: 0,
                    recovered_count: 0,
                    not_stuck_count: 0,
                    still_stuck_count: 0,
                    terminated_count: 0,
                    failed_count: 0,
                });
            } else {
                printer.success("No stuck processes found");
            }
            return Ok(());
        }

        // Real CPU readings whenever the list is shown or returned (dry run,
        // the prompt, or the error for a missing `--yes`); `--yes` acts at once
        if self.dry_run || !self.yes {
            Process::measure_cpu(&mut stuck);
        }

        // Show stuck processes. When a person is about to be asked, the
        // question shows them, beside the question
        let asked_below = !self.dry_run
            && matches!(
                must_confirm("unstick", self.yes, self.json, &stuck, Some(&not_found)),
                Ok(true)
            );
        if !self.json && !asked_below {
            self.show_processes(&stuck);
        }

        // Dry run
        if self.dry_run {
            if self.json {
                printer.print_json(&UnstickOutput {
                    action: "unstick",
                    success: true,
                    dry_run: true,
                    force: self.force,
                    found: stuck.len(),
                    recovered: 0,
                    not_stuck: 0,
                    still_stuck: 0,
                    terminated: 0,
                    failed: 0,
                    not_found: &not_found,
                    left_out: &left_out,
                    processes: stuck
                        .iter()
                        .map(|p| ProcessOutcome::of(p, "would_attempt".to_string()))
                        .collect(),
                    count: stuck.len(),
                    recovered_count: 0,
                    not_stuck_count: 0,
                    still_stuck_count: 0,
                    terminated_count: 0,
                    failed_count: 0,
                });
            } else {
                out_println!(
                    "\n{} Dry run: Would attempt to unstick {} process{}",
                    "ℹ".blue().bold(),
                    stuck.len().to_string().cyan().bold(),
                    plural(stuck.len())
                );
                if self.force {
                    out_println!("  With --force: will terminate if recovery fails");
                } else {
                    out_println!("  Without --force: will only attempt recovery");
                }
                out_println!();
            }
            return Ok(());
        }

        // Confirm. Same rule everywhere: --yes, or ask; --json never skips it
        if must_confirm("unstick", self.yes, self.json, &stuck, Some(&not_found))? {
            let question = format!("Unstick {} process{}?", stuck.len(), plural(stuck.len()));

            let show = || {
                self.show_processes(&stuck);
                if self.force {
                    out_println!(
                        "\n{} With --force: processes will be terminated if recovery fails.\n",
                        "!".yellow().bold()
                    );
                } else {
                    out_println!(
                        "\n{} Will attempt recovery only. Use --force to terminate if needed.\n",
                        "ℹ".blue().bold()
                    );
                }
            };
            if !prompt::ask("unstick", &stuck, Some(&not_found), &question, show)? {
                printer.warning("Aborted");
                return Ok(());
            }
        }

        // Attempt to unstick each process
        let mut outcomes: Vec<(Process, Outcome)> = Vec::new();

        for proc in &stuck {
            if !self.json {
                out_print!(
                    "  {} {} [PID {}]... ",
                    "→".bright_black(),
                    proc.name.white(),
                    proc.pid.to_string().cyan()
                );
            }

            let outcome = self.attempt_unstick(proc);

            if !self.json {
                out_println!("{}", outcome_text(&outcome));
            }

            outcomes.push((proc.clone(), outcome));
        }

        // Count outcomes
        let recovered = outcomes
            .iter()
            .filter(|(_, o)| *o == Outcome::Recovered)
            .count();
        let terminated = outcomes
            .iter()
            .filter(|(_, o)| *o == Outcome::Terminated)
            .count();
        let still_stuck = outcomes
            .iter()
            .filter(|(_, o)| *o == Outcome::StillStuck)
            .count();
        let not_stuck = outcomes
            .iter()
            .filter(|(_, o)| *o == Outcome::NotStuck)
            .count();
        let failed = outcomes
            .iter()
            .filter(|(_, o)| matches!(o, Outcome::Failed(_)))
            .count();

        // Output results
        if self.json {
            printer.print_json(&UnstickOutput {
                action: "unstick",
                success: failed == 0 && still_stuck == 0,
                dry_run: false,
                force: self.force,
                found: stuck.len(),
                recovered,
                not_stuck,
                still_stuck,
                terminated,
                failed,
                not_found: &not_found,
                left_out: &left_out,
                processes: outcomes
                    .iter()
                    .map(|(p, o)| ProcessOutcome::of(p, outcome_label(o)))
                    .collect(),
                count: stuck.len(),
                recovered_count: recovered,
                not_stuck_count: not_stuck,
                still_stuck_count: still_stuck,
                terminated_count: terminated,
                failed_count: failed,
            });
        } else {
            out_println!();
            for line in summary_lines(recovered, not_stuck, terminated, still_stuck, failed) {
                out_println!("{line}");
            }
        }

        if failed > 0 || still_stuck > 0 {
            return Err(ProcError::reported(
                ExitCode::GeneralError,
                format!(
                    "{} process{} still stuck, {} failed",
                    still_stuck,
                    plural(still_stuck),
                    failed
                ),
            ));
        }

        Ok(())
    }

    /// Resolve target to processes, excluding self and the processes proc
    /// runs under (see [`resolve_targets_excluding_self`])
    fn resolve_target_processes(&self, typed: &[String]) -> Result<Resolved> {
        let targets = flatten_targets(typed);
        let resolved = resolve_targets_excluding_self(&targets, NameMatch::from_flag(self.cmd));
        if resolved.processes.is_empty() {
            Err(nothing_resolved(
                typed.join(" "),
                &resolved.unseen,
                &resolved.left_out,
            ))
        } else {
            Ok(resolved)
        }
    }

    /// Check if a process looks stuck, the way `proc stuck` measures it (two
    /// readings a second apart). The `cpu_percent` of a fresh lookup is read over
    /// a few milliseconds and is ~0 for any process, so it can't be used here.
    fn is_stuck(&self, proc: &Process) -> bool {
        proc.looks_stuck_now()
    }

    /// Attempt to unstick a process: gentle recovery first, termination only with --force
    fn attempt_unstick(&self, proc: &Process) -> Outcome {
        // For targeted processes, check if actually stuck
        if !self.target.is_empty() && !self.is_stuck(proc) {
            return Outcome::NotStuck;
        }

        #[cfg(unix)]
        return recover(proc, self.force);
        // On non-Unix, we can only terminate
        #[cfg(not(unix))]
        return recover_by_termination(proc, self.force);
    }

    fn show_processes(&self, processes: &[Process]) {
        let label = if !self.target.is_empty() {
            "Target"
        } else {
            "Found stuck"
        };

        out_println!(
            "\n{} {} {} process{}:\n",
            "!".yellow().bold(),
            label,
            processes.len().to_string().cyan().bold(),
            plural(processes.len())
        );

        for proc in processes {
            let uptime = proc
                .start_time
                .map(|st| {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs().saturating_sub(st))
                        .unwrap_or(0);
                    format_duration(now)
                })
                .unwrap_or_else(|| "unknown".to_string());

            out_println!(
                "  {} {} [PID {}] - {:.1}% CPU, running for {}",
                "→".bright_black(),
                proc.name.white().bold(),
                proc.pid.to_string().cyan(),
                proc.cpu_percent,
                uptime.yellow()
            );
        }
    }
}

/// What the recovery sequences do to one process. A trait so a test can script
/// every answer: the failures and the races can't be produced on demand.
trait Steps {
    fn is_running(&self) -> bool;
    fn pause(&self, secs: u64);
    fn terminate(&self) -> Result<()>;
    fn kill(&self) -> Result<()>;
    /// SIGCONT, wake it if stopped (the result doesn't matter)
    #[cfg(unix)]
    fn wake(&self);
    /// SIGINT, like Ctrl+C
    #[cfg(unix)]
    fn interrupt(&self) -> Result<()>;
    /// Whether it uses under a tenth of a core over a second. A fresh lookup's
    /// reading is ~0 for anything, so this measures over a real window.
    #[cfg(unix)]
    fn calmed_down(&self) -> bool;
}

impl Steps for Process {
    fn is_running(&self) -> bool {
        Process::is_running(self)
    }

    fn pause(&self, secs: u64) {
        std::thread::sleep(Duration::from_secs(secs));
    }

    fn terminate(&self) -> Result<()> {
        Process::terminate(self)
    }

    fn kill(&self) -> Result<()> {
        Process::kill(self)
    }

    // Signals go through Process, which checks the PID wasn't reused meanwhile
    #[cfg(unix)]
    fn wake(&self) {
        let _ = self.send_signal(Signal::SIGCONT);
    }

    #[cfg(unix)]
    fn interrupt(&self) -> Result<()> {
        self.send_signal(Signal::SIGINT)
    }

    #[cfg(unix)]
    fn calmed_down(&self) -> bool {
        self.cpu_over(Duration::from_secs(1))
            .is_some_and(|cpu| cpu < 10.0)
    }
}

/// The outcome of a SIGKILL (or `taskkill /F`): a failed kill of a process
/// that is gone anyway still counts as terminated
fn after_kill(killed: Result<()>, still_running: bool) -> Outcome {
    match killed {
        Ok(()) => Outcome::Terminated,
        Err(_) if !still_running => Outcome::Terminated,
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

/// Unix recovery: SIGCONT, then SIGINT; with `force`, SIGTERM, then SIGKILL
#[cfg(unix)]
fn recover(steps: &impl Steps, force: bool) -> Outcome {
    // Step 1: SIGCONT (wake if stopped); calmed_down measures over a second
    steps.wake();
    if steps.calmed_down() {
        return Outcome::Recovered;
    }

    // Step 2: SIGINT (interrupt)
    if steps.interrupt().is_err() && !steps.is_running() {
        return Outcome::Terminated;
    }
    steps.pause(3);

    if !steps.is_running() {
        return Outcome::Terminated;
    }
    if steps.calmed_down() {
        return Outcome::Recovered;
    }

    // Without --force, stop here
    if !force {
        return Outcome::StillStuck;
    }

    // Step 3: SIGTERM (polite termination) - only with --force
    if steps.terminate().is_err() && !steps.is_running() {
        return Outcome::Terminated;
    }
    steps.pause(5);

    if !steps.is_running() {
        return Outcome::Terminated;
    }

    // Step 4: SIGKILL (force, last resort) - only with --force
    after_kill(steps.kill(), steps.is_running())
}

/// Windows recovery: there is nothing gentle to try, so only `force` acts
/// (compiled on Unix for the tests only)
#[cfg(any(not(unix), test))]
fn recover_by_termination(steps: &impl Steps, force: bool) -> Outcome {
    if !force {
        return Outcome::StillStuck;
    }

    if steps.terminate().is_ok() {
        steps.pause(3);
        if !steps.is_running() {
            return Outcome::Terminated;
        }
    }

    after_kill(steps.kill(), steps.is_running())
}

/// What one process's outcome looks like in the human output
fn outcome_text(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Recovered => "recovered".green().to_string(),
        Outcome::Terminated => "terminated".yellow().to_string(),
        Outcome::StillStuck => "still stuck".red().to_string(),
        Outcome::NotStuck => "not stuck".blue().to_string(),
        Outcome::Failed(e) => format!("{}: {}", "failed".red(), e),
    }
}

/// What one process's outcome is called in the JSON output
fn outcome_label(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Recovered => "recovered".to_string(),
        Outcome::Terminated => "terminated".to_string(),
        Outcome::StillStuck => "still_stuck".to_string(),
        Outcome::NotStuck => "not_stuck".to_string(),
        Outcome::Failed(e) => format!("failed: {}", e),
    }
}

/// The summary after the outcomes: one line for each kind that occurred
fn summary_lines(
    recovered: usize,
    not_stuck: usize,
    terminated: usize,
    still_stuck: usize,
    failed: usize,
) -> Vec<String> {
    let mut lines = Vec::new();
    if recovered > 0 {
        lines.push(format!(
            "{} {} process{} recovered",
            "✓".green().bold(),
            recovered.to_string().cyan().bold(),
            plural(recovered)
        ));
    }
    if not_stuck > 0 {
        lines.push(format!(
            "{} {} process{} not stuck",
            "ℹ".blue().bold(),
            not_stuck.to_string().cyan().bold(),
            if not_stuck == 1 { " was" } else { "es were" }
        ));
    }
    if terminated > 0 {
        lines.push(format!(
            "{} {} process{} terminated",
            "!".yellow().bold(),
            terminated.to_string().cyan().bold(),
            plural(terminated)
        ));
    }
    if still_stuck > 0 {
        lines.push(format!(
            "{} {} process{} still stuck (use --force to terminate)",
            "✗".red().bold(),
            still_stuck.to_string().cyan().bold(),
            plural(still_stuck)
        ));
    }
    if failed > 0 {
        lines.push(format!(
            "{} {} process{} failed",
            "✗".red().bold(),
            failed.to_string().cyan().bold(),
            plural(failed)
        ));
    }
    lines
}

#[derive(Serialize)]
struct UnstickOutput<'a> {
    action: &'static str,
    success: bool,
    dry_run: bool,
    force: bool,
    found: usize,
    recovered: usize,
    not_stuck: usize,
    still_stuck: usize,
    terminated: usize,
    failed: usize,
    /// The targets that matched nothing
    not_found: &'a [String],
    /// The processes proc runs under that a target matched and that were left
    /// out; absent when there are none
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    left_out: &'a [crate::core::Ancestor],
    processes: Vec<ProcessOutcome>,
    /// The counts above under the names every other command's counts have:
    /// `count` for the processes found, `<outcome>_count` for the rest. The
    /// bare numbers (`found`, `recovered`, `not_stuck`, `still_stuck`,
    /// `terminated`, `failed`) are deprecated, removed in 3.0
    count: usize,
    recovered_count: usize,
    not_stuck_count: usize,
    still_stuck_count: usize,
    terminated_count: usize,
    failed_count: usize,
}

#[derive(Serialize)]
struct ProcessOutcome {
    pid: u32,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    uptime_secs: Option<u64>,
    outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    status: ProcessStatus,
    cpu_percent: f32,
    memory_mb: f64,
    #[serde(flatten)]
    details: ProcessDetails,
}

impl ProcessOutcome {
    fn of(p: &Process, outcome: String) -> Self {
        Self {
            pid: p.pid,
            name: p.name.clone(),
            uptime_secs: p.uptime_secs,
            outcome,
            cwd: p.cwd.clone(),
            command: p.command.clone(),
            status: p.status,
            cpu_percent: p.cpu_percent,
            memory_mb: p.memory_mb,
            details: ProcessDetails::of(p),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    /// A process that answers from a script, and remembers what was done to it
    #[derive(Default)]
    struct Fake {
        /// Answers to `is_running`, in order; the last one repeats
        running: RefCell<VecDeque<bool>>,
        /// Answers to `calmed_down`, in order; `false` once they run out
        #[cfg(unix)]
        calm: RefCell<VecDeque<bool>>,
        #[cfg(unix)]
        interrupt_fails: bool,
        terminate_fails: bool,
        kill_fails: bool,
        calls: RefCell<Vec<String>>,
    }

    impl Fake {
        fn running(answers: &[bool]) -> Self {
            Fake {
                running: RefCell::new(answers.iter().copied().collect()),
                ..Fake::default()
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }

        fn note(&self, what: &str) {
            self.calls.borrow_mut().push(what.to_string());
        }

        fn result(&self, fails: bool) -> Result<()> {
            if fails {
                Err(ProcError::SignalError("denied".to_string()))
            } else {
                Ok(())
            }
        }
    }

    impl Steps for Fake {
        fn is_running(&self) -> bool {
            let mut answers = self.running.borrow_mut();
            if answers.len() > 1 {
                answers.pop_front().unwrap()
            } else {
                answers.front().copied().unwrap_or(true)
            }
        }

        fn pause(&self, secs: u64) {
            self.note(&format!("pause {secs}"));
        }

        fn terminate(&self) -> Result<()> {
            self.note("terminate");
            self.result(self.terminate_fails)
        }

        fn kill(&self) -> Result<()> {
            self.note("kill");
            self.result(self.kill_fails)
        }

        #[cfg(unix)]
        fn wake(&self) {
            self.note("wake");
        }

        #[cfg(unix)]
        fn interrupt(&self) -> Result<()> {
            self.note("interrupt");
            self.result(self.interrupt_fails)
        }

        #[cfg(unix)]
        fn calmed_down(&self) -> bool {
            self.note("calmed_down");
            self.calm.borrow_mut().pop_front().unwrap_or(false)
        }
    }

    /// Text without colour codes, whether or not colour is on
    fn plain(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn colour_codes_are_stripped_from_text() {
        assert_eq!(plain("\x1b[1;31mred\x1b[0m and plain"), "red and plain");
    }

    #[cfg(unix)]
    #[test]
    fn a_process_that_calms_after_sigcont_is_recovered_without_a_signal() {
        let fake = Fake {
            calm: RefCell::new([true].into()),
            ..Fake::running(&[true])
        };
        assert_eq!(recover(&fake, true), Outcome::Recovered);
        assert_eq!(fake.calls(), ["wake", "calmed_down"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_process_gone_after_a_failed_sigint_is_terminated() {
        let fake = Fake {
            interrupt_fails: true,
            ..Fake::running(&[false])
        };
        assert_eq!(recover(&fake, false), Outcome::Terminated);
        assert_eq!(fake.calls(), ["wake", "calmed_down", "interrupt"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_process_that_exits_after_sigint_is_terminated() {
        let fake = Fake::running(&[false]);
        assert_eq!(recover(&fake, false), Outcome::Terminated);
        assert_eq!(
            fake.calls(),
            ["wake", "calmed_down", "interrupt", "pause 3"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_process_that_calms_after_sigint_is_recovered() {
        let fake = Fake {
            calm: RefCell::new([false, true].into()),
            ..Fake::running(&[true])
        };
        assert_eq!(recover(&fake, false), Outcome::Recovered);
    }

    #[cfg(unix)]
    #[test]
    fn without_force_a_process_that_stays_busy_is_still_stuck() {
        let fake = Fake::running(&[true]);
        assert_eq!(recover(&fake, false), Outcome::StillStuck);
        assert!(!fake.calls().contains(&"terminate".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn force_escalates_from_sigterm_to_sigkill() {
        let fake = Fake::running(&[true]);
        assert_eq!(recover(&fake, true), Outcome::Terminated);
        assert_eq!(&fake.calls()[5..], ["terminate", "pause 5", "kill"]);
    }

    #[cfg(unix)]
    #[test]
    fn force_stops_at_sigterm_when_that_ends_the_process() {
        // running after SIGINT's wait, gone after SIGTERM's
        let fake = Fake::running(&[true, false]);
        assert_eq!(recover(&fake, true), Outcome::Terminated);
        assert!(!fake.calls().contains(&"kill".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn force_with_a_failed_sigterm_on_a_gone_process_is_terminated() {
        let fake = Fake {
            terminate_fails: true,
            ..Fake::running(&[true, false])
        };
        assert_eq!(recover(&fake, true), Outcome::Terminated);
        assert!(!fake.calls().contains(&"pause 5".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn a_kill_that_fails_on_a_process_still_running_is_failed() {
        let fake = Fake {
            kill_fails: true,
            ..Fake::running(&[true])
        };
        assert_eq!(
            recover(&fake, true),
            Outcome::Failed("Signal failed: denied".to_string())
        );
    }

    #[test]
    fn a_kill_that_fails_on_a_process_already_gone_is_terminated() {
        assert_eq!(
            after_kill(Err(ProcError::ProcessNotFound("1".into())), false),
            Outcome::Terminated
        );
        assert_eq!(after_kill(Ok(()), true), Outcome::Terminated);
    }

    #[test]
    fn termination_only_recovery_does_nothing_without_force() {
        let fake = Fake::running(&[true]);
        assert_eq!(recover_by_termination(&fake, false), Outcome::StillStuck);
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn termination_only_recovery_with_force_stops_when_the_request_works() {
        let fake = Fake::running(&[false]);
        assert_eq!(recover_by_termination(&fake, true), Outcome::Terminated);
        assert_eq!(fake.calls(), ["terminate", "pause 3"]);
    }

    #[test]
    fn termination_only_recovery_kills_what_ignores_the_request() {
        let fake = Fake::running(&[true]);
        assert_eq!(recover_by_termination(&fake, true), Outcome::Terminated);
        assert_eq!(fake.calls(), ["terminate", "pause 3", "kill"]);

        let fake = Fake {
            terminate_fails: true,
            ..Fake::running(&[true])
        };
        assert_eq!(recover_by_termination(&fake, true), Outcome::Terminated);
        assert_eq!(fake.calls(), ["terminate", "kill"]);

        let fake = Fake {
            kill_fails: true,
            ..Fake::running(&[true])
        };
        assert_eq!(
            recover_by_termination(&fake, true),
            Outcome::Failed("Signal failed: denied".to_string())
        );
    }

    #[test]
    fn a_process_that_outlasts_the_request_is_killed_and_seen_gone() {
        // still running after the pause, gone once the kill has been sent
        let fake = Fake::running(&[true, false]);
        assert_eq!(recover_by_termination(&fake, true), Outcome::Terminated);
        assert_eq!(fake.calls(), ["terminate", "pause 3", "kill"]);
    }

    #[test]
    fn the_steps_act_on_the_real_process() {
        let mut child = crate::test_support::idle_child();
        let record = crate::test_support::record_of(&child);
        assert!(Steps::is_running(&record));
        // a pause with no time in it: the real wait is the same call
        Steps::pause(&record, 0);
        Steps::kill(&record).unwrap();
        let _ = child.wait();
        // Windows can list a process for a moment after it has exited (the
        // recovery itself does not ask: a kill that worked is `Terminated`)
        let gone = (0..200).any(|_| {
            std::thread::sleep(Duration::from_millis(50));
            !Steps::is_running(&record)
        });
        assert!(gone, "still listed 10 seconds after it exited");
        crate::test_support::end(&mut child);
    }

    #[test]
    fn each_outcome_has_its_words_in_text_and_json() {
        let cases = [
            (Outcome::Recovered, "recovered", "recovered"),
            (Outcome::Terminated, "terminated", "terminated"),
            (Outcome::StillStuck, "still stuck", "still_stuck"),
            (Outcome::NotStuck, "not stuck", "not_stuck"),
            (
                Outcome::Failed("denied".to_string()),
                "failed: denied",
                "failed: denied",
            ),
        ];
        for (outcome, text, label) in cases {
            assert_eq!(plain(&outcome_text(&outcome)), text);
            assert_eq!(outcome_label(&outcome), label);
        }
    }

    #[test]
    fn the_summary_has_a_line_for_each_kind_that_occurred() {
        let all: Vec<String> = summary_lines(2, 1, 3, 1, 2)
            .iter()
            .map(|l| plain(l))
            .collect();
        assert_eq!(
            all,
            [
                "✓ 2 processes recovered",
                "ℹ 1 process was not stuck",
                "! 3 processes terminated",
                "✗ 1 process still stuck (use --force to terminate)",
                "✗ 2 processes failed",
            ]
        );
        let some: Vec<String> = summary_lines(0, 2, 0, 0, 1)
            .iter()
            .map(|l| plain(l))
            .collect();
        assert_eq!(some, ["ℹ 2 processes were not stuck", "✗ 1 process failed"]);
        assert!(summary_lines(0, 0, 0, 0, 0).is_empty());
    }
}
