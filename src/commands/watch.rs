//! `proc watch` - Real-time process monitoring
//!
//! Examples:
//!   proc watch                     # Watch all processes (alias: proc top)
//!   proc watch node                # Watch the processes named node
//!   proc watch node --cmd          # ... or with node anywhere in their command line
//!   proc watch :3000               # Watch process on port 3000
//!   proc watch :3000 :8080 node    # Watch several targets: separate arguments
//!   proc watch --in . --by node    # Watch node processes in current directory
//!   proc watch -n 1 --sort mem     # 1s refresh, sorted by memory
//!   proc watch --json | head -2    # NDJSON output
//!
//! Commas still work (`proc watch :3000,:8080`) but are deprecated.

use crate::core::process::{
    classify_sysinfo, fill_family, matches_outside_command_line, refreshed_system,
    self_and_ancestors, sysinfo_locked,
};
use crate::core::target::{includes_note, left_out_warning};
use crate::core::{
    flatten_targets, guard_targets, nothing_resolved, parse_target, resolve_in_dir,
    resolve_targets, sort_processes, Ancestor, Ancestors, Guards, Hit, NameMatch, Process, SortKey,
    TargetType,
};
use crate::error::Result;
use crate::ui::output::screen_of;
use crate::ui::table::{process_table, Screen, TableLayout};
use crate::ui::{interrupt, out};
use clap::{Args, ValueEnum};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute, terminal,
};
use serde::Serialize;
use std::collections::HashSet;
use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};

/// A refresh interval must be a real number of seconds, and not so small that
/// watch spins in a busy loop (0, NaN, infinity and negatives used to panic or spin)
fn parse_interval(s: &str) -> std::result::Result<f64, String> {
    let secs: f64 = s
        .parse()
        .map_err(|_| format!("'{s}' is not a number of seconds"))?;
    if !secs.is_finite() || secs < 0.1 {
        return Err(format!("must be at least 0.1 seconds, got '{s}'"));
    }
    Ok(secs)
}

/// Watch processes in real-time
#[derive(Args, Debug, Clone)]
pub struct WatchCommand {
    /// Target(s): PID, :port, or name (separate arguments for several; omit for all processes)
    #[arg(num_args = 0.., value_parser = crate::core::validate_target_arg)]
    pub target: Vec<String>,

    /// Refresh interval in seconds (at least 0.1)
    #[arg(long = "interval", short = 'n', default_value = "2", value_parser = parse_interval, allow_negative_numbers = true)]
    pub interval: f64,

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

    /// Only show processes using more than this CPU %
    #[arg(long, value_parser = crate::core::values::cpu_percent, allow_negative_numbers = true)]
    pub min_cpu: Option<f32>,

    /// Only show processes using more than this memory (MB)
    #[arg(long, value_parser = crate::core::values::megabytes, allow_negative_numbers = true)]
    pub min_mem: Option<f64>,

    /// Output as JSON (newline-delimited, one object per refresh)
    #[arg(long, short = 'j')]
    pub json: bool,

    /// Add PPID, STATUS and DIR to the table, and cut nothing
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// Print the directory and the command line whole instead of fitting the terminal width
    #[arg(long, short = 'w')]
    pub wide: bool,

    /// Limit the number of results shown
    #[arg(long, short = 'l', value_parser = crate::core::values::count, allow_negative_numbers = true)]
    pub limit: Option<usize>,

    /// Sort by: cpu, mem (biggest first); pid, name (ascending). Ties come by PID
    #[arg(long, short = 's', value_enum, default_value_t = WatchSortKey::Cpu)]
    pub sort: WatchSortKey,

    /// `--in` as `execute` resolved it, absolute; every refresh uses this one
    #[arg(skip)]
    in_dir_path: Option<std::path::PathBuf>,

    /// The names among the targets that matched no process by name at the
    /// first look but are in command lines, each with how many: an empty view
    /// says so, with the command that shows them
    #[arg(skip)]
    command_line: Vec<(String, usize)>,
}

/// Sort key for watch command (mirrors SortKey but with independent default)
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum WatchSortKey {
    /// Sort by CPU usage (descending)
    Cpu,
    /// Sort by memory usage (descending)
    Mem,
    /// Sort by process ID (ascending)
    Pid,
    /// Sort by process name (ascending)
    Name,
}

impl From<WatchSortKey> for SortKey {
    fn from(key: WatchSortKey) -> Self {
        match key {
            WatchSortKey::Cpu => SortKey::Cpu,
            WatchSortKey::Mem => SortKey::Mem,
            WatchSortKey::Pid => SortKey::Pid,
            WatchSortKey::Name => SortKey::Name,
        }
    }
}

#[derive(Serialize)]
struct WatchJsonOutput {
    action: &'static str,
    success: bool,
    count: usize,
    processes: Vec<Process>,
    /// The processes proc runs under that a name matched and that were left
    /// out (with --cmd); absent when there are none
    #[serde(skip_serializing_if = "Vec::is_empty")]
    left_out: Vec<Ancestor>,
}

/// The PIDs of the processes listening on `port`, none if nothing does
fn port_holders(port: u16) -> Vec<u32> {
    crate::core::PortInfo::holders_of(port)
        .unwrap_or_default()
        .into_iter()
        .map(|info| info.pid)
        .collect()
}

