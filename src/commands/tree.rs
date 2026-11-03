//! Tree command - Show process tree
//!
//! Usage:
//!   proc tree              # Full process tree
//!   proc tree node         # Tree for the processes named node
//!   proc tree node --cmd   # ... or with node anywhere in their command line
//!   proc tree :3000        # Tree for process on port 3000
//!   proc tree 1234         # Tree for PID 1234
//!   proc tree --min-cpu 10 # Only processes using >10% CPU
//!   proc tree 1234 -a      # Show ancestry (path UP to root)

use crate::core::{
    classify, matches_by_filter, matches_in_filter, parse_target, resolve_in_dir, resolve_target,
    ruled_out_by, Guards, Hit, NameMatch, Process, ProcessStatus, StatusFilter, TargetType,
};
use crate::error::{ProcError, Result};
use crate::out_println;
use crate::ui::{format_memory, plural, uptime_label, Printer};
use clap::Args;
use colored::*;
use serde::Serialize;
use std::collections::HashMap;

/// Show process tree
#[derive(Args, Debug)]
pub struct TreeCommand {
    /// Target: process name, :port, or PID (shows full tree if omitted)
    #[arg(value_parser = crate::core::validate_target_arg)]
    target: Option<String>,

    /// Show ancestry (path UP to root) instead of descendants
    #[arg(long, short)]
    ancestors: bool,

    /// Show verbose output
    #[arg(long, short = 'v')]
    verbose: bool,

    /// Output as JSON
    #[arg(long, short = 'j')]
    json: bool,

    /// Maximum depth to display
    #[arg(long, short, default_value = "10")]
    depth: usize,

    /// Show PIDs only (compact view)
    #[arg(long, short = 'C')]
    compact: bool,

    /// Only show processes using more than this CPU %
    #[arg(long)]
    min_cpu: Option<f32>,

    /// Only show processes using more than this memory (MB)
    #[arg(long)]
    min_mem: Option<f64>,

    /// Filter by status: running, sleeping, stopped, zombie
    #[arg(long, value_enum, ignore_case = true)]
    status: Option<StatusFilter>,

    /// Only show processes running longer than this (seconds)
    #[arg(long)]
    min_uptime: Option<u64>,

    /// Filter by directory; takes the next word as the directory unless it starts with '-'
    /// (bare --in is the current directory, --in=DIR is explicit)
    #[arg(long = "in", short = 'i', num_args = 0..=1, default_missing_value = ".")]
    pub in_dir: Option<String>,

    /// Filter by process name
    #[arg(long = "by", short = 'b')]
    pub by_name: Option<String>,

    /// Match name targets and --by as a substring of the process name or full command line (like pkill -f)
    #[arg(long)]
    pub cmd: bool,
}

