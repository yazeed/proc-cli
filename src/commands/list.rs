//! `proc list` - List processes
//!
//! Examples:
//!   proc list                  # List all processes
//!   proc list node             # Only the processes named node
//!   proc list node --cmd       # ... or with node anywhere in their command line
//!   proc list node python      # Several names: separate arguments
//!   proc list --in             # Processes in current directory
//!   proc list --in /project    # Processes in /project
//!   proc list --min-cpu 10     # Processes using >10% CPU
//!
//! Commas still work (`proc list node,python`) but are deprecated.

use crate::core::filters::resolve_dir;
use crate::core::target::{includes_note, left_out_warning};
use crate::core::{
    flatten_targets, resolve_in_dir, sort_processes, Ancestor, Ancestors, NameMatch, Process,
    SortKey, StatusFilter,
};
use crate::error::{ProcError, Result};
use crate::ui::Printer;
use clap::Args;
use std::path::PathBuf;

/// List processes
#[derive(Args, Debug)]
pub struct ListCommand {
    /// Process name(s) to filter by (separate arguments for several)
    #[arg(num_args = 0..)]
    pub name: Vec<String>,

    /// Match the names as a substring of the process name or full command line (like pkill -f)
    #[arg(long)]
    pub cmd: bool,

    /// Filter by directory; takes the next word as the directory unless it starts with '-'
    /// (bare --in is the current directory, --in=DIR is explicit)
    #[arg(long = "in", short = 'i', num_args = 0..=1, default_missing_value = ".")]
    pub in_dir: Option<String>,

    /// Filter by executable path
    #[arg(long, short = 'p')]
    pub path: Option<String>,

    /// Only show processes using more than this CPU %
    #[arg(long, value_parser = crate::core::values::cpu_percent, allow_negative_numbers = true)]
    pub min_cpu: Option<f32>,

    /// Only show processes using more than this memory (MB)
    #[arg(long, value_parser = crate::core::values::megabytes, allow_negative_numbers = true)]
    pub min_mem: Option<f64>,

    /// Filter by status: running, sleeping, stopped, zombie, idle, starting, disk-wait
    #[arg(long, value_enum, ignore_case = true)]
    pub status: Option<StatusFilter>,

    /// Only show processes running longer than this (whole seconds)
    #[arg(long, value_parser = crate::core::values::whole_seconds, allow_negative_numbers = true)]
    pub min_uptime: Option<u64>,

    /// Only show children of this parent PID
    #[arg(long, value_parser = crate::core::values::pid, allow_negative_numbers = true)]
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
    #[arg(long, short = 'n', value_parser = crate::core::values::count, allow_negative_numbers = true)]
    pub limit: Option<usize>,

    /// Sort by: cpu, mem (biggest first); pid, name (ascending). Ties come by PID
    #[arg(long, short = 's', value_enum, default_value_t = SortKey::Cpu)]
    pub sort: SortKey,
}

impl ListCommand {
    /// Executes the list command, displaying processes matching the filters.
    pub fn execute(&self) -> Result<()> {
        let printer = Printer::from_flags(self.json, self.verbose)
            .wide(self.wide)
            .scoped_to_dir(self.in_dir.is_some());

        // Get base process list (several names as separate arguments; each may
        // still be a comma-separated list). A process proc runs under is listed,
        // marked, when its name matches; a match in its command line is left
        // out, and named
        let mut left_out: Vec<Ancestor> = Vec::new();
        // the names that match no process by name but are in command lines
        let mut command_line: Vec<(String, usize)> = Vec::new();
        let mut processes = if !self.name.is_empty() {
            let names = flatten_targets(&self.name);
            let mut all = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for n in &names {
                match Process::lookup_name(n, NameMatch::from_flag(self.cmd), Ancestors::Mark) {
                    Ok(found) => {
                        for p in found.processes {
                            if seen.insert(p.pid) {
                                all.push(p);
                            }
                        }
                        left_out.extend(found.left_out);
                    }
                    Err(ProcError::AncestorsOnly { ancestors, .. }) => left_out.extend(ancestors),
                    Err(error) => command_line.extend(error.command_line_match()),
                }
            }
            all
        } else {
            Process::find_all()?
        };
        left_out.sort_by_key(|a| a.pid);
        left_out.dedup_by_key(|a| a.pid);
        if let Some(warning) = left_out_warning(&left_out) {
            printer.warning(&warning);
        }
        let printer = printer
            .with_left_out(&left_out)
            .with_command_line(&command_line);

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

        // Resolve path filter
        let path_filter: Option<PathBuf> = match &self.path {
            Some(p) => Some(resolve_dir(PathBuf::from(p))?),
            None => None,
        };

        // Apply filters
        processes.retain(|p| self.keeps(p, &in_dir_filter, &path_filter));

        // Sort processes
        sort_processes(&mut processes, self.sort);

        // Apply limit if specified
        if let Some(limit) = self.limit {
            processes.truncate(limit);
        }

        // Build context string for output (e.g., "in /path/to/dir")
        let context = in_dir_filter
            .as_ref()
            .map(|p| format!("in {}", p.display()));

        printer.print_processes_with_context(&processes, context.as_deref());
        // only a name chose these: a full list shows proc's ancestors anyway
        if !self.name.is_empty() {
            if let Some(note) = includes_note(&processes) {
                printer.note(&note);
            }
        }
        Ok(())
    }