/// Restore the terminal if watch panics, so the shell is not left in raw mode
/// on the alternate screen, then run the hook that was there before. (A signal
/// is handled by `interrupt`.) The codes are written whether or not an earlier
/// write failed: the terminal may well still be there.
fn install_panic_hook() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        interrupt::restore_screen(&mut out::stdout_unrecorded());
        original_hook(panic_info);
    }));
}

/// A terminal that can't be switched or read (the screen, raw mode, a key) is
/// a system error. Not `?`'s own conversion, which reads a missing device as a
/// process not found
fn screen_error(e: std::io::Error) -> crate::error::ProcError {
    crate::error::ProcError::SystemError(e.to_string())
}

/// Whether a key press ends the watch: q, Esc or Ctrl+C (raw mode turns Ctrl+C
/// into a key instead of a signal)
fn is_quit_key(code: KeyCode, modifiers: KeyModifiers) -> bool {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => true,
        KeyCode::Char('c') => modifiers.contains(KeyModifiers::CONTROL),
        _ => false,
    }
}

/// Draw one frame: clear the screen and write each line, which needs `\r\n`
/// in raw mode
fn draw(out: &mut impl Write, frame: &str) -> std::io::Result<()> {
    execute!(out, cursor::MoveTo(0, 0))?;
    execute!(out, terminal::Clear(terminal::ClearType::All))?;
    execute!(out, cursor::MoveTo(0, 0))?;
    for line in frame.lines() {
        write!(out, "{}\r\n", line)?;
    }
    out.flush()
}

/// Write one NDJSON line and flush it. Returns false once the reader is gone
/// (a broken pipe), which ends the stream; a frame that cannot be serialized is
/// reported on `err` and skipped.
fn write_json_frame(out: &mut impl Write, err: &mut impl Write, frame: &impl Serialize) -> bool {
    match serde_json::to_string(frame) {
        Ok(json) => writeln!(out, "{}", json).is_ok() && out.flush().is_ok(),
        Err(e) => {
            let _ = writeln!(err, "JSON serialization error: {}", e);
            true
        }
    }
}

impl WatchCommand {
    /// Execute the watch command — real-time process monitoring.
    pub fn execute(&self) -> Result<()> {
        // A missing --in directory is an error before the first frame, not a
        // view of nothing that looks like an answer. A deleted directory that
        // processes still run in is kept, so its leftovers can be watched.
        let in_dir_path = resolve_in_dir(&self.in_dir)?;
        crate::core::require_in_dir_live(&self.in_dir, &in_dir_path)?;
        self.require_targets_found()?;
        // Every refresh uses the directory as resolved now, so a relative --in
        // never depends on the current directory still existing later
        let resolved = WatchCommand {
            in_dir_path,
            command_line: self.names_in_command_lines(),
            ..self.clone()
        };
        resolved.run()
    }

    /// The names among the targets that match no process by name but are in
    /// command lines, each with how many; none with `--cmd`, which matches them
    fn names_in_command_lines(&self) -> Vec<(String, usize)> {
        if self.cmd {
            return Vec::new();
        }
        flatten_targets(&self.target)
            .iter()
            .filter(|t| matches!(parse_target(t), TargetType::Name(_)))
            .filter_map(|name| {
                Process::lookup_name(name, NameMatch::Exact, Ancestors::Mark)
                    .err()?
                    .command_line_match()
            })
            .collect()
    }

    /// PID targets are a fixed question: when no PID is held, or `--in` and
    /// `--by` rule out every process they name, that is the error the other
    /// commands give (exit 2), not empty frames forever, since a PID nothing
    /// holds won't come back as the process that was meant. A name or a port
    /// is a live question, watched until something matches (`proc watch node`
    /// before node starts). Only the first look decides: a process that exits
    /// while it is watched leaves an empty frame. `--min-cpu` and `--min-mem`
    /// never decide, since a process can cross them a frame later.
    fn require_targets_found(&self) -> Result<()> {
        let targets = flatten_targets(&self.target);
        let pids_only = !targets.is_empty()
            && targets
                .iter()
                .all(|t| matches!(parse_target(t), TargetType::Pid(_)));
        if !pids_only {
            return Ok(());
        }
        let mode = NameMatch::from_flag(self.cmd);
        let resolved = resolve_targets(&targets, mode);
        let mut processes = resolved.processes;
        if processes.is_empty() {
            return Err(nothing_resolved(
                &self.target,
                &resolved.unseen,
                &resolved.left_out,
                &resolved.command_line,
            ));
        }
        let guards = Guards::new(&self.in_dir, &self.by_name, None);
        guard_targets(&mut processes, &self.target, &guards, mode)
    }

    /// `execute` once `--in` is resolved: a JSON stream, the live view, or one snapshot
    fn run(&self) -> Result<()> {
        let is_tty = out::stdout_is_terminal();

        if self.json {
            self.run_json_loop()
        } else if is_tty && std::io::stdin().is_terminal() {
            self.run_tui_loop()
        } else {
            // Without a terminal on both ends there is no screen to draw on or
            // key to quit with: one snapshot, like `list`. Say so when stdout
            // is a terminal, where a person is waiting for a live view.
            if is_tty {
                crate::ui::print_note(
                    "stdin is not a terminal, so there is no live view to quit with a key; \
                     showing one snapshot (--json streams one line per refresh)",
                );
            }
            self.run_snapshot()
        }
    }

