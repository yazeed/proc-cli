//! The process table: the one set of columns and the one width policy that
//! `list`, `by`, `in`, `for`, `orphans`, `stuck`, `watch` and the previews
//! before a kill or a stop all print.
//!
//! ```text
//!  PID    NAME     USER  CPU%  MEM     UPTIME  COMMAND
//!  39158  python3  zee    0.0  26.8MB  13s     python3 -m http.server 0 --bind 1…
//! ```
//!
//! - The columns are `PID NAME USER CPU% MEM UPTIME COMMAND`. With `-v` the
//!   table keeps them and adds `PPID STATUS DIR`. When a command is scoped to a
//!   directory (`--in`), `DIR` is the second column, with or without `-v`.
//! - COMMAND takes the width the other columns leave and is cut with `…`; every
//!   other column keeps its natural width, so UPTIME never depends on how wide
//!   the terminal is. The one exception is DIR (only under `--in`, where every
//!   row has much the same directory): it is cut, in the middle, before it
//!   takes more than a third of the line or leaves COMMAND under 24 columns.
//!   With `-v` and `--wide` nothing is cut, and in the preview before a signal
//!   (the thing a person approves) COMMAND is printed whole.
//! - A value that is not known shows `-`, never an empty cell.

use crate::core::{Process, ProcessStatus};
use crate::ui::card::known;
use crate::ui::{format_duration, format_memory, truncate_path, truncate_string};
use comfy_table::presets::NOTHING;
use comfy_table::{Attribute, Cell, CellAlignment, Color, ContentArrangement, Table};
use unicode_width::UnicodeWidthStr;

/// Shown for a value that is not known
const UNKNOWN: &str = "-";
/// Columns comfy-table adds around every column (one space on each side)
const PADDING: usize = 2;
/// A line exactly as wide as the terminal wraps in some emulators
const SLACK: usize = 1;
/// COMMAND keeps at least this much, however narrow the terminal is
const MIN_COMMAND: usize = 12;
/// DIR yields to COMMAND: it is cut before COMMAND gets less than this...
const COMMAND_BEFORE_DIR: usize = 24;
/// ... and it never takes more than this share of the line (a third of it),
/// because under `--in` every row has much the same directory
const DIR_SHARE: usize = 3;
/// DIR is never cut below this
const MIN_DIR: usize = 12;

/// The screen a table is drawn for
#[derive(Clone, Copy, Debug)]
pub struct Screen {
    /// Terminal width in columns
    pub width: u16,
    /// The time now, in Unix seconds: UPTIME is this minus the start time
    pub now_secs: u64,
}

/// How a table is laid out
#[derive(Clone, Copy, Debug)]
pub struct TableLayout {
    /// The screen it is drawn for
    pub screen: Screen,
    /// Add `PPID STATUS DIR` (`-v`)
    pub verbose: bool,
    /// Put `DIR` second: the command is scoped to a directory (`--in`)
    pub dir_second: bool,
    /// Cut DIR to fit; false prints it whole
    pub cut_dir: bool,
    /// Cut COMMAND to the width left; false prints it whole
    pub cut_command: bool,
}

impl TableLayout {
    /// The layout the flags ask for: `-v` adds columns and cuts nothing,
    /// `--wide` cuts nothing, and `--in` puts DIR second. Every command that
    /// prints the table builds its layout here, so they cannot drift apart.
    pub fn from_flags(screen: Screen, verbose: bool, wide: bool, scoped_to_dir: bool) -> Self {
        let cuts_nothing = verbose || wide;
        TableLayout {
            screen,
            verbose,
            dir_second: scoped_to_dir,
            cut_dir: !cuts_nothing,
            cut_command: !cuts_nothing,
        }
    }