    /// Whether a process passes every filter: `--in`, `--path`, `--min-cpu`,
    /// `--min-mem`, `--status`, `--min-uptime` and `--parent`.
    fn keeps(
        &self,
        p: &Process,
        in_dir_filter: &Option<PathBuf>,
        path_filter: &Option<PathBuf>,
    ) -> bool {
        // Directory filter (--in)
        if let Some(ref dir_path) = in_dir_filter {
            if !crate::core::filters::matches_in_filter(p, dir_path) {
                return false;
            }
        }

        // Path filter (executable path)
        if let Some(ref exe_path) = path_filter {
            if let Some(ref proc_exe) = p.exe_path {
                let proc_path = PathBuf::from(proc_exe);
                if !proc_path.starts_with(exe_path) {
                    return false;
                }
            } else {
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

    fn command() -> ListCommand {
        ListCommand {
            name: Vec::new(),
            cmd: false,
            in_dir: None,
            path: None,
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

    /// A process record with no working directory, executable or start time,
    /// which some processes really are (a zombie has none on Linux; protected
    /// system processes on Windows and other users' on macOS have no cwd)
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
            runs_proc: false,
        }
    }

    #[test]
    fn a_process_with_no_working_directory_fails_in() {
        let dir = Some(PathBuf::from("/"));
        assert!(!command().keeps(&bare_process(), &dir, &None));
        assert!(command().keeps(&bare_process(), &None, &None));
    }

    #[test]
    fn a_process_with_no_executable_fails_path() {
        let path = Some(PathBuf::from("/usr"));
        assert!(!command().keeps(&bare_process(), &None, &path));
    }

    #[test]
    fn a_process_with_no_start_time_fails_min_uptime() {
        let cmd = ListCommand {
            min_uptime: Some(0),
            ..command()
        };
        assert!(!cmd.keeps(&bare_process(), &None, &None));
    }

    #[test]
    fn a_process_that_passes_every_filter_is_kept() {
        let mut process = bare_process();
        process.cwd = Some("/srv/app".to_string());
        process.exe_path = Some("/usr/bin/node".to_string());
        process.start_time = Some(0);
        process.parent_pid = Some(7);
        let cmd = ListCommand {
            min_cpu: Some(0.0),
            min_mem: Some(0.0),
            status: Some(StatusFilter::Sleeping),
            min_uptime: Some(1),
            parent: Some(7),
            ..command()
        };
        let dir = Some(PathBuf::from("/srv"));
        let path = Some(PathBuf::from("/usr/bin"));
        assert!(cmd.keeps(&process, &dir, &path));
        // and each filter on its own can still turn it away
        let other_parent = ListCommand {
            parent: Some(8),
            ..command()
        };
        assert!(!other_parent.keeps(&process, &dir, &path));
    }

    #[test]
    fn each_filter_turns_away_a_process_that_fails_it() {
        let mut process = bare_process();
        process.start_time = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
        process.memory_mb = 1.0;
        let turned_away = [
            ListCommand {
                min_cpu: Some(50.0),
                ..command()
            },
            ListCommand {
                min_mem: Some(500.0),
                ..command()
            },
            ListCommand {
                status: Some(StatusFilter::Zombie),
                ..command()
            },
            ListCommand {
                min_uptime: Some(3600),
                ..command()
            },
            ListCommand {
                parent: Some(7),
                ..command()
            },
        ];
        for cmd in &turned_away {
            assert!(!cmd.keeps(&process, &None, &None), "{cmd:?}");
        }
    }
}