    /// Collect processes for one cycle using the persistent System instance
    fn collect_processes(&self, sys: &System) -> Vec<Process> {
        self.collect_with(sys, port_holders)
    }

    /// `collect_processes` with the port lookup passed in: the holders of a port
    /// are read at a different moment than `sys`, so one can be a process `sys`
    /// has not seen
    fn collect_with(&self, sys: &System, port_holders: impl Fn(u16) -> Vec<u32>) -> Vec<Process> {
        let self_pid = Pid::from_u32(std::process::id());
        // a name finds a process proc runs under when its name matches; with
        // --cmd it is left out (see `left_out`), since its command line holds
        // proc's own arguments (`sh -c 'proc watch node'`)
        let ancestors = self_and_ancestors(sys);
        let in_dir_filter = &self.in_dir_path;
        let mode = NameMatch::from_flag(self.cmd);

        // Parse targets once
        let targets: Vec<TargetType> = flatten_targets(&self.target)
            .iter()
            .map(|t| parse_target(t))
            .collect();

        let mut seen_pids = HashSet::new();
        let mut processes = Vec::new();

        if targets.is_empty() {
            // No target: all processes
            for (pid, proc) in sys.processes() {
                if *pid == self_pid || crate::core::is_thread(proc) {
                    continue;
                }
                // a map's keys are unique, so every PID here is new; it is
                // noted for the zombies below
                seen_pids.insert(pid.as_u32());
                processes.push(Process::from_sysinfo(*pid, proc));
            }
            // the zombies sysinfo leaves out, which `list` shows
            let zombies = crate::core::process::unlisted_zombies(&seen_pids);
            processes.extend(zombies.into_iter().filter(|z| z.pid != self_pid.as_u32()));
        } else {
            for target in &targets {
                match target {
                    TargetType::Port(port) => {
                        for owner in port_holders(*port) {
                            let pid = Pid::from_u32(owner);
                            if let Some(proc) = sys.process(pid) {
                                if seen_pids.insert(owner) {
                                    processes.push(Process::from_sysinfo(pid, proc));
                                }
                            }
                        }
                    }
                    TargetType::Pid(pid) => {
                        let sysinfo_pid = Pid::from_u32(*pid);
                        let found = sys
                            .process(sysinfo_pid)
                            .map(|proc| Process::from_sysinfo(sysinfo_pid, proc))
                            .or_else(|| crate::core::process::unlisted_zombie(*pid));
                        if let Some(found) = found {
                            if seen_pids.insert(*pid) {
                                processes.push(found);
                            }
                        }
                    }
                    TargetType::Name(name) => {
                        for (pid, proc) in sys.processes() {
                            if *pid == self_pid
                                || (ancestors.contains(pid) && mode != NameMatch::Exact)
                                || crate::core::is_thread(proc)
                            {
                                continue;
                            }
                            if classify_sysinfo(name, proc, mode) == Hit::Match
                                && seen_pids.insert(pid.as_u32())
                            {
                                processes.push(Process::from_sysinfo(*pid, proc));
                            }
                        }
                    }
                }
            }
        }

        // Apply --by filter
        if let Some(ref by_name) = self.by_name {
            processes.retain(|p| crate::core::matches_by_filter(p, by_name, mode));
        }

        // Apply --in filter
        if let Some(ref dir_path) = in_dir_filter {
            processes.retain(|p| crate::core::filters::matches_in_filter(p, dir_path));
        }

        // Apply --min-cpu filter
        if let Some(min_cpu) = self.min_cpu {
            processes.retain(|p| p.cpu_percent >= min_cpu);
        }

        // Apply --min-mem filter
        if let Some(min_mem) = self.min_mem {
            processes.retain(|p| p.memory_mb >= min_mem);
        }

        // `sys` holds every process, so parents and children are known
        fill_family(&mut processes, sys);

        // Sort
        sort_processes(&mut processes, self.sort.into());

        // Apply limit
        if let Some(limit) = self.limit {
            processes.truncate(limit);
        }

        processes
    }

    /// TUI loop: alternate screen + raw mode
    fn run_tui_loop(&self) -> Result<()> {
        let mut stdout = out::stdout();

        install_panic_hook();
        // A signal (kill, closing the window) puts the screen back too; armed
        // before the screen changes, and dropped once it is back
        let _interrupts = interrupt::protect_screen();

        // Enter alternate screen + raw mode
        execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide).map_err(screen_error)?;
        terminal::enable_raw_mode().map_err(screen_error)?;

        let mut sys = refreshed_system();
        let interval = Duration::from_secs_f64(self.interval);

        // The snapshot above is the baseline for the first CPU readings
        std::thread::sleep(Duration::from_millis(250));

        let result = self.tui_event_loop(&mut sys, &mut stdout, interval);

        // Whether or not a write failed on the way: the terminal may be fine
        interrupt::restore_screen(&mut out::stdout_unrecorded());

