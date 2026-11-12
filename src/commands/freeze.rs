//! `proc freeze` - Pause processes with SIGSTOP
//!
//! Examples:
//!   proc freeze node              # Freeze the processes named node
//!   proc freeze node --cmd        # ... or with node anywhere in their command line
//!   proc freeze :3000             # Freeze process on port 3000
//!   proc freeze :3000 :8080       # Freeze several targets: separate arguments
//!   proc freeze node --dry-run    # Show what would be frozen
//!   proc freeze :3000 --yes       # Skip confirmation (a port or PID, not a name)
//!
//! Commas still work (`proc freeze :3000,:8080`) but are deprecated.

#[cfg(unix)]
use crate::core::{
    apply_to_each, flatten_targets, guard_targets, nothing_resolved,
    resolve_targets_excluding_self, Guards, NameMatch, Process,
};
use crate::error::Result;
#[cfg(unix)]
use crate::ui::Printer;
use clap::Args;

/// Freeze (pause) process(es) with SIGSTOP
#[derive(Args, Debug)]
pub struct FreezeCommand {
    /// Target(s): process name, PID, or :port (separate arguments for several)
    #[arg(required = true, num_args = 1.., value_parser = crate::core::validate_target_arg)]
    pub target: Vec<String>,

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Show what would be frozen without actually freezing
    #[arg(long)]
    pub dry_run: bool,

    /// Output as JSON
    #[arg(long, short = 'j')]
    pub json: bool,

    /// Add PPID, STATUS and DIR to the table of processes it lists, and cut nothing
    #[arg(long, short = 'v')]
    pub verbose: bool,

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

impl FreezeCommand {
    /// Executes the freeze command, pausing matched processes with SIGSTOP.
    #[cfg(unix)]
    pub fn execute(&self) -> Result<()> {
        use nix::sys::signal::Signal;

        let printer =
            Printer::from_flags(self.json, self.verbose).scoped_to_dir(self.in_dir.is_some());

        let targets = flatten_targets(&self.target);
        let mode = NameMatch::from_flag(self.cmd);
        let resolved = resolve_targets_excluding_self(&targets, mode);

        if let Some(warning) = resolved.warning() {
            printer.warning(&warning);
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
            ));
        }

        if self.dry_run {
            printer.print_dry_run("freeze", &processes);
            return Ok(());
        }

        if !printer.ask_confirm("freeze", &processes, self.yes)? {
            return Ok(());
        }

        let (succeeded, failed) =
            apply_to_each(&processes, |proc| proc.send_signal(Signal::SIGSTOP));

        printer
            .with_processes(&processes)
            .print_action_result("freeze", &succeeded, &failed)
    }

    /// Windows stub
    #[cfg(not(unix))]
    pub fn execute(&self) -> Result<()> {
        Err(crate::error::ProcError::NotSupported(
            "freeze (SIGSTOP) is not supported on Windows".to_string(),
        ))
    }
}