    /// The same layout with the command lines printed whole: for a list a
    /// person is about to approve
    pub fn with_whole_command(mut self) -> Self {
        self.cut_command = false;
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Column {
    Pid,
    Dir,
    Name,
    User,
    Cpu,
    Mem,
    Uptime,
    Ppid,
    Status,
    Command,
}

impl Column {
    fn header(self) -> &'static str {
        match self {
            Column::Pid => "PID",
            Column::Dir => "DIR",
            Column::Name => "NAME",
            Column::User => "USER",
            Column::Cpu => "CPU%",
            Column::Mem => "MEM",
            Column::Uptime => "UPTIME",
            Column::Ppid => "PPID",
            Column::Status => "STATUS",
            Column::Command => "COMMAND",
        }
    }

    /// Numbers that grow to the right line up on their right edge
    fn alignment(self) -> CellAlignment {
        match self {
            Column::Cpu | Column::Mem | Column::Uptime => CellAlignment::Right,
            _ => CellAlignment::Left,
        }
    }

    fn colour(self, process: &Process) -> Option<Color> {
        match self {
            Column::Pid => Some(Color::Cyan),
            Column::Dir | Column::Command => Some(Color::DarkGrey),
            Column::Name => Some(Color::White),
            Column::Status => Some(match process.status {
                ProcessStatus::Running => Color::Green,
                ProcessStatus::Sleeping => Color::Blue,
                ProcessStatus::Stopped => Color::Yellow,
                ProcessStatus::Zombie => Color::Red,
                _ => Color::White,
            }),
            _ => None,
        }
    }

    /// The cell's text, whole. DIR and COMMAND are cut later, once the other
    /// widths are known.
    fn text(self, process: &Process, layout: &TableLayout) -> String {
        match self {
            Column::Pid => process.pid.to_string(),
            Column::Dir => known(&process.cwd).unwrap_or(UNKNOWN).to_string(),
            Column::Name => process.name.clone(),
            Column::User => known(&process.user).unwrap_or(UNKNOWN).to_string(),
            Column::Cpu => format!("{:.1}", process.cpu_percent),
            Column::Mem => format_memory(process.memory_mb),
            Column::Uptime => process.start_time.map_or_else(
                || UNKNOWN.to_string(),
                |start| format_duration(layout.screen.now_secs.saturating_sub(start)),
            ),
            Column::Ppid => process
                .parent_pid
                .map_or_else(|| UNKNOWN.to_string(), |pid| pid.to_string()),
            Column::Status => format!("{:?}", process.status),
            Column::Command => known(&process.command).unwrap_or(UNKNOWN).to_string(),
        }
    }
}

/// The columns, left to right. COMMAND is last: it is the one that is long.
fn columns(layout: &TableLayout) -> Vec<Column> {
    let mut columns = vec![Column::Pid];
    if layout.dir_second {
        columns.push(Column::Dir);
    }
    columns.extend([
        Column::Name,
        Column::User,
        Column::Cpu,
        Column::Mem,
        Column::Uptime,
    ]);
    if layout.verbose {
        columns.push(Column::Ppid);
        columns.push(Column::Status);
        if !layout.dir_second {
            columns.push(Column::Dir);
        }
    }
    columns.push(Column::Command);
    columns
}

/// The table for `processes`: a header line and a line per process (no headline
/// and no trailing blank line), as the text the terminal gets.
pub fn process_table(processes: &[Process], layout: &TableLayout) -> String {
    let columns = columns(layout);
    let mut cells: Vec<Vec<String>> = processes
        .iter()
        .map(|process| {
            columns
                .iter()
                .map(|column| column.text(process, layout))
                .collect()
        })
        .collect();
    let screen = layout.screen.width as usize;
    let last = columns.len() - 1;
    // A column is as wide as its widest cell or its header, plus the padding
    let width_of = |cells: &[Vec<String>], at: usize| {
        let widest = cells.iter().map(|row| row[at].width()).max().unwrap_or(0);
        widest.max(columns[at].header().width()) + PADDING
    };

    // DIR keeps its natural width unless that would squeeze COMMAND, or take
    // more than a third of the line
    if layout.cut_dir {
        if let Some(at) = columns.iter().position(|column| *column == Column::Dir) {
            let others: usize = (0..last)
                .filter(|other| *other != at)
                .map(|other| width_of(&cells, other))
                .sum();
            let room = (screen / DIR_SHARE)
                .min(
                    screen
                        .saturating_sub(SLACK + others + 2 * PADDING)
                        .saturating_sub(COMMAND_BEFORE_DIR),
                )
                .max(MIN_DIR);
            for row in &mut cells {
                row[at] = truncate_path(&row[at], room);
            }
        }
    }

    // COMMAND gets what is left of the line
    if layout.cut_command {
        let used: usize = (0..last).map(|at| width_of(&cells, at)).sum();
        let room = screen
            .saturating_sub(used + PADDING + SLACK)
            .max(MIN_COMMAND);
        for row in &mut cells {
            row[last] = truncate_string(&row[last], room);
        }
    }

    let mut table = Table::new();
    // Disabled never reflows a cell, so one process is always one line (unless
    // it is printed whole). The cuts above are what fit the terminal: a wrapped
    // cell breaks the column alignment that makes a table readable.
    table
        .load_preset(NOTHING)
        .set_content_arrangement(ContentArrangement::Disabled);
    table.set_header(columns.iter().map(|column| {
        Cell::new(column.header())
            .fg(Color::Blue)
            .add_attribute(Attribute::Bold)
            .set_alignment(column.alignment())
    }));
    for (process, row) in processes.iter().zip(cells) {
        table.add_row(columns.iter().zip(row).map(|(column, text)| {
            let cell = Cell::new(text).set_alignment(column.alignment());
            match column.colour(process) {
                Some(colour) => cell.fg(colour),
                None => cell,
            }
        }));
    }
    table.to_string()
}
