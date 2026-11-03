//! `proc for` - Find processes by file path
//!
//! Examples:
//!   proc for ./server            # Processes running this program (or holding it open)
//!   proc for ./app               # Relative path
//!   proc for /var/log/app.log    # What has this file open? (not searched on Windows)
//!   proc for ~/bin/myapp         # Tilde expansion
//!   proc for app.log --by node   # Only the process named node (--cmd: node anywhere in the command line)

use crate::core::{
    resolve_in_dir, sort_processes, with_ports, NameMatch, PortInfo, Process, SortKey, StatusFilter,
};
use crate::error::{ProcError, Result};
use crate::out_println;
use crate::ui::{format_duration, format_memory, Printer};
use clap::Args;
use colored::*;
use serde::Serialize;
use std::collections::HashSet;
use std::path::PathBuf;

/// Find processes by file path
#[derive(Args, Debug)]
pub struct ForCommand {
    /// File path (relative, absolute, or with ~)
    pub file: String,

    /// Filter by working directory; takes the next word as the directory unless it starts with '-'
    /// (bare --in is the current directory, --in=DIR is explicit)
    #[arg(long = "in", short = 'i', num_args = 0..=1, default_missing_value = ".")]
    pub in_dir: Option<String>,

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

    /// Output as JSON
    #[arg(long, short = 'j')]
    pub json: bool,

    /// Show verbose output
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// Print full directories and arguments instead of fitting the terminal width
    #[arg(long, short = 'w')]
    pub wide: bool,

    /// Sort by: cpu, mem (biggest first); pid, name (ascending). Ties come by PID
    #[arg(long, short = 's', value_enum, default_value_t = SortKey::Cpu)]
    pub sort: SortKey,

    /// Limit the number of results
    #[arg(long, short = 'n')]
    pub limit: Option<usize>,
}

impl ForCommand {
    /// Executes the for command, finding processes by file path.
    pub fn execute(&self) -> Result<()> {
        // 1. Resolve file path (relative, tilde, canonicalize)
        let file_path = self.resolve_path(&self.file)?;

        // 2. Find processes by executable path
        let exe_processes = Process::find_by_exe_path(&file_path)?;

        // 3. Find processes with file open (lsof)
        let open_file_procs = Process::find_by_open_file(&file_path)?;

        // 4. Merge and deduplicate by PID
        let mut seen_pids = HashSet::new();
        let mut processes: Vec<Process> = Vec::new();

        // One loop for both lists: Windows has no open-file lookup, so its second
        // list is always empty and a loop of its own would never run there
        for proc in exe_processes.into_iter().chain(open_file_procs) {
            if seen_pids.insert(proc.pid) {
                processes.push(proc);
            }
        }

        // 5. Apply filters, with real CPU readings for --min-cpu, the sort and the view
        Process::measure_cpu(&mut processes);
        self.apply_filters(&mut processes)?;

        // 6. Sort processes
        sort_processes(&mut processes, self.sort);

        // 7. Apply limit if specified
        if let Some(limit) = self.limit {
            processes.truncate(limit);
        }

        // 8. Handle no results
        if processes.is_empty() {
            return Err(ProcError::ProcessNotFound(self.file.clone()));
        }

        // 9. For each process, get ports
        let results = with_ports(processes)?;

        // 10. Output
        if self.json {
            self.print_json(&results)?;
        } else {
            self.print_human(&results);
        }

        Ok(())
    }

    fn resolve_path(&self, path: &str) -> Result<PathBuf> {
        // Tilde expansion
        let expanded = if let Some(stripped) = path.strip_prefix("~/") {
            if let Ok(home) = std::env::var("HOME") {
                PathBuf::from(home).join(stripped)
            } else {
                PathBuf::from(path)
            }
        } else if path == "~" {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        } else {
            PathBuf::from(path)
        };

        // Make absolute if relative
        let absolute = if expanded.is_relative() {
            crate::core::filters::current_dir_for(&expanded)?.join(expanded)
        } else {
            expanded
        };

        // Canonicalize (resolve symlinks, normalize)
        let canonical = absolute
            .canonicalize()
            .map_err(|_| ProcError::InvalidInput(format!("File not found: {}", path)))?;

        // Windows returns `\\?\D:\...`, but process exe paths are `D:\...`
        #[cfg(windows)]
        let canonical = PathBuf::from(crate::core::filters::strip_verbatim_prefix(
            &canonical.to_string_lossy(),
        ));

        Ok(canonical)
    }