impl TreeCommand {
    /// The matches to draw trees from. A match that a root above it draws in
    /// its tree (no more than `--depth` levels down) is not a root itself:
    /// drawing it again showed it twice (a shell and what it starts share a
    /// cwd, so `--in` matches both). Roots are chosen top down, so a match
    /// counts as drawn only under one that is drawn. The walk up is bounded,
    /// so a loop in the parent links cannot hold it.
    fn topmost<'a>(
        &self,
        matches: Vec<&'a Process>,
        pid_map: &HashMap<u32, &Process>,
    ) -> Vec<&'a Process> {
        // each match with the PIDs above it, nearest first
        let above: Vec<(&Process, Vec<u32>)> = matches
            .into_iter()
            .map(|p| {
                let up = std::iter::successors(p.parent_pid, |ppid| {
                    pid_map.get(ppid).and_then(|q| q.parent_pid)
                })
                .take(pid_map.len())
                .collect();
                (p, up)
            })
            .collect();
        let mut top_down: Vec<&(&Process, Vec<u32>)> = above.iter().collect();
        top_down.sort_by_key(|(_, up)| up.len());
        let mut roots = std::collections::HashSet::new();
        for (p, up) in top_down {
            if !up.iter().take(self.depth).any(|pid| roots.contains(pid)) {
                roots.insert(p.pid);
            }
        }
        above
            .into_iter()
            .filter(|(p, _)| roots.contains(&p.pid))
            .map(|(p, _)| p)
            .collect()
    }

    /// Build the set of ancestor PIDs to exclude from name matching.
    ///
    /// Walks up the process tree from the current process to avoid matching
    /// shells whose command lines contain proc's own arguments. `all_processes`
    /// leaves proc itself out, so its parent is looked up separately.
    fn ancestor_pids(all_processes: &[Process]) -> std::collections::HashSet<u32> {
        let mut pids = std::collections::HashSet::new();
        let self_pid = std::process::id();
        pids.insert(self_pid);
        let mut next = Process::find_by_pid(self_pid)
            .ok()
            .flatten()
            .and_then(|me| me.parent_pid);
        for _ in 0..10 {
            let Some(parent_pid) = next else { break };
            pids.insert(parent_pid);
            next = all_processes
                .iter()
                .find(|p| p.pid == parent_pid)
                .and_then(|p| p.parent_pid);
        }
        pids
    }

    /// The processes a name matches, leaving out `ancestors` (proc and the shells
    /// above it), and how many others have the name only in their command line
    /// (what `--cmd` would add): both come from the one pass over `all`.
    fn named<'a>(
        all: impl Iterator<Item = &'a Process>,
        ancestors: &std::collections::HashSet<u32>,
        pattern: &str,
        mode: NameMatch,
    ) -> (Vec<&'a Process>, usize) {
        let mut others = 0;
        let named = all
            .filter(|p| !ancestors.contains(&p.pid))
            .filter(|p| match classify(pattern, &p.names(), mode) {
                Hit::Match => true,
                Hit::CommandLineOnly => {
                    others += 1;
                    false
                }
                Hit::No => false,
            })
            .collect();
        (named, others)
    }

    /// Executes the tree command, displaying the process hierarchy.
    pub fn execute(&self) -> Result<()> {
        let printer = Printer::from_flags(self.json, self.verbose);

        // Ancestry is of a target; with none there is nothing to trace, and
        // answering with an empty result would look like success
        let ancestry_of = match (self.ancestors, &self.target) {
            (true, None) => {
                return Err(ProcError::InvalidInput(
                    "--ancestors needs a target (PID, :port, or name)".to_string(),
                ))
            }
            (true, Some(target)) => Some(target),
            (false, _) => None,
        };

        // Get all processes, with real CPU readings for the nodes and --min-cpu
        let mut all_processes = Process::find_all()?;
        Process::measure_cpu(&mut all_processes);

        // The one place the tree's order is decided: PID ascending. The roots,
        // every parent's children, the filter list, the target list and the
        // ancestry targets all come from walking this list (or `children_map`,
        // built from it), so the human tree and `--json` read alike.
        all_processes.sort_by_key(|p| p.pid);

        // Build PID -> Process map for quick lookup
        let pid_map: HashMap<u32, &Process> = all_processes.iter().map(|p| (p.pid, p)).collect();

        // Build parent -> children map
        let mut children_map: HashMap<u32, Vec<&Process>> = HashMap::new();

        for proc in &all_processes {
            if let Some(ppid) = proc.parent_pid {
                children_map.entry(ppid).or_default().push(proc);
            }
        }

        // --in and --by narrow a target's processes. With no target they pick
        // the processes shown, each with its subtree, as --min-cpu does; they
        // used to be ignored there, so `tree --in .` printed the whole system
        let mode = NameMatch::from_flag(self.cmd);
        let in_dir_filter = resolve_in_dir(&self.in_dir)?;
        let in_dir_has_any = in_dir_filter
            .as_ref()
            .is_some_and(|dir_path| all_processes.iter().any(|p| matches_in_filter(p, dir_path)));
        crate::core::require_in_dir(&self.in_dir, &in_dir_filter, in_dir_has_any)?;
        let in_and_by = |p: &Process| {
            in_dir_filter
                .as_ref()
                .is_none_or(|dir_path| matches_in_filter(p, dir_path))
                && self
                    .by_name
                    .as_ref()
                    .is_none_or(|name| matches_by_filter(p, name, mode))
        };

        // Handle --ancestors mode
        if let Some(target) = ancestry_of {
            return self.show_ancestors(&printer, target, &pid_map, &all_processes, &in_and_by);
        }

        // Determine target processes
        let target_processes: Vec<&Process> = if let Some(ref target) = self.target {
            // Use unified target resolution
            match parse_target(target) {
                TargetType::Port(_) | TargetType::Pid(_) => {
                    // For port or PID, resolve to specific process(es)
                    // Errors (not found) rather than returning nothing
                    let resolved = resolve_target(target, mode)?;
                    // Find matching processes in all_processes
                    let pids: Vec<u32> = resolved.iter().map(|p| p.pid).collect();
                    all_processes
                        .iter()
                        .filter(|p| pids.contains(&p.pid))
                        .collect()
                }
                TargetType::Name(ref pattern) => {
                    // For name, do pattern matching (exclude self and parent to avoid
                    // false positives from proc's own args in the shell command line)
                    let ancestor_pids = Self::ancestor_pids(&all_processes);
                    let (named, others) =
                        Self::named(all_processes.iter(), &ancestor_pids, pattern, mode);
                    // A name that matches nothing is not found, as a PID or a
                    // port that matches nothing is
                    if named.is_empty() {
                        return Err(ProcError::name_not_found(pattern, others));
                    }
                    named
                }
            }
        } else {
            Vec::new() // Will show full tree
        };

        let target_processes: Vec<&Process> = if let Some(target) = &self.target {
            let found: Vec<Process> = target_processes.iter().map(|p| (*p).clone()).collect();
            let kept: Vec<&Process> = target_processes
                .into_iter()
                .filter(|p| in_and_by(p))
                .collect();
            // The target found processes (none would have been an error above),
            // so an empty list means --in or --by ruled them all out: the same
            // guard error as `info` and `on`, not an empty answer
            if kept.is_empty() {
                let guards = Guards::new(&self.in_dir, &self.by_name, None);
                return Err(ProcError::FilteredOut {
                    target: target.clone(),
                    ruled_out_by: ruled_out_by(&found, &guards, mode)?,
                });
            }
            kept
        } else {
            target_processes
        };

        // Apply resource filters if specified
        let matches_filters = |p: &Process| self.matches_filters(p);

        // Apply filters to target processes or find filtered roots
        let has_filters = in_dir_filter.is_some()
            || self.by_name.is_some()
            || self.min_cpu.is_some()
            || self.min_mem.is_some()
            || self.status.is_some()
            || self.min_uptime.is_some();

        if self.json {
            let tree_nodes = if self.target.is_some() {
                target_processes
                    .iter()
                    .filter(|p| matches_filters(p))
                    .map(|p| self.build_tree_node(p, &children_map, 0))
                    .collect()
            } else if has_filters {
                // Show only processes matching filters, each once
                let matches = all_processes
                    .iter()
                    .filter(|p| in_and_by(p) && matches_filters(p))
                    .collect();
                self.topmost(matches, &pid_map)
                    .into_iter()
                    .map(|p| self.build_tree_node(p, &children_map, 0))
                    .collect()
            } else {
                // Show full tree from roots
                all_processes
                    .iter()
                    .filter(|p| p.parent_pid.is_none() || p.parent_pid == Some(0))
                    .map(|p| self.build_tree_node(p, &children_map, 0))
                    .collect()
            };

            printer.print_json(&TreeOutput {
                action: "tree",
                success: true,
                tree: tree_nodes,
            });
        } else if self.target.is_some() {
            let filtered: Vec<_> = target_processes
                .into_iter()
                .filter(|p| matches_filters(p))
                .collect();
            if filtered.is_empty() {
                // Only --min-cpu, --min-mem, --status or --min-uptime get here:
                // they narrow the view, so nothing left is an empty answer, as
                // in `list`
                let target = self.target.as_ref().unwrap();
                printer.warning(&format!(
                    "No processes for '{target}' match the specified filters"
                ));
                return Ok(());
            }

            out_println!(
                "{} Process tree for '{}':\n",
                "✓".green().bold(),
                self.target.as_ref().unwrap().cyan()
            );

            for proc in &filtered {
                self.print_tree(proc, &children_map, "", true, 0);
                out_println!();
            }
        } else if has_filters {
            let filtered: Vec<_> = all_processes
                .iter()
                .filter(|p| in_and_by(p) && matches_filters(p))
                .collect();
            if filtered.is_empty() {
                printer.warning("No processes match the specified filters");
                return Ok(());
            }

            out_println!(
                "{} {} process{} matching filters:\n",
                "✓".green().bold(),
                filtered.len().to_string().cyan().bold(),
                plural(filtered.len())
            );

            let roots = self.topmost(filtered, &pid_map);
            for (i, proc) in roots.iter().enumerate() {
                let is_last = i == roots.len() - 1;
                self.print_tree(proc, &children_map, "", is_last, 0);
            }
        } else {
            out_println!("{} Process tree:\n", "✓".green().bold());

            // Find processes with PID 1 or no parent as roots
            let display_roots: Vec<&Process> = all_processes
                .iter()
                .filter(|p| p.parent_pid.is_none() || p.parent_pid == Some(0))
                .collect();

            for (i, proc) in display_roots.iter().enumerate() {
                let is_last = i == display_roots.len() - 1;
                self.print_tree(proc, &children_map, "", is_last, 0);
            }
        }

        Ok(())
    }

    /// Whether a process passes `--min-cpu`, `--min-mem`, `--status` and `--min-uptime`
    fn matches_filters(&self, p: &Process) -> bool {
        if let Some(min_cpu) = self.min_cpu {
            if p.cpu_percent < min_cpu {
                return false;
            }
        }
        if let Some(min_mem) = self.min_mem {
            if p.memory_mb < min_mem {
                return false;
            }
        }
        if let Some(status) = self.status {
            if !status.matches(p.status) {
                return false;
            }
        }
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
    }

    fn print_tree(
        &self,
        proc: &Process,
        children_map: &HashMap<u32, Vec<&Process>>,
        prefix: &str,
        is_last: bool,
        depth: usize,
    ) {
        if depth > self.depth {
            return;
        }

        let connector = if is_last { "└── " } else { "├── " };

        if self.compact {
            out_println!(
                "{}{}{}",
                prefix.bright_black(),
                connector.bright_black(),
                proc.pid.to_string().cyan()
            );
        } else {
            let status_indicator = status_indicator(proc.status);

            out_println!(
                "{}{}{} {} [{}] {:.1}% {}{}",
                prefix.bright_black(),
                connector.bright_black(),
                status_indicator,
                proc.name.white().bold(),
                proc.pid.to_string().cyan(),
                proc.cpu_percent,
                format_memory(proc.memory_mb),
                uptime_label(proc.uptime_secs).bright_black()
            );
        }

        let child_prefix = if is_last {
            format!("{}    ", prefix)
        } else {
            format!("{}│   ", prefix)
        };

        // the children are already in PID order (see `execute`)
        if let Some(children) = children_map.get(&proc.pid) {
            for (i, child) in children.iter().enumerate() {
                let child_is_last = i == children.len() - 1;
                self.print_tree(child, children_map, &child_prefix, child_is_last, depth + 1);
            }
        }
    }

    fn build_tree_node(
        &self,
        proc: &Process,
        children_map: &HashMap<u32, Vec<&Process>>,
        depth: usize,
    ) -> TreeNode {
        let children = if depth < self.depth {
            children_map
                .get(&proc.pid)
                .map(|kids| {
                    kids.iter()
                        .map(|p| self.build_tree_node(p, children_map, depth + 1))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        TreeNode {
            pid: proc.pid,
            name: proc.name.clone(),
            cpu_percent: proc.cpu_percent,
            memory_mb: proc.memory_mb,
            status: proc.status,
            uptime_secs: proc.uptime_secs,
            children,
        }
    }

    /// Show ancestry (path UP to root) for target processes; `all_processes` is
    /// the process list in PID order, which the targets of a name follow
    fn show_ancestors(
        &self,
        printer: &Printer,
        target: &str,
        pid_map: &HashMap<u32, &Process>,
        all_processes: &[Process],
        in_and_by: &dyn Fn(&Process) -> bool,
    ) -> Result<()> {
        use crate::core::{parse_target, resolve_target, TargetType};

        let mode = NameMatch::from_flag(self.cmd);

        // Resolve target to processes
        let target_processes: Vec<Process> = match parse_target(target) {
            TargetType::Port(_) | TargetType::Pid(_) => resolve_target(target, mode)?,
            TargetType::Name(ref pattern) => {
                let ancestor_pids = Self::ancestor_pids(all_processes);
                let (named, others) =
                    Self::named(all_processes.iter(), &ancestor_pids, pattern, mode);
                if named.is_empty() {
                    return Err(ProcError::name_not_found(pattern, others));
                }
                named.into_iter().cloned().collect()
            }
        };
        // --in and --by guard the target here too, as without --ancestors
        let found = target_processes.clone();
        let target_processes: Vec<Process> = target_processes
            .into_iter()
            .filter(|p| in_and_by(p))
            .collect();
        if target_processes.is_empty() {
            let guards = Guards::new(&self.in_dir, &self.by_name, None);
            return Err(ProcError::FilteredOut {
                target: target.to_string(),
                ruled_out_by: ruled_out_by(&found, &guards, mode)?,
            });
        }

        if self.json {
            let ancestry_output: Vec<AncestryNode> = target_processes
                .iter()
                .map(|proc| self.build_ancestry_node(proc, pid_map))
                .collect();
            printer.print_json(&AncestryOutput {
                action: "tree",
                success: true,
                ancestry: ancestry_output,
            });
        } else {
            out_println!("{} Ancestry for '{}':\n", "✓".green().bold(), target.cyan());

            for proc in &target_processes {
                self.print_ancestry(proc, pid_map);
                out_println!();
            }
        }

        Ok(())
    }

    /// Trace and print ancestry from root down to target
    fn print_ancestry(&self, target: &Process, pid_map: &HashMap<u32, &Process>) {
        // Root first, target last
        let chain = ancestor_chain(target, pid_map);

        // Print the chain
        for (i, proc) in chain.iter().enumerate() {
            let is_target = proc.pid == target.pid;
            let indent = "    ".repeat(i);
            let connector = if i == 0 { "" } else { "└── " };

            let status_indicator = status_indicator(proc.status);

            if is_target {
                // Highlight the target
                out_println!(
                    "{}{}{} {} [{}] {:.1}% {}{}  {}",
                    indent.bright_black(),
                    connector.bright_black(),
                    status_indicator,
                    proc.name.cyan().bold(),
                    proc.pid.to_string().cyan().bold(),
                    proc.cpu_percent,
                    format_memory(proc.memory_mb),
                    uptime_label(proc.uptime_secs).bright_black(),
                    "← target".yellow()
                );
            } else {
                out_println!(
                    "{}{}{} {} [{}] {:.1}% {}{}",
                    indent.bright_black(),
                    connector.bright_black(),
                    status_indicator,
                    proc.name.white(),
                    proc.pid.to_string().cyan(),
                    proc.cpu_percent,
                    format_memory(proc.memory_mb),
                    uptime_label(proc.uptime_secs).bright_black()
                );
            }
        }
    }

    /// Build ancestry node for JSON output
    fn build_ancestry_node(
        &self,
        target: &Process,
        pid_map: &HashMap<u32, &Process>,
    ) -> AncestryNode {
        let chain: Vec<ProcessInfo> = ancestor_chain(target, pid_map)
            .into_iter()
            .map(|proc| ProcessInfo {
                pid: proc.pid,
                name: proc.name.clone(),
                cpu_percent: proc.cpu_percent,
                memory_mb: proc.memory_mb,
                status: proc.status,
                uptime_secs: proc.uptime_secs,
            })
            .collect();

        AncestryNode {
            target_pid: target.pid,
            target_name: target.name.clone(),
            depth: chain.len(),
            chain,
        }
    }
}

/// The marker for a process status in the tree
fn status_indicator(status: ProcessStatus) -> ColoredString {
    match status {
        ProcessStatus::Running => "●".green(),
        ProcessStatus::Sleeping => "○".blue(),
        ProcessStatus::Stopped => "◐".yellow(),
        ProcessStatus::Zombie => "✗".red(),
        _ => "?".white(),
    }
}

/// The processes from the root down to `target`, found by following parent
/// PIDs. The chain ends at a parent the process list doesn't hold (the root's
/// parent is 0, or the parent is gone) and is cut at 100 so a loop in the
/// parent links cannot run forever.
fn ancestor_chain<'a>(
    target: &'a Process,
    pid_map: &HashMap<u32, &'a Process>,
) -> Vec<&'a Process> {
    let mut chain: Vec<&Process> = Vec::new();
    let mut current_pid = Some(target.pid);

    while let Some(pid) = current_pid {
        let Some(proc) = pid_map.get(&pid) else { break };
        chain.push(proc);
        current_pid = proc.parent_pid;
        if chain.len() > 100 {
            break;
        }
    }

    chain.reverse();
    chain
}

