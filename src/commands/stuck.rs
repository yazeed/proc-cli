//! `proc stuck` - Find stuck/hung processes
//!
//! A process counts as stuck when it has been running longer than --timeout and,
//! in two readings a second apart, is using more than 50% CPU or is blocked in
//! uninterruptible sleep. A long build at full CPU looks the same as a loop
//! that never ends, so check the list before --kill.
//!
//! Examples:
//!   proc stuck              # Running over 5 minutes, and busy or blocked for 2 seconds
//!   proc stuck --timeout 60 # The same, for processes running over 1 minute
//!   proc stuck --kill       # Find and kill stuck processes
//!   proc stuck --by node    # Only those named node (--cmd: node anywhere in the command line)

use crate::core::{apply_filters, apply_to_each, NameMatch, Process};
use crate::error::Result;
use crate::ui::{must_confirm, plural, prompt, Printer};
use clap::Args;
use std::time::Duration;

/// Find stuck/hung processes
#[derive(Args, Debug)]
pub struct StuckCommand {
    /// Only processes running longer than this many whole seconds (stuck: over 50% CPU or blocked, for 2 seconds)
    #[arg(long, short = 't', default_value = "300", value_parser = crate::core::values::whole_seconds, allow_negative_numbers = true)]
    pub timeout: u64,

    /// Kill found stuck processes
    #[arg(long, short = 'k')]
    pub kill: bool,

    /// Show what would be killed without actually killing
    #[arg(long)]
    pub dry_run: bool,

    /// Skip confirmation when killing
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Output as JSON
    #[arg(long, short = 'j')]
    pub json: bool,

    /// Add PPID, STATUS and DIR to the table, and cut nothing
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// Print the directory and the command line whole instead of fitting the terminal width
    #[arg(long, short = 'w')]
    pub wide: bool,

    /// Filter by directory; takes the next word as the directory unless it starts with '-'
    /// (bare --in is the current directory, --in=DIR is explicit)
    #[arg(long = "in", short = 'i', num_args = 0..=1, default_missing_value = ".")]
    pub in_dir: Option<String>,

    /// Filter by process name
    #[arg(long = "by", short = 'b')]
    pub by_name: Option<String>,

    /// Match --by as a substring of the process name or full command line (like pkill -f)
    #[arg(long)]
    pub cmd: bool,
}

impl StuckCommand {
    /// Executes the stuck command, finding long-running processes that stay busy (over 50% CPU)
    /// or are blocked in uninterruptible sleep.
    pub fn execute(&self) -> Result<()> {
        let printer = Printer::from_flags(self.json, self.verbose)
            .wide(self.wide)
            .scoped_to_dir(self.in_dir.is_some());

        let timeout = Duration::from_secs(self.timeout);
        let mut processes = Process::find_stuck(timeout)?;

        // Apply --in and --by filters
        apply_filters(
            &mut processes,
            &self.in_dir,
            &self.by_name,
            NameMatch::from_flag(self.cmd),
        )?;

        if processes.is_empty() {
            printer.print_empty_result(
                "stuck",
                &format!("No stuck processes found (threshold: {}s)", self.timeout),
            );
            return Ok(());
        }

        printer.warning(&format!(
            "Found {} potentially stuck process{}",
            processes.len(),
            plural(processes.len())
        ));

        if !self.kill {
            printer.print_processes_under_headline("stuck", &processes);
            return Ok(());
        }

        // Dry run: show what would be killed
        if self.dry_run {
            printer.print_dry_run_with(
                "stuck",
                &processes,
                &format!(
                    "Dry run: would kill {} stuck process{}",
                    processes.len(),
                    plural(processes.len())
                ),
            );
            return Ok(());
        }

        // Same rule everywhere: --yes, or ask; --json never skips it
        let asking = must_confirm("stuck --kill", self.yes, self.json, &processes, None);

        // Kill: JSON prints a single action result; humans see the list first.
        // When they are asked, the question shows it, beside the question.
        if !self.json && !matches!(asking, Ok(true)) {
            printer.print_processes_to_approve(&processes);
        }

        if asking? {
            let question = format!(
                "Kill {} stuck process{}?",
                processes.len(),
                plural(processes.len())
            );
            let list = || printer.print_processes_to_approve(&processes);
            let confirmed = prompt::ask("stuck --kill", &processes, None, &question, list)?;

            if !confirmed {
                printer.warning("Cancelled");
                return Ok(());
            }
        }

        // Use kill_and_wait to ensure stuck processes are actually terminated
        let (killed, failed) = apply_to_each(&processes, Process::kill_and_wait);

        printer
            .with_processes(&processes)
            .print_action_result_as("stuck", "kill", &killed, &failed)
    }
}
