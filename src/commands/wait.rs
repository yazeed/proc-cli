//! `proc wait` - Wait for process(es) to exit
//!
//! Examples:
//!   proc wait node                   # Wait until the processes named node exit
//!   proc wait node --cmd             # ... or with node anywhere in their command line
//!   proc wait 1234                   # Wait for PID 1234 to exit
//!   proc wait :3000                  # Wait for whatever is on port 3000 to exit
//!   proc wait node python            # Wait for all to exit (separate arguments)
//!   proc wait node --timeout 3600    # Timeout after 1 hour
//!   proc wait node --interval 10     # Status line every 10 seconds
//!
//! Commas still work (`proc wait node,python`) but are deprecated.

use crate::core::process::ProcessDetails;
use crate::core::{
    flatten_targets, guard_targets, nothing_resolved, resolve_targets_excluding_self, Guards,
    NameMatch, Process, ProcessStatus,
};
use crate::error::{ExitCode, ProcError, Result};
use crate::out_println;
use crate::ui::{format_duration, plural, Printer};
use clap::Args;
use colored::*;
use serde::Serialize;

/// Wait for process(es) to exit
#[derive(Args, Debug)]
pub struct WaitCommand {
    /// Target(s): process name, PID, or :port (separate arguments for several)
    #[arg(required = true, num_args = 1.., value_parser = crate::core::validate_target_arg)]
    target: Vec<String>,

    /// Whole seconds between status lines (exits are noticed within a quarter second)
    #[arg(long, short = 'n', default_value = "5", value_parser = crate::core::values::whole_seconds, allow_negative_numbers = true)]
    interval: u64,

    /// Timeout in whole seconds (0 = no timeout)
    #[arg(long, short = 't', default_value = "0", value_parser = crate::core::values::whole_seconds, allow_negative_numbers = true)]
    timeout: u64,

    /// Output as JSON
    #[arg(long, short = 'j')]
    json: bool,

    /// List what it waits for in the process table, with PPID, STATUS and DIR
    #[arg(long, short = 'v')]
    verbose: bool,

    /// Filter by directory; takes the next word as the directory unless it starts with '-'
    /// (bare --in is the current directory, --in=DIR is explicit)
    #[arg(long = "in", short = 'i', num_args = 0..=1, default_missing_value = ".")]
    pub in_dir: Option<String>,

    /// Filter by process name
    #[arg(long = "by", short = 'b')]
    pub by_name: Option<String>,

    /// Wait only if the process still has this start time (Unix seconds, `start_time`
    /// in its JSON): a PID the OS gave to a new process is ruled out. One PID target
    #[arg(long = "start-time", value_name = "SECONDS", value_parser = crate::core::values::whole_seconds, allow_negative_numbers = true)]
    pub start_time: Option<u64>,

    /// Match name targets and --by as a substring of the process name or full command line (like pkill -f)
    #[arg(long)]
    pub cmd: bool,

    /// Don't print the periodic still-running lines; the start, each exit and the result still print
    #[arg(long, short = 'q')]
    quiet: bool,
}