#[derive(Serialize)]
struct AncestryOutput {
    action: &'static str,
    success: bool,
    ancestry: Vec<AncestryNode>,
}

#[derive(Serialize)]
struct AncestryNode {
    target_pid: u32,
    target_name: String,
    depth: usize,
    chain: Vec<ProcessInfo>,
}

#[derive(Serialize)]
struct ProcessInfo {
    pid: u32,
    name: String,
    cpu_percent: f32,
    memory_mb: f64,
    status: ProcessStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    uptime_secs: Option<u64>,
}

#[derive(Serialize)]
struct TreeOutput {
    action: &'static str,
    success: bool,
    tree: Vec<TreeNode>,
}

#[derive(Serialize)]
struct TreeNode {
    pid: u32,
    name: String,
    cpu_percent: f32,
    memory_mb: f64,
    status: ProcessStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    uptime_secs: Option<u64>,
    children: Vec<TreeNode>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command() -> TreeCommand {
        TreeCommand {
            target: None,
            ancestors: false,
            verbose: false,
            json: false,
            depth: 10,
            compact: false,
            min_cpu: None,
            min_mem: None,
            status: None,
            min_uptime: None,
            in_dir: None,
            by_name: None,
            cmd: false,
        }
    }

    /// A process record with no start time, which some processes really are
    fn process(pid: u32, parent_pid: Option<u32>) -> Process {
        Process {
            pid,
            name: format!("p{pid}"),
            exe_path: None,
            cwd: None,
            command: None,
            argv0: None,
            raw: None,
            cpu_percent: 0.0,
            memory_mb: 0.0,
            status: ProcessStatus::Sleeping,
            user: None,
            parent_pid,
            start_time: None,
            uptime_secs: None,
            parent_name: None,
            cpu_time_secs: None,
            open_files: None,
            child_count: None,
        }
    }

