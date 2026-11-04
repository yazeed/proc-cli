//! `proc orphans` - Find orphaned processes
//!
//! Examples:
//!   proc orphans              # List orphaned processes
//!   proc orphans --in .       # Orphans in current directory
//!   proc orphans --by node    # Only those named node (--cmd: node anywhere in the command line)
//!   proc orphans --kill       # Find and kill orphans
//!   proc orphans --kill --yes # Kill orphans without confirmation

use crate::core::Process;
#[cfg(unix)]
use crate::core::{apply_filters, apply_to_each, NameMatch};
use crate::error::Result;
#[cfg(unix)]
use crate::ui::{must_confirm, plural, prompt, Printer};
use clap::Args;

/// Find orphaned processes (re-parented to PID 1: init/launchd; includes processes launchd started itself). Not available on Windows
#[derive(Args, Debug)]
pub struct OrphansCommand {
    /// Kill found orphaned processes
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

impl OrphansCommand {
    /// Executes the orphans command, finding processes whose parent is PID 1.
    #[cfg(unix)]
    pub fn execute(&self) -> Result<()> {
        let printer = Printer::from_flags(self.json, self.verbose)
            .wide(self.wide)
            .scoped_to_dir(self.in_dir.is_some());

        let mut processes = Process::find_orphans()?;

        // Apply --in and --by filters
        apply_filters(
            &mut processes,
            &self.in_dir,
            &self.by_name,
            NameMatch::from_flag(self.cmd),
        )?;

        // Real CPU readings for every view that shows or returns the list; only
        // `--kill --json --yes` (no dry run) goes straight to killing
        if !(self.kill && self.json && self.yes && !self.dry_run) {
            Process::measure_cpu(&mut processes);
        }

        if processes.is_empty() {
            printer.print_empty_result("orphans", "No orphaned processes found");
            return Ok(());
        }

        printer.warning(&format!(
            "Found {} orphaned process{}",
            processes.len(),
            plural(processes.len())
        ));

        if !self.kill {
            printer.print_processes_under_headline("orphans", &processes);
            return Ok(());
        }

        // Dry run: show what would be killed
        if self.dry_run {
            printer.print_dry_run_with(
                "orphans",
                &processes,
                &format!(
                    "Dry run: would kill {} orphaned process{}",
                    processes.len(),
                    plural(processes.len())
                ),
            );
            return Ok(());
        }

        // Same rule everywhere: --yes, or ask; --json never skips it
        let asking = must_confirm("orphans --kill", self.yes, self.json, &processes, None);

        // Kill: JSON prints a single action result; humans see the list first.
        // When they are asked, the question shows it, beside the question.
        if !self.json && !matches!(asking, Ok(true)) {
            printer.print_processes_to_approve(&processes);
        }

        if asking? {
            let question = format!(
                "Kill {} orphaned process{}?",
                processes.len(),
                plural(processes.len())
            );
            let list = || printer.print_processes_to_approve(&processes);
            let confirmed = prompt::ask("orphans --kill", &processes, None, &question, list)?;

            if !confirmed {
                printer.warning("Cancelled");
                return Ok(());
            }
        }

        let (killed, failed) = apply_to_each(&processes, |proc| proc.kill_and_wait());

        printer.print_action_result_as("orphans", "kill", &processes, &killed, &failed)
    }

    /// Windows stub: Windows does not re-parent a process whose parent exited,
    /// so there is no reliable orphan to find (or to kill)
    #[cfg(not(unix))]
    pub fn execute(&self) -> Result<()> {
        Process::find_orphans().map(|_| ())
    }
}
