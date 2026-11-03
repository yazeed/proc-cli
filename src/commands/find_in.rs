//! `proc in` - Filter processes by working directory
//!
//! Examples:
//!   proc in .                  # Processes in current directory
//!   proc in /path/to/project   # Processes in specific directory
//!   proc in . --by node        # Processes named node in cwd
//!   proc in . --by node --cmd  # ... or with node anywhere in their command line
//!   proc in ~/projects         # Processes in ~/projects

use crate::core::filters::resolve_dir;
use crate::core::{sort_processes, NameMatch, Process, SortKey, StatusFilter};
use crate::error::{ProcError, Result};
use crate::ui::{OutputFormat, Printer};
use clap::Args;
use std::path::PathBuf;

/// Filter processes by working directory
#[derive(Args, Debug)]
pub struct InCommand {
    /// Directory path (absolute, relative, or . for cwd)
    pub path: String,

    /// Filter by process name
    #[arg(long = "by", short = 'b')]
    pub by_name: Option<String>,

    /// Match --by as a substring of the process name or full command line (like pkill -f)
    #[arg(long)]
    pub cmd: bool,

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

    /// Add PPID and STATUS to the table, and cut nothing (DIR is always there)
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

impl InCommand {
    /// Expand ~ to home directory
    fn expand_tilde(path: &str) -> PathBuf {
        if let Some(stripped) = path.strip_prefix("~/") {
            if let Ok(home) = std::env::var("HOME") {
                return PathBuf::from(home).join(stripped);
            }
        } else if path == "~" {
            if let Ok(home) = std::env::var("HOME") {
                return PathBuf::from(home);
            }
        }
        PathBuf::from(path)
    }

    /// Executes the in command, listing processes in the specified directory.
    pub fn execute(&self) -> Result<()> {
        let format = if self.json {
            OutputFormat::Json
        } else {
            OutputFormat::Human
        };
        // `in` is scoped to a directory by definition
        let printer = Printer::new(format, self.verbose)
            .wide(self.wide)
            .scoped_to_dir(true);

        // Get base process list
        let mut processes = if let Some(ref name) = self.by_name {
            Process::find_by_name(name, NameMatch::from_flag(self.cmd))?
        } else {
            Process::find_all()?
        };

        // Real CPU readings for the table, --min-cpu and --sort cpu
        Process::measure_cpu(&mut processes);

        // Resolve directory path
        let dir_filter = resolve_dir(if self.path == "." {
            PathBuf::from(".")
        } else {
            Self::expand_tilde(&self.path)
        })?;

        // Directory filter (required for this command)
        processes.retain(|p| crate::core::filters::matches_in_filter(p, &dir_filter));

        // A missing path with nothing running there is a typo, not an empty
        // directory (same as `proc for`). A deleted directory that processes
        // still run in is kept, so its leftovers can be found.
        if processes.is_empty() && !dir_filter.is_dir() {
            return Err(ProcError::InvalidInput(format!(
                "Directory not found: {}",
                self.path
            )));
        }

        // Apply filters
        processes.retain(|p| self.keeps(p));

        // Sort processes
        sort_processes(&mut processes, self.sort);

        // Apply limit if specified
        if let Some(limit) = self.limit {
            processes.truncate(limit);
        }

        // Build context string for output
        let mut context_parts = vec![format!("in {}", dir_filter.display())];
        if let Some(ref name) = self.by_name {
            context_parts.push(format!("by '{}'", name));
        }
        let context = Some(context_parts.join(" "));

        printer.print_processes_as("in", &processes, context.as_deref());
        Ok(())
    }

    /// Whether a process passes every filter but the directory: `--min-cpu`,
    /// `--min-mem`, `--status`, `--min-uptime` and `--parent`.
    fn keeps(&self, p: &Process) -> bool {
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

    fn command() -> InCommand {
        InCommand {
            path: ".".to_string(),
            by_name: None,
            cmd: false,
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

    /// A process record with no start time, which some processes really are
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
    fn a_process_with_no_start_time_fails_min_uptime() {
        let cmd = InCommand {
            min_uptime: Some(0),
            ..command()
        };
        assert!(!cmd.keeps(&bare_process()));
        assert!(command().keeps(&bare_process()));
    }

    #[test]
    fn a_process_that_passes_every_filter_is_kept() {
        let mut process = bare_process();
        process.start_time = Some(0);
        process.parent_pid = Some(7);
        let cmd = InCommand {
            min_cpu: Some(0.0),
            min_mem: Some(0.0),
            status: Some(StatusFilter::Sleeping),
            min_uptime: Some(1),
            parent: Some(7),
            ..command()
        };
        assert!(cmd.keeps(&process));
        let other_parent = InCommand {
            parent: Some(8),
            ..command()
        };
        assert!(!other_parent.keeps(&process));
    }
}