impl WaitCommand {
    /// Executes the wait command, blocking until all matched processes exit.
    pub fn execute(&self) -> Result<()> {
        let printer = Printer::from_flags(self.json, self.verbose);

        // Clamp interval to minimum 1 second
        let interval = self.interval.max(1);

        // Parse and resolve targets. proc never waits on itself, nor on a
        // process it runs under, which can't exit while proc runs: those are
        // left out, and named
        let targets = flatten_targets(&self.target);
        let mode = NameMatch::from_flag(self.cmd);
        let resolved = resolve_targets_excluding_self(&targets, mode);

        if let Some(warning) = resolved.warning() {
            printer.warning(&warning);
        }
        let left_out = resolved.left_out_ancestors();
        let not_found = resolved.not_found;
        let mut processes = resolved.processes;

        // Apply --in and --by: guards that rule out everything found say so
        let guards = Guards::new(&self.in_dir, &self.by_name, self.start_time);
        guard_targets(&mut processes, &self.target, &guards, mode)?;

        if processes.is_empty() {
            return Err(nothing_resolved(
                &self.target,
                &resolved.unseen,
                &resolved.left_out,
            ));
        }

        // Real CPU readings for the verbose list, the only view that shows CPU;
        // measured before the clock starts, so it doesn't count against --timeout
        if self.verbose && !self.json {
            Process::measure_cpu(&mut processes);
        }

        let initial_count = processes.len();
        let start = std::time::Instant::now();

        // Print initial status
        if !self.json {
            out_println!(
                "{} Waiting for {} process{} to exit...",
                "~".cyan().bold(),
                initial_count.to_string().cyan().bold(),
                plural(initial_count)
            );
            // -v lists them in the table every list in proc prints
            if self.verbose {
                out_println!();
                printer.print_table(&processes);
            }
        }

        // Track processes: (process, exited_after_seconds)
        let mut tracking: Vec<(Process, Option<u64>)> =
            processes.into_iter().map(|p| (p, None)).collect();

        // Exits are checked every quarter second; --interval only sets how often
        // the status line is printed. The timeout is checked after the exits, so a
        // process that exited at the last moment is never reported as running.
        let tick = std::time::Duration::from_millis(250);
        let status_every = std::time::Duration::from_secs(interval);
        let deadline =
            (self.timeout > 0).then(|| start + std::time::Duration::from_secs(self.timeout));
        let mut next_status = start + status_every;

        loop {
            let now = std::time::Instant::now();
            let nap = match deadline {
                Some(d) => tick.min(d.saturating_duration_since(now)),
                None => tick,
            };
            std::thread::sleep(nap);

            let elapsed = start.elapsed().as_secs();

            // Check which processes have exited
            for (proc, exited_at) in tracking.iter_mut() {
                if exited_at.is_none() && !proc.is_running() {
                    *exited_at = Some(elapsed);
                    if !self.json {
                        out_println!(
                            "{} {} [PID {}] exited after {}",
                            "✓".green().bold(),
                            proc.name.white(),
                            proc.pid.to_string().cyan(),
                            format_duration(elapsed)
                        );
                    }
                }
            }

            let still_running_count = tracking.iter().filter(|(_, t)| t.is_none()).count();

            if still_running_count == 0 {
                break;
            }

            // Check timeout
            if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
                // Mark still-running processes
                let exited: Vec<ExitedProcess> = tracking
                    .iter()
                    .filter(|(_, t)| t.is_some())
                    .map(|(p, t)| ExitedProcess {
                        pid: p.pid,
                        name: p.name.clone(),
                        exited_after_seconds: t.unwrap(),
                        last: LastReading::of(p),
                        exited_after_secs: t.unwrap(),
                    })
                    .collect();
                let still_running: Vec<RunningProcess> = tracking
                    .iter()
                    .filter(|(_, t)| t.is_none())
                    .map(|(p, _)| RunningProcess {
                        pid: p.pid,
                        name: p.name.clone(),
                        last: LastReading::of(p),
                    })
                    .collect();

                let names: Vec<String> = still_running
                    .iter()
                    .map(|p| format!("{} [{}]", p.name, p.pid))
                    .collect();
                let message = format!(
                    "after {} — {} still running: {}",
                    format_duration(elapsed),
                    still_running.len(),
                    names.join(", ")
                );

                if self.json {
                    printer.print_json(&WaitOutput {
                        action: "wait",
                        success: false,
                        timed_out: true,
                        elapsed_seconds: elapsed,
                        elapsed_human: format_duration(elapsed),
                        target: self.target.join(" "),
                        initial_count,
                        not_found,
                        left_out,
                        exited,
                        still_running,
                        count: initial_count,
                        elapsed_secs: elapsed,
                        targets: self.target.clone(),
                    });
                    // Same exit code as the human-mode Timeout error
                    return Err(ProcError::reported(
                        ExitCode::GeneralError,
                        format!("Operation timed out: {}", message),
                    ));
                }

                return Err(ProcError::Timeout(message));
            }

            // Print periodic status every --interval (unless quiet or json)
            if std::time::Instant::now() < next_status {
                continue;
            }
            next_status += status_every;
            if !self.quiet && !self.json {
                let names: Vec<String> = tracking
                    .iter()
                    .filter(|(_, t)| t.is_none())
                    .map(|(p, _)| format!("{} [{}]", p.name, p.pid))
                    .collect();
                let exited_count = initial_count - still_running_count;
                let exited_note = if exited_count > 0 {
                    format!(" ({} exited)", exited_count)
                } else {
                    String::new()
                };
                out_println!(
                    "{} {} elapsed — {} still running: {}{}",
                    "~".cyan(),
                    format_duration(elapsed),
                    still_running_count,
                    names.join(", "),
                    exited_note.bright_black()
                );
            }
        }