    #[test]
    fn a_name_matches_whole_and_counts_the_processes_only_cmd_would_add() {
        let mut node = process(1, None);
        node.name = "node".to_string();
        let mut editor = process(2, None);
        editor.name = "vim".to_string();
        editor.command = Some("vim node-notes.md".to_string());
        let mut shell = process(3, None);
        shell.name = "node".to_string();
        let all = [node, editor, shell];
        let skipped: std::collections::HashSet<u32> = [3].into();

        let (named, others) = TreeCommand::named(all.iter(), &skipped, "node", NameMatch::Exact);
        assert_eq!(named.iter().map(|p| p.pid).collect::<Vec<_>>(), vec![1]);
        assert_eq!(others, 1);

        let (named, others) = TreeCommand::named(all.iter(), &skipped, "node", NameMatch::Command);
        assert_eq!(named.iter().map(|p| p.pid).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(others, 0);

        let (named, others) = TreeCommand::named(all.iter(), &skipped, "python", NameMatch::Exact);
        assert!(named.is_empty());
        assert_eq!(others, 0);
    }

    #[test]
    fn a_process_with_no_start_time_fails_min_uptime() {
        let cmd = TreeCommand {
            min_uptime: Some(0),
            ..command()
        };
        assert!(!cmd.matches_filters(&process(1, None)));
        assert!(command().matches_filters(&process(1, None)));
    }

    #[test]
    fn every_status_has_its_own_marker() {
        let color = |status| status_indicator(status).fgcolor;
        let text = |status| status_indicator(status).trim().to_string();
        assert_eq!(text(ProcessStatus::Running), "●");
        assert_eq!(text(ProcessStatus::Sleeping), "○");
        assert_eq!(text(ProcessStatus::Stopped), "◐");
        assert_eq!(text(ProcessStatus::Zombie), "✗");
        assert_eq!(text(ProcessStatus::Dead), "?");
        assert_eq!(text(ProcessStatus::Unknown), "?");
        assert_eq!(color(ProcessStatus::Running), Some(Color::Green));
        assert_eq!(color(ProcessStatus::Sleeping), Some(Color::Blue));
        assert_eq!(color(ProcessStatus::Stopped), Some(Color::Yellow));
        assert_eq!(color(ProcessStatus::Zombie), Some(Color::Red));
        assert_eq!(color(ProcessStatus::Unknown), Some(Color::White));
    }

    #[test]
    fn the_chain_runs_from_the_root_down_to_the_target() {
        let (root, middle, leaf) = (
            process(1, Some(0)),
            process(2, Some(1)),
            process(3, Some(2)),
        );
        let map: HashMap<u32, &Process> = [&root, &middle, &leaf]
            .iter()
            .map(|p| (p.pid, *p))
            .collect();
        let chain: Vec<u32> = ancestor_chain(&leaf, &map).iter().map(|p| p.pid).collect();
        assert_eq!(chain, vec![1, 2, 3]);
    }

    #[test]
    fn a_loop_in_the_parent_links_is_cut() {
        let (a, b) = (process(1, Some(2)), process(2, Some(1)));
        let map: HashMap<u32, &Process> = [&a, &b].iter().map(|p| (p.pid, *p)).collect();
        assert_eq!(ancestor_chain(&a, &map).len(), 101);
    }

    #[test]
    fn a_process_that_passes_every_filter_is_kept() {
        let mut p = process(1, None);
        p.start_time = Some(0);
        let cmd = TreeCommand {
            min_cpu: Some(0.0),
            min_mem: Some(0.0),
            status: Some(StatusFilter::Sleeping),
            min_uptime: Some(1),
            ..command()
        };
        assert!(cmd.matches_filters(&p));
        let other_status = TreeCommand {
            status: Some(StatusFilter::Running),
            ..command()
        };
        assert!(!other_status.matches_filters(&p));
    }
}
