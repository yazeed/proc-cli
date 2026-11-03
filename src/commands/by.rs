//! `proc by` - Filter processes by name
//!
//! Examples:
//!   proc by node               # Processes named 'node'
//!   proc by node --cmd         # ... or with node anywhere in their command line
//!   proc by node --in .        # Node processes in current directory
//!   proc by node --min-cpu 5   # Node processes using >5% CPU
//!   proc by "my app"           # Processes with spaces in name

use crate::core::{resolve_in_dir, sort_processes, NameMatch, Process, SortKey, StatusFilter};
use crate::error::Result;
use crate::ui::Printer;
use clap::Args;
use std::path::PathBuf;

/// Filter processes by name
#[derive(Args, Debug)]
pub struct ByCommand {
    /// Process name to match (the whole name, ignoring case)
    pub name: String,

    /// Match the name as a substring of the process name or full command line (like pkill -f)
    #[arg(long)]
    pub cmd: bool,

    /// Filter by directory; takes the next word as the directory unless it starts with '-'
    /// (bare --in is the current directory, --in=DIR is explicit)
    #[arg(long = "in", short = 'i', num_args = 0..=1, default_missing_value = ".")]
    pub in_dir: Option<String>,

    /// Only show processes using more than this CPU %
    #[arg(long)]
    pub min_cpu: Option<f32>,

    /// Only show processes using more than this memory (MB)
    #[arg(long)]
    pub min_mem: Option<f64>,

    /// Filter by status: running, sleeping, stopped, zombie
    #[arg(long, value_enum, ignore_case = true)]
    pub status: Option<StatusFilter>,

    /// Only show processes running longer than this (seconds)
    #[arg(long)]
    pub min_uptime: Option<u64>,

    /// Only show children of this parent PID
    #[arg(long)]
    pub parent: Option<u32>,

    /// Output as JSON
    #[arg(long, short = 'j')]
    pub json: bool,

    /// Add PPID, STATUS and DIR to the table, and cut nothing
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// Print the directory and the command line whole instead of fitting the terminal width
    #[arg(long, short = 'w')]
    pub wide: bool,

    /// Limit the number of results
    #[arg(long, short = 'n')]
    pub limit: Option<usize>,

    /// Sort by: cpu, mem (biggest first); pid, name (ascending). Ties come by PID
    #[arg(long, short = 's', value_enum, default_value_t = SortKey::Cpu)]
    pub sort: SortKey,
}

impl ByCommand {
    /// Executes the by command, listing processes matching the name filter.
    pub fn execute(&self) -> Result<()> {
        let printer = Printer::from_flags(self.json, self.verbose)
            .wide(self.wide)
            .scoped_to_dir(self.in_dir.is_some());

        // Get processes by name
        let mut processes = Process::find_by_name(&self.name, NameMatch::from_flag(self.cmd))?;

        // Real CPU readings for the table, --min-cpu and --sort cpu
        Process::measure_cpu(&mut processes);

        // Resolve --in filter path
        let in_dir_filter = resolve_in_dir(&self.in_dir)?;
        let in_dir_has_any = in_dir_filter.as_ref().is_some_and(|dir_path| {
            processes
                .iter()
                .any(|p| crate::core::filters::matches_in_filter(p, dir_path))
        });
        crate::core::require_in_dir(&self.in_dir, &in_dir_filter, in_dir_has_any)?;

        // Apply filters
        processes.retain(|p| self.keeps(p, &in_dir_filter));

        // Sort processes
        sort_processes(&mut processes, self.sort);

        // Apply limit if specified
        if let Some(limit) = self.limit {
            processes.truncate(limit);
        }

        // Build context string for output
        let mut context_parts = vec![format!("by '{}'", self.name)];
        if let Some(ref dir) = in_dir_filter {
            context_parts.push(format!("in {}", dir.display()));
        }
        let context = Some(context_parts.join(" "));

        printer.print_processes_as("by", &processes, context.as_deref());
        Ok(())
    }

    /// Whether a process passes every filter: `--in`, `--min-cpu`, `--min-mem`,
    /// `--status`, `--min-uptime` and `--parent`.
    fn keeps(&self, p: &Process, in_dir_filter: &Option<PathBuf>) -> bool {
        // Directory filter (--in)
        if let Some(ref dir_path) = in_dir_filter {
            if !crate::core::filters::matches_in_filter(p, dir_path) {
                return false;
            }
        }

        // CPU filter
        if let Some(min_cpu) = self.min_cpu {
            if p.cpu_percent < min_cpu {
                return false;
            }
        }

        // Memory filter
        if let Some(min_mem) = self.min_mem {
            if p.memory_mb < min_mem {
                return false;
            }
        }

        // Status filter
        if let Some(status) = self.status {
            if !status.matches(p.status) {
                return false;
            }
        }

        // Uptime filter
        if let Some(min_uptime) = self.min_uptime {
            if let Some(start_time) = p.start_time {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                if now.saturating_sub(start_time) < min_uptime {
                    return false;
                }
            } else {
                return false;
            }
        }

        // Parent PID filter
        if let Some(ppid) = self.parent {
            if p.parent_pid != Some(ppid) {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ProcessStatus;

    fn command() -> ByCommand {
        ByCommand {
            name: "x".to_string(),
            cmd: false,
            in_dir: None,
            min_cpu: None,
            min_mem: None,
            status: None,
            min_uptime: None,
            parent: None,
            json: false,
            verbose: false,
            wide: false,
            limit: None,
            sort: SortKey::Cpu,
        }
    }

    /// A process record with no working directory and no start time, which
    /// some processes really are (a zombie has neither on Linux)
    fn bare_process() -> Process {
        Process {
            pid: 1,
            name: "x".to_string(),
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

    #[test]
    fn a_process_with_no_working_directory_fails_in() {
        let dir = Some(PathBuf::from("/"));
        assert!(!command().keeps(&bare_process(), &dir));
        assert!(command().keeps(&bare_process(), &None));
    }

    #[test]
    fn a_process_with_no_start_time_fails_min_uptime() {
        let cmd = ByCommand {
            min_uptime: Some(0),
            ..command()
        };
        assert!(!cmd.keeps(&bare_process(), &None));
    }
}
