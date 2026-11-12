//! `proc kill` - Kill processes
//!
//! Examples:
//!   proc kill node              # Kill the processes named node
//!   proc kill node --cmd        # ... or with node anywhere in their command line
//!   proc kill :3000             # Kill what's on port 3000
//!   proc kill 1234              # Kill specific PID
//!   proc kill 1234 5678         # Kill several targets: separate arguments
//!   proc kill :3000 :8080 node  # Mixed targets (port + port + name)
//!   proc kill node --dry-run    # Show what would be killed, kill nothing
//!   proc kill :3000 --yes       # Skip confirmation (a port or PID, not a name)
//!
//! Commas still work (`proc kill :3000,:8080`) but are deprecated.

use crate::core::{
    apply_to_each, flatten_targets, guard_targets, nothing_resolved,
    resolve_targets_excluding_self, Guards, NameMatch, Process,
};
use crate::error::Result;
use crate::ui::Printer;
use clap::Args;

/// Kill process(es)
#[derive(Args, Debug)]
pub struct KillCommand {
    /// Target(s): process name, PID, or :port (separate arguments for several)
    #[arg(required = true, num_args = 1.., value_parser = crate::core::validate_target_arg)]
    pub target: Vec<String>,

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Show what would be killed without actually killing
    #[arg(long)]
    pub dry_run: bool,

    /// Output as JSON
    #[arg(long, short = 'j')]
    pub json: bool,

    /// Add PPID, STATUS and DIR to the table of processes it lists, and cut nothing
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// Send SIGTERM instead of SIGKILL (graceful)
    #[arg(long, short = 'g')]
    pub graceful: bool,

    /// Filter by directory; takes the next word as the directory unless it starts with '-'
    /// (bare --in is the current directory, --in=DIR is explicit)
    #[arg(long = "in", short = 'i', num_args = 0..=1, default_missing_value = ".")]
    pub in_dir: Option<String>,

    /// Filter by process name
    #[arg(long = "by", short = 'b')]
    pub by_name: Option<String>,

    /// Act only if the process still has this start time (Unix seconds, `start_time`
    /// in its JSON): a PID the OS gave to a new process is ruled out. One PID target
    #[arg(long = "start-time", value_name = "SECONDS", value_parser = crate::core::values::whole_seconds, allow_negative_numbers = true)]
    pub start_time: Option<u64>,

    /// Match name targets and --by as a substring of the process name or full command line (like pkill -f)
    #[arg(long)]
    pub cmd: bool,
}

impl KillCommand {
    /// Executes the kill command, forcefully terminating matched processes.
    pub fn execute(&self) -> Result<()> {
        let printer =
            Printer::from_flags(self.json, self.verbose).scoped_to_dir(self.in_dir.is_some());

        // Split each value on commas and resolve to processes
        // Use resolve_targets_excluding_self to avoid killing ourselves
        let targets = flatten_targets(&self.target);
        let mode = NameMatch::from_flag(self.cmd);
        let resolved = resolve_targets_excluding_self(&targets, mode);

        // Warn about targets that weren't found; when nothing was found at
        // all, the error says it instead
        if !resolved.only_not_found() {
            if let Some(warning) = resolved.warning() {
                printer.warning(&warning);
            }
        }
        // the JSON results list the targets that matched nothing, and the
        // processes proc runs under that a target matched and left out
        let printer = printer
            .with_not_found(&resolved.not_found)
            .with_left_out(&resolved.left_out_ancestors());
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
            return Err(nothing_resolved(
                &self.target,
                &resolved.unseen,
                &resolved.left_out,
                &resolved.command_line,
            ));
        }

        // Dry run: just show what would be killed
        if self.dry_run {
            printer.print_dry_run("kill", &processes);
            return Ok(());
        }

        // Confirm before killing (unless --yes)
        if !printer.ask_confirm("kill", &processes, self.yes)? {
            return Ok(());
        }

        // Kill the processes
        let (killed, failed) = apply_to_each(&processes, |proc| {
            if self.graceful {
                proc.terminate()
            } else {
                proc.kill()
            }
        });

        printer
            .with_processes(&processes)
            .print_action_result("kill", &killed, &failed)
    }
}