        let elapsed = start.elapsed().as_secs();

        // Final output
        if self.json {
            let exited: Vec<ExitedProcess> = tracking
                .iter()
                .map(|(p, t)| ExitedProcess {
                    pid: p.pid,
                    name: p.name.clone(),
                    exited_after_seconds: t.unwrap_or(elapsed),
                    last: LastReading::of(p),
                    exited_after_secs: t.unwrap_or(elapsed),
                })
                .collect();
            printer.print_json(&WaitOutput {
                action: "wait",
                success: true,
                timed_out: false,
                elapsed_seconds: elapsed,
                elapsed_human: format_duration(elapsed),
                target: self.target.join(" "),
                initial_count,
                not_found,
                left_out,
                exited,
                still_running: vec![],
                count: initial_count,
                elapsed_secs: elapsed,
                targets: self.target.clone(),
            });
        } else {
            out_println!(
                "{} All {} process{} exited after {}",
                "✓".green().bold(),
                initial_count.to_string().cyan().bold(),
                plural(initial_count),
                format_duration(elapsed)
            );
        }

        Ok(())
    }
}

#[derive(Serialize)]
struct WaitOutput {
    action: &'static str,
    success: bool,
    timed_out: bool,
    elapsed_seconds: u64,
    elapsed_human: String,
    target: String,
    initial_count: usize,
    /// The targets that matched nothing
    not_found: Vec<String>,
    /// The processes proc runs under that a target matched and that were left
    /// out; absent when there are none
    #[serde(skip_serializing_if = "Vec::is_empty")]
    left_out: Vec<crate::core::Ancestor>,
    exited: Vec<ExitedProcess>,
    still_running: Vec<RunningProcess>,
    /// The processes waited for, as every other command's `count` (`initial_count`
    /// is the old name for it, deprecated, removed in 3.0)
    count: usize,
    /// `elapsed_seconds` under the name every duration has (`_secs`)
    elapsed_secs: u64,
    /// The targets as typed, one per entry (`target` joins them with spaces)
    targets: Vec<String>,
}

#[derive(Serialize, Clone)]
struct ExitedProcess {
    pid: u32,
    name: String,
    exited_after_seconds: u64,
    #[serde(flatten)]
    last: LastReading,
    /// `exited_after_seconds` under the name every duration has (`_secs`)
    exited_after_secs: u64,
}

#[derive(Serialize, Clone)]
struct RunningProcess {
    pid: u32,
    name: String,
    #[serde(flatten)]
    last: LastReading,
}

/// The fields of the last reading proc had of a process, before it exited
/// (for a process still running, the reading taken when the wait began). A
/// value that reading did not have is an absent key.
#[derive(Serialize, Clone)]
struct LastReading {
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    status: ProcessStatus,
    cpu_percent: f32,
    memory_mb: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    uptime_secs: Option<u64>,
    #[serde(flatten)]
    details: ProcessDetails,
}

impl LastReading {
    fn of(p: &Process) -> Self {
        Self {
            cwd: p.cwd.clone(),
            command: p.command.clone(),
            status: p.status,
            cpu_percent: p.cpu_percent,
            memory_mb: p.memory_mb,
            uptime_secs: p.uptime_secs,
            details: ProcessDetails::of(p),
        }
    }
}