        result
    }

    fn tui_event_loop(
        &self,
        sys: &mut System,
        stdout: &mut impl Write,
        interval: Duration,
    ) -> Result<()> {
        let left_out = self.left_out(sys);
        loop {
            sysinfo_locked(|| sys.refresh_all());
            let processes = self.collect_processes(sys);

            // Get terminal size
            let (width, height) = terminal::size().unwrap_or((120, 40));

            // Build frame
            let frame = self.render_frame_with(&processes, &left_out, screen_of(width), height);

            // A write that fails ends the watch. `out` has the policy for what
            // that means: a reader that left changes nothing, and any other
            // failure it recorded becomes the exit code and the message.
            if draw(stdout, &frame).is_err() {
                return Ok(());
            }

            // Wait for interval or keypress
            let deadline = Instant::now() + interval;
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }

                if event::poll(remaining).map_err(screen_error)? {
                    if let Event::Key(KeyEvent {
                        code, modifiers, ..
                    }) = event::read().map_err(screen_error)?
                    {
                        if is_quit_key(code, modifiers) {
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    /// Render a frame for the TUI, with nothing left out
    #[cfg(test)]
    fn render_frame(&self, processes: &[Process], screen: Screen, height: u16) -> String {
        self.render_frame_with(processes, &[], screen, height)
    }

    /// Whether a target is a name (not a PID or a port)
    fn names_a_process(&self) -> bool {
        flatten_targets(&self.target)
            .iter()
            .any(|t| matches!(parse_target(t), TargetType::Name(_)))
    }

    /// Render a frame for the TUI, saying under the header which processes
    /// proc runs under were left out
    fn render_frame_with(
        &self,
        processes: &[Process],
        left_out: &[Ancestor],
        screen: Screen,
        height: u16,
    ) -> String {
        let sort_label = match self.sort {
            WatchSortKey::Cpu => "CPU",
            WatchSortKey::Mem => "Memory",
            WatchSortKey::Pid => "PID",
            WatchSortKey::Name => "Name",
        };

        let target_label = if self.target.is_empty() {
            "all".to_string()
        } else {
            self.target.join(" ")
        };

        // Header line
        let header = format!(
            " Watching {} | {} processes | Sort: {} | Refresh: {:.1}s | q to exit",
            target_label,
            processes.len(),
            sort_label,
            self.interval,
        );

        let mut output = String::new();
        output.push_str(&header);
        output.push('\n');
        if let Some(warning) = left_out_warning(left_out) {
            output.push_str(&format!(" ⚠ {warning}\n"));
        }
        output.push('\n');

        if processes.is_empty() {
            output.push_str(" No matching processes found.");
            if !self.command_line.is_empty() {
                output.push('\n');
                output.push_str(&crate::error::command_line_lines(&self.command_line));
            }
            return output;
        }

        // Cap rows to terminal height minus header (2 lines) + table header (1) + padding (1)
        let max_rows = (height as usize).saturating_sub(5);
        let display_count = processes.len().min(max_rows);

        // The same table as `list` prints: the layout comes from the same flags
        let layout =
            TableLayout::from_flags(screen, self.verbose, self.wide, self.in_dir.is_some());
        output.push_str(&process_table(&processes[..display_count], &layout));

        if processes.len() > display_count {
            output.push('\n');
            output.push_str(&format!(
                " ... and {} more (use --limit to control)",
                processes.len() - display_count
            ));
        }

        output
    }

    /// The processes proc runs under that a name target matches and that
    /// watch leaves out: with --cmd only, since an exact name keeps them. They
    /// don't change while proc runs, so this is read once
    fn left_out(&self, sys: &System) -> Vec<Ancestor> {
        let mode = NameMatch::from_flag(self.cmd);
        if mode == NameMatch::Exact {
            return Vec::new();
        }
        let self_pid = Pid::from_u32(std::process::id());
        let names: Vec<String> = flatten_targets(&self.target)
            .iter()
            .filter_map(|t| match parse_target(t) {
                TargetType::Name(name) => Some(name),
                _ => None,
            })
            .collect();
        let mut left_out: Vec<Ancestor> = self_and_ancestors(sys)
            .into_iter()
            .filter(|pid| *pid != self_pid)
            .filter_map(|pid| sys.process(pid).map(|proc| (pid, proc)))
            .filter(|(_, proc)| {
                names
                    .iter()
                    .any(|n| classify_sysinfo(n, proc, mode) == Hit::Match)
            })
            .map(|(pid, proc)| Process::from_sysinfo(pid, proc))
            // a hit in its command line alone is proc's own arguments
            .filter(|p| {
                names
                    .iter()
                    .any(|n| matches_outside_command_line(p, n, mode))
            })
            .map(|p| Ancestor::from(&p))
            .collect();
        left_out.sort_by_key(|a| a.pid);
        left_out
    }

    /// JSON loop: NDJSON output, one line per refresh
    fn run_json_loop(&self) -> Result<()> {
        let mut sys = refreshed_system();
        let interval = Duration::from_secs_f64(self.interval);

        // The snapshot above is the baseline for the first CPU readings
        std::thread::sleep(Duration::from_millis(250));

        let left_out = self.left_out(&sys);
        loop {
            sysinfo_locked(|| sys.refresh_all());
            let processes = self.collect_processes(&sys);

            let output = WatchJsonOutput {
                action: "watch",
                success: true,
                count: processes.len(),
                processes,
                left_out: left_out.clone(),
            };

            if !write_json_frame(&mut out::stdout(), &mut out::stderr(), &output) {
                return Ok(()); // Broken pipe — exit cleanly
            }

            // Not a plain sleep: a reader that leaves is noticed at once, not a
            // whole interval later (see `out::reader_gone_within`)
            if out::reader_gone_within(interval) {
                return Ok(());
            }
        }
    }

    /// Single snapshot (non-TTY mode)
    fn run_snapshot(&self) -> Result<()> {
        let mut sys = refreshed_system();
        std::thread::sleep(Duration::from_millis(250));
        sysinfo_locked(|| sys.refresh_all());

        let processes = self.collect_processes(&sys);

        let printer = crate::ui::Printer::from_flags(false, self.verbose)
            .wide(self.wide)
            .scoped_to_dir(self.in_dir.is_some())
            .with_command_line(&self.command_line);
        if let Some(warning) = left_out_warning(&self.left_out(&sys)) {
            printer.warning(&warning);
        }
        printer.print_processes(&processes);
        if self.names_a_process() {
            if let Some(note) = includes_note(&processes) {
                printer.note(&note);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ProcessStatus;
    use crate::test_support::FailsAfter;
    use serde::ser::Error as _;
    use std::io;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    fn watch_args() -> WatchCommand {
        WatchCommand {
            target: Vec::new(),
            interval: 2.0,
            in_dir: None,
            by_name: None,
            cmd: false,
            min_cpu: None,
            min_mem: None,
            json: false,
            verbose: false,
            wide: false,
            limit: None,
            sort: WatchSortKey::Cpu,
            in_dir_path: None,
            command_line: Vec::new(),
        }
    }

    fn process(pid: u32, name: &str) -> Process {
        Process {
            pid,
            name: name.to_string(),
            exe_path: None,
            cwd: None,
            command: None,
            argv0: None,
            raw: None,
            cpu_percent: 1.5,
            memory_mb: 12.0,
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

    /// The text of a frame without colour codes, which comfy-table adds only
    /// when stdout is a terminal
    fn plain(frame: &str) -> String {
        let mut out = String::new();
        let mut chars = frame.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for end in chars.by_ref() {
                    if end.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn colour_codes_are_stripped_from_a_frame() {
        assert_eq!(
            plain("\x1b[1;36m4242\x1b[0m  \x1b[32mRunning\x1b[0m"),
            "4242  Running"
        );
    }

    fn lines_with<'a>(frame: &'a str, needle: &str) -> Vec<&'a str> {
        frame.lines().filter(|l| l.contains(needle)).collect()
    }

    /// A terminal error is a system error with the system's words, whatever
    /// its kind: a missing device is not a process not found
    #[test]
    fn a_terminal_that_fails_is_a_system_error() {
        for kind in [io::ErrorKind::NotFound, io::ErrorKind::Other] {
            let err = screen_error(io::Error::new(kind, "no terminal"));
            assert!(
                matches!(&err, crate::error::ProcError::SystemError(m) if m == "no terminal"),
                "{err:?}"
            );
        }
    }

    // ---- keys ----

    #[test]
    fn q_escape_and_ctrl_c_quit() {
        assert!(is_quit_key(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(is_quit_key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(is_quit_key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    }

    #[test]
    fn other_keys_do_not_quit() {
        assert!(!is_quit_key(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(!is_quit_key(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(!is_quit_key(KeyCode::Enter, KeyModifiers::CONTROL));
        assert!(!is_quit_key(KeyCode::Char('x'), KeyModifiers::CONTROL));
    }

    // ---- the terminal ----

    #[test]
    fn a_frame_is_drawn_from_the_top_with_raw_mode_line_ends() {
        let mut out = Vec::new();
        draw(&mut out, "one\ntwo").unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("\x1b[1;1H\x1b[2J\x1b[1;1H"), "{text:?}");
        assert!(text.ends_with("one\r\ntwo\r\n"), "{text:?}");
    }

    #[test]
    fn a_frame_stops_at_the_first_failed_write() {
        let frame = "one\ntwo";
        let mut all = FailsAfter {
            left: usize::MAX,
            written: Vec::new(),
        };
        draw(&mut all, frame).unwrap();
        for limit in 0..all.written.len() {
            let mut out = FailsAfter {
                left: limit,
                written: Vec::new(),
            };
            let err = draw(&mut out, frame).expect_err(&format!("limit {limit}"));
            assert_eq!(err.to_string(), "disk full", "limit {limit}");
            assert_eq!(out.written, all.written[..limit], "limit {limit}");
        }
    }

    // Unix only: on Windows crossterm may draw through the console instead of the writer
    #[cfg(unix)]
    #[test]
    fn a_screen_that_cannot_be_written_ends_the_watch_without_an_error() {
        let mut gone = FailsAfter {
            left: 0,
            written: Vec::new(),
        };
        let ended = watch_args().tui_event_loop(&mut System::new(), &mut gone, Duration::MAX);
        assert!(ended.is_ok());
        assert!(gone.written.is_empty());
    }

    #[test]
    fn a_panic_runs_the_hook_that_was_there_before() {
        let before = std::panic::take_hook();
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        std::panic::set_hook(Box::new(move |_| flag.store(true, Ordering::SeqCst)));

        install_panic_hook();
        let caught = std::panic::catch_unwind(|| panic!("watch crashed"));
        // Drop ours, put back what the test run had
        let _ = std::panic::take_hook();
        std::panic::set_hook(before);

        assert!(caught.is_err());
        assert!(
            ran.load(Ordering::SeqCst),
            "the earlier hook runs after the terminal is restored"
        );
    }

    // ---- the stream ----

    struct FlushFails;

    impl Write for FlushFails {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::from(io::ErrorKind::BrokenPipe))
        }
    }

    struct CannotSerialize;

    impl Serialize for CannotSerialize {
        fn serialize<S: serde::Serializer>(&self, _: S) -> std::result::Result<S::Ok, S::Error> {
            Err(S::Error::custom("no frame today"))
        }
    }

    #[test]
    fn a_frame_says_what_a_name_left_out() {
        let zsh = Ancestor {
            pid: 7,
            name: "zsh".to_string(),
        };
        let frame = plain(&watch_args().render_frame_with(&[], &[zsh], screen(120), 40));
        let lines: Vec<&str> = frame.lines().collect();
        assert_eq!(
            lines[1], " ⚠ Left out: zsh (PID 7), which runs this proc",
            "{frame}"
        );
        let none = plain(&watch_args().render_frame(&[], screen(120), 40));
        assert!(!none.contains("Left out"), "{none}");
    }

    #[test]
    fn with_cmd_no_name_is_said_to_be_only_in_command_lines() {
        // --cmd already matches command lines, so there is nothing to suggest
        let watch = WatchCommand {
            target: vec!["proc-no-such-name-anywhere".to_string()],
            cmd: true,
            ..watch_args()
        };
        assert!(watch.names_in_command_lines().is_empty());
    }

    #[test]
    fn an_empty_frame_names_the_command_lines_a_name_is_in() {
        let watch = WatchCommand {
            command_line: vec![("serve".to_string(), 1)],
            ..watch_args()
        };
        let frame = plain(&watch.render_frame(&[], screen(120), 40));
        assert!(
            frame.ends_with(
                " No matching processes found.\n  1 process has 'serve' in its command line (add --cmd to match those)\n  Try: proc by serve --cmd to see them"
            ),
            "{frame}"
        );
        let none = plain(&watch_args().render_frame(&[], screen(120), 40));
        assert!(none.ends_with(" No matching processes found."), "{none}");
    }

    fn frame_of(processes: Vec<Process>) -> WatchJsonOutput {
        WatchJsonOutput {
            action: "watch",
            success: true,
            count: processes.len(),
            processes,
            left_out: Vec::new(),
        }
    }

    #[test]
    fn a_frame_is_one_json_line() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert!(write_json_frame(
            &mut out,
            &mut err,
            &frame_of(vec![process(7, "sh")])
        ));
        let text = String::from_utf8(out).unwrap();
        assert!(text.ends_with('\n'));
        assert_eq!(text.matches('\n').count(), 1);
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["action"], "watch");
        assert_eq!(json["count"], 1);
        assert_eq!(json["processes"][0]["pid"], 7);
        assert!(err.is_empty());
    }

    #[test]
    fn a_closed_reader_ends_the_stream() {
        let mut err = Vec::new();
        let mut closed = FailsAfter {
            left: 0,
            written: Vec::new(),
        };
        assert!(!write_json_frame(&mut closed, &mut err, &frame_of(vec![])));
        assert!(err.is_empty());
    }

    #[test]
    fn a_reader_that_goes_away_during_the_flush_ends_the_stream() {
        let mut err = Vec::new();
        assert!(!write_json_frame(
            &mut FlushFails,
            &mut err,
            &frame_of(vec![])
        ));
        assert!(err.is_empty());
    }

    #[test]
    fn a_frame_that_cannot_be_serialized_is_reported_and_skipped() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert!(
            write_json_frame(&mut out, &mut err, &CannotSerialize),
            "the stream goes on"
        );
        assert!(out.is_empty());
        assert_eq!(
            String::from_utf8(err).unwrap(),
            "JSON serialization error: no frame today\n"
        );
    }

    // ---- the sort keys ----

    #[test]
    fn each_sort_key_maps_to_the_shared_one() {
        assert!(matches!(SortKey::from(WatchSortKey::Cpu), SortKey::Cpu));
        assert!(matches!(SortKey::from(WatchSortKey::Mem), SortKey::Mem));
        assert!(matches!(SortKey::from(WatchSortKey::Pid), SortKey::Pid));
        assert!(matches!(SortKey::from(WatchSortKey::Name), SortKey::Name));
    }

    // ---- the frame ----

    #[test]
    fn the_header_names_the_target_sort_and_refresh() {
        let mut cmd = watch_args();
        cmd.target = vec![":3000".into()];
        cmd.interval = 0.5;
        for (sort, label) in [
            (WatchSortKey::Cpu, "Sort: CPU"),
            (WatchSortKey::Mem, "Sort: Memory"),
            (WatchSortKey::Pid, "Sort: PID"),
            (WatchSortKey::Name, "Sort: Name"),
        ] {
            cmd.sort = sort;
            let frame = cmd.render_frame(&[process(1, "a"), process(2, "b")], screen(120), 40);
            let first = frame.lines().next().unwrap();
            assert_eq!(
                first,
                format!(" Watching :3000 | 2 processes | {label} | Refresh: 0.5s | q to exit")
            );
        }
    }

    #[test]
    fn the_header_names_several_targets_as_typed_joined_by_spaces() {
        let mut cmd = watch_args();
        cmd.target = vec![":3000".into(), "node,python".into()];
        let frame = cmd.render_frame(&[process(1, "a")], screen(120), 40);
        assert!(
            frame.starts_with(" Watching :3000 node,python | 1 processes |"),
            "{frame}"
        );
    }

    #[test]
    fn with_no_target_the_header_says_all() {
        let frame = watch_args().render_frame(&[process(1, "a")], screen(120), 40);
        assert!(
            frame.starts_with(" Watching all | 1 processes |"),
            "{frame}"
        );
    }

    #[test]
    fn no_processes_says_so() {
        let frame = watch_args().render_frame(&[], screen(120), 40);
        assert_eq!(
            frame.lines().last().unwrap(),
            " No matching processes found."
        );
        assert!(frame.contains("0 processes"));
    }

    /// The time the frames are drawn at
    const NOW: u64 = 1_000_000;

    /// A screen `width` columns wide, at [`NOW`]
    fn screen(width: u16) -> Screen {
        Screen {
            width,
            now_secs: NOW,
        }
    }

    /// A process with every column known
    fn known_process() -> Process {
        let mut p = process(4242, "server");
        p.user = Some("zee".into());
        p.cwd = Some("/srv/app".into());
        p.command = Some("/usr/bin/server --port 80".into());
        p.parent_pid = Some(7);
        p.start_time = Some(NOW - 3725);
        p.cpu_percent = 12.34;
        p.memory_mb = 256.0;
        p
    }

    /// The cells of the lines of `frame` that hold `needle`, one list per line
    fn cells_with(frame: &str, needle: &str) -> Vec<Vec<String>> {
        lines_with(frame, needle)
            .iter()
            .map(|line| line.split_whitespace().map(str::to_string).collect())
            .collect()
    }

    #[test]
    fn watch_uses_the_same_columns() {
        let frame = plain(&watch_args().render_frame(&[known_process()], screen(120), 40));
        assert_eq!(
            cells_with(&frame, "PID"),
            [["PID", "NAME", "USER", "CPU%", "MEM", "UPTIME", "COMMAND"]]
        );
        assert_eq!(
            cells_with(&frame, "4242"),
            [[
                "4242",
                "server",
                "zee",
                "12.3",
                "256.0MB",
                "1h",
                "2m",
                "/usr/bin/server",
                "--port",
                "80"
            ]]
        );
        // ... and they are the shared table's, the one `list` prints
        let layout = TableLayout::from_flags(screen(120), false, false, false);
        let table = plain(&process_table(&[known_process()], &layout));
        assert!(frame.contains(&table), "{frame}\n{table}");
    }

    #[test]
    fn the_flags_choose_the_columns_as_they_do_for_list() {
        let mut verbose = watch_args();
        verbose.verbose = true;
        let frame = plain(&verbose.render_frame(&[known_process()], screen(30), 40));
        assert_eq!(
            cells_with(&frame, "PID")[0],
            ["PID", "NAME", "USER", "CPU%", "MEM", "UPTIME", "PPID", "STATUS", "DIR", "COMMAND"]
        );
        // nothing is cut, however narrow the screen
        assert!(frame.contains("/usr/bin/server --port 80"), "{frame}");

        let mut scoped = watch_args();
        scoped.in_dir = Some(".".into());
        let frame = plain(&scoped.render_frame(&[known_process()], screen(120), 40));
        assert_eq!(
            cells_with(&frame, "PID")[0],
            ["PID", "DIR", "NAME", "USER", "CPU%", "MEM", "UPTIME", "COMMAND"]
        );

        let mut wide = watch_args();
        wide.wide = true;
        let frame = plain(&wide.render_frame(&[known_process()], screen(30), 40));
        assert!(frame.contains("/usr/bin/server --port 80"), "{frame}");
    }

    #[test]
    fn a_narrow_terminal_cuts_the_command_and_keeps_the_uptime() {
        let frame = plain(&watch_args().render_frame(&[known_process()], screen(60), 40));
        let row = lines_with(&frame, "4242")[0];
        assert!(row.contains("1h 2m"), "{row:?}");
        assert!(row.contains('…'), "{row:?}");
        assert!(row.chars().count() <= 60, "{row:?}");
    }

    #[test]
    fn every_status_is_spelled_out() {
        for (status, word) in [
            (ProcessStatus::Running, "Running"),
            (ProcessStatus::Sleeping, "Sleeping"),
            (ProcessStatus::Stopped, "Stopped"),
            (ProcessStatus::Zombie, "Zombie"),
            (ProcessStatus::Dead, "Dead"),
            (ProcessStatus::Idle, "Idle"),
            (ProcessStatus::Starting, "Starting"),
            (ProcessStatus::DiskWait, "Disk wait"),
            (ProcessStatus::Unknown, "Unknown"),
        ] {
            let mut p = process(4242, "x");
            p.status = status;
            let mut cmd = watch_args();
            cmd.verbose = true;
            let frame = plain(&cmd.render_frame(&[p], screen(120), 40));
            assert!(lines_with(&frame, "4242")[0].contains(word), "{word}");
        }
    }

    #[test]
    fn rows_beyond_the_screen_are_counted_not_drawn() {
        let processes: Vec<Process> = (1..=10).map(|i| process(1000 + i, "p")).collect();
        // 12 rows leave 7 for processes
        let frame = plain(&watch_args().render_frame(&processes, screen(120), 12));
        assert_eq!(lines_with(&frame, " p ").len(), 7, "{frame}");
        assert_eq!(
            frame.lines().last().unwrap(),
            " ... and 3 more (use --limit to control)"
        );
        let all = plain(&watch_args().render_frame(&processes, screen(120), 40));
        assert!(!all.contains("more"), "{all}");
    }

    #[test]
    fn verbose_counts_the_processes_that_do_not_fit_as_well() {
        let processes: Vec<Process> = (1..=5).map(|i| process(1000 + i, "p")).collect();
        let mut cmd = watch_args();
        cmd.verbose = true;
        // 7 rows leave 2 for processes
        let frame = plain(&cmd.render_frame(&processes, screen(120), 7));
        assert_eq!(lines_with(&frame, " p ").len(), 2, "{frame}");
        assert!(
            frame.ends_with(" ... and 3 more (use --limit to control)"),
            "{frame:?}"
        );
    }

    // ---- what is collected ----

    #[test]
    fn collecting_applies_the_sort_and_the_limit() {
        let sys = refreshed_system();
        let mut cmd = watch_args();
        cmd.sort = WatchSortKey::Pid;
        let all = cmd.collect_processes(&sys);
        assert!(all.len() > 1);
        assert!(all.windows(2).all(|w| w[0].pid <= w[1].pid));
        assert!(all.iter().all(|p| p.pid != std::process::id()));

        cmd.limit = Some(1);
        let first = cmd.collect_processes(&sys);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].pid, all[0].pid);
    }

    #[test]
    fn a_port_holder_the_snapshot_has_not_seen_is_left_out() {
        let sys = refreshed_system();
        let mut cmd = watch_args();
        cmd.target = vec![":8080".into()];
        // This test's own process is in the snapshot; PID 4294967294 is not
        let me = std::process::id();
        let known = cmd.collect_with(&sys, |port| if port == 8080 { vec![me] } else { vec![] });
        assert_eq!(known.iter().map(|p| p.pid).collect::<Vec<_>>(), [me]);
        let unseen = cmd.collect_with(&sys, |_| vec![u32::MAX - 1]);
        assert!(unseen.is_empty(), "{unseen:?}");
        let nobody = cmd.collect_with(&sys, |_| Vec::new());
        assert!(nobody.is_empty(), "{nobody:?}");
    }

    #[test]
    fn a_name_finds_the_process_that_ran_watch_only_by_its_exact_name() {
        let sys = refreshed_system();
        let me = Pid::from_u32(std::process::id());
        let parent = sys.process(me).and_then(|p| p.parent()).expect("a parent");
        let parent_name = sys
            .process(parent)
            .unwrap()
            .name()
            .to_string_lossy()
            .into_owned();

        // its exact name: listed, marked as running proc
        let mut cmd = watch_args();
        cmd.target = vec![parent_name.clone()];
        let found = cmd.collect_processes(&sys);
        let listed = found.iter().find(|p| p.pid == parent.as_u32());
        assert!(listed.is_some_and(|p| p.runs_proc), "{found:?}");
        assert!(cmd.left_out(&sys).is_empty());

        // with --cmd, a match that may be proc's own arguments: left out, named
        cmd.cmd = true;
        let found = cmd.collect_processes(&sys);
        assert!(
            found.iter().all(|p| p.pid != parent.as_u32()),
            "listed the parent: {found:?}"
        );
        assert!(cmd.left_out(&sys).iter().any(|a| a.pid == parent.as_u32()));
    }

    #[test]
    fn every_holder_of_a_port_is_watched_once() {
        let sys = refreshed_system();
        let mut cmd = watch_args();
        cmd.target = vec![":8080".into(), std::process::id().to_string()];
        let me = std::process::id();
        // the parent is in the snapshot too; this process is named twice (as a
        // holder of the port and by PID) and listed once
        let parent = sys
            .process(Pid::from_u32(me))
            .and_then(|p| p.parent())
            .expect("a parent")
            .as_u32();
        let found = cmd.collect_with(&sys, |_| vec![me, parent]);
        let mut pids: Vec<u32> = found.iter().map(|p| p.pid).collect();
        pids.sort_unstable();
        let mut expected = vec![me, parent];
        expected.sort_unstable();
        assert_eq!(pids, expected);
    }

    #[test]
    fn the_holders_of_a_port_are_the_processes_listening_on_it() {
        let me = std::process::id();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // Not exactly [me]: macOS has no SOCK_CLOEXEC, so a child another test
        // spawns between the socket's creation and its close-on-exec flag
        // inherits it and holds the port too (seen in CI: two extra holders)
        assert!(port_holders(port).contains(&me));
        drop(listener);
        assert!(!port_holders(port).contains(&me));
    }
}