    fn apply_filters(&self, processes: &mut Vec<Process>) -> Result<()> {
        let in_dir_filter = resolve_in_dir(&self.in_dir)?;
        let in_dir_has_any = in_dir_filter.as_ref().is_some_and(|dir_path| {
            processes
                .iter()
                .any(|p| crate::core::filters::matches_in_filter(p, dir_path))
        });
        crate::core::require_in_dir(&self.in_dir, &in_dir_filter, in_dir_has_any)?;

        processes.retain(|p| {
            // Directory filter (--in)
            if let Some(ref dir_path) = in_dir_filter {
                if !crate::core::filters::matches_in_filter(p, dir_path) {
                    return false;
                }
            }

            // Name filter (--by)
            if let Some(ref name) = self.by_name {
                if !crate::core::matches_by_filter(p, name, NameMatch::from_flag(self.cmd)) {
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

            true
        });
        Ok(())
    }

    fn print_human(&self, results: &[(Process, Vec<PortInfo>)]) {
        let count = results.len();
        let file_display = &self.file;

        if count == 1 {
            // Single process - detailed view (like `on` command)
            let (proc, ports) = &results[0];

            out_println!(
                "{} Found 1 process for {}",
                "✓".green().bold(),
                file_display.cyan().bold()
            );
            out_println!();

            // Same labels and order as `proc on :port`
            out_println!(
                "  {} {} (PID {})",
                "Process:".bright_black(),
                proc.name.white().bold(),
                proc.pid.to_string().cyan()
            );
            if let Some(ref cwd) = proc.cwd {
                out_println!("  {} {}", "Directory:".bright_black(), cwd);
            }
            if let Some(ref path) = proc.exe_path {
                out_println!("  {} {}", "Path:".bright_black(), path.bright_black());
            }
            for port_info in ports {
                let addr = port_info.address.as_deref().unwrap_or("*");
                out_println!(
                    "  {} :{} ({} on {})",
                    "Listening:".bright_black(),
                    port_info.port.to_string().cyan(),
                    format!("{:?}", port_info.protocol).to_uppercase(),
                    addr
                );
            }
            out_println!(
                "  {} {:.1}% CPU, {}",
                "Resources:".bright_black(),
                proc.cpu_percent,
                format_memory(proc.memory_mb)
            );
            if let Some(uptime) = proc.uptime_secs {
                out_println!("  {} {}", "Uptime:".bright_black(), format_duration(uptime));
            }
            if self.verbose {
                if let Some(ref cmd) = proc.command {
                    out_println!("  {} {}", "Command:".bright_black(), cmd.bright_black());
                }
            }
            out_println!();
        } else {
            // Multiple processes: the standard process table, then who listens where
            let processes: Vec<Process> = results.iter().map(|(p, _)| p.clone()).collect();
            Printer::from_flags(false, self.verbose)
                .wide(self.wide)
                .print_processes_as("for", &processes, Some(&format!("for {}", file_display)));

            let listening: Vec<_> = results
                .iter()
                .flat_map(|(proc, ports)| ports.iter().map(move |p| (proc, p)))
                .collect();
            if !listening.is_empty() {
                out_println!("  {}", "Listening:".bright_black());
                for (proc, port_info) in listening {
                    let addr = port_info.address.as_deref().unwrap_or("*");
                    out_println!(
                        "    {} :{} ({} on {})  {} [PID {}]",
                        "→".bright_black(),
                        port_info.port.to_string().cyan(),
                        format!("{:?}", port_info.protocol).to_uppercase(),
                        addr,
                        proc.name.white(),
                        proc.pid.to_string().cyan()
                    );
                }
                out_println!();
            }
        }
    }

    fn print_json(&self, results: &[(Process, Vec<PortInfo>)]) -> Result<()> {
        let printer = Printer::from_flags(true, self.verbose);
        let items: Vec<ProcessForJson> = results
            .iter()
            .map(|(proc, ports)| ProcessForJson {
                process: proc,
                ports,
            })
            .collect();

        printer.print_json(&ForOutput {
            action: "for",
            success: true,
            count: items.len(),
            results: &items,
        });
        Ok(())
    }
}

#[derive(Serialize)]
struct ProcessForJson<'a> {
    process: &'a Process,
    ports: &'a [PortInfo],
}

#[derive(Serialize)]
struct ForOutput<'a> {
    action: &'static str,
    success: bool,
    count: usize,
    results: &'a [ProcessForJson<'a>],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ProcessStatus;

    fn command() -> ForCommand {
        ForCommand {
            file: "x".to_string(),
            in_dir: None,
            by_name: None,
            cmd: false,
            min_cpu: None,
            min_mem: None,
            status: None,
            min_uptime: None,
            json: false,
            verbose: false,
            wide: false,
            sort: SortKey::Cpu,
            limit: None,
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
        let cmd = ForCommand {
            in_dir: Some("/".to_string()),
            ..command()
        };
        let mut processes = vec![bare_process()];
        cmd.apply_filters(&mut processes).unwrap();
        assert!(processes.is_empty());

        let mut processes = vec![bare_process()];
        command().apply_filters(&mut processes).unwrap();
        assert_eq!(processes.len(), 1);
    }

    #[test]
    fn a_process_with_no_start_time_fails_min_uptime() {
        let cmd = ForCommand {
            min_uptime: Some(0),
            ..command()
        };
        let mut processes = vec![bare_process()];
        cmd.apply_filters(&mut processes).unwrap();
        assert!(processes.is_empty());
    }
}
