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
//!
//! `proc ports` prints the same table for listeners ([`port_table`]):
//!
//! ```text
//!  PORT  PROTO  ADDRESS    EXPOSURE  PID    NAME     UPTIME  COMMAND
//!  4000  TCP    127.0.0.1  local     39158  python3  13s     python3 -m http.server 4000
//! ```
//!
//! The columns are `PORT PROTO ADDRESS EXPOSURE PID NAME UPTIME COMMAND`; with
//! `-v` it adds `USER MEM` and the executable's path (`PATH`) before COMMAND,
//! and cuts nothing. The process cells of a holder that ended since its port
//! was listed show `-`; its port cells and the name its socket gave stay.

use crate::core::{Exposure, PortInfo, Process, ProcessStatus};
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
                ProcessStatus::Sleeping | ProcessStatus::Idle => Color::Blue,
                ProcessStatus::Stopped | ProcessStatus::DiskWait => Color::Yellow,
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
            // what the OS did not give is `-`, never a 0 that reads as a measurement
            Column::Cpu if process.read_by_os() => format!("{:.1}", process.cpu_percent),
            Column::Mem if process.read_by_os() => format_memory(process.memory_mb),
            Column::Cpu | Column::Mem => UNKNOWN.to_string(),
            Column::Uptime => process.start_time.map_or_else(
                || UNKNOWN.to_string(),
                |start| format_duration(layout.screen.now_secs.saturating_sub(start)),
            ),
            Column::Ppid => process
                .parent_pid
                .map_or_else(|| UNKNOWN.to_string(), |pid| pid.to_string()),
            Column::Status => {
                // `Running`, `Disk wait`: the card's word, capitalised
                let word = process.status.word();
                format!("{}{}", word[..1].to_uppercase(), &word[1..])
            }
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
    let cells: Vec<Vec<String>> = processes
        .iter()
        .map(|process| {
            columns
                .iter()
                .map(|column| column.text(process, layout))
                .collect()
        })
        .collect();
    let heads: Vec<Head> = columns
        .iter()
        .map(|column| Head {
            title: column.header(),
            alignment: column.alignment(),
            is_dir: *column == Column::Dir,
        })
        .collect();
    let colours: Vec<Vec<Option<Color>>> = processes
        .iter()
        .map(|process| {
            columns
                .iter()
                .map(|column| column.colour(process))
                .collect()
        })
        .collect();
    lay_out(&heads, cells, &colours, layout)
}

/// What a column needs to be drawn, whatever the rows are
struct Head {
    title: &'static str,
    alignment: CellAlignment,
    /// The column is DIR: it is cut in the middle, to a share of the line
    is_dir: bool,
}

/// Cut `cells` to the terminal and draw them under `heads`. The last column
/// is COMMAND; the rows are the cells with the colour of each.
fn lay_out(
    heads: &[Head],
    mut cells: Vec<Vec<String>>,
    colours: &[Vec<Option<Color>>],
    layout: &TableLayout,
) -> String {
    let screen = layout.screen.width as usize;
    let last = heads.len() - 1;
    // A column is as wide as its widest cell or its header, plus the padding
    let width_of = |cells: &[Vec<String>], at: usize| {
        let widest = cells.iter().map(|row| row[at].width()).max().unwrap_or(0);
        widest.max(heads[at].title.width()) + PADDING
    };

    // DIR keeps its natural width unless that would squeeze COMMAND, or take
    // more than a third of the line
    if layout.cut_dir {
        if let Some(at) = heads.iter().position(|head| head.is_dir) {
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
    follow_colour(
        &mut table,
        colored::control::SHOULD_COLORIZE.should_colorize(),
    );
    table.set_header(heads.iter().map(|head| {
        Cell::new(head.title)
            .fg(Color::Blue)
            .add_attribute(Attribute::Bold)
            .set_alignment(head.alignment)
    }));
    for (row, colours) in cells.into_iter().zip(colours) {
        table.add_row(
            heads
                .iter()
                .zip(row)
                .zip(colours)
                .map(|((head, text), colour)| {
                    let cell = Cell::new(text).set_alignment(head.alignment);
                    match colour {
                        Some(colour) => cell.fg(*colour),
                        None => cell,
                    }
                }),
        );
    }
    table.to_string()
}

/// Colour as the rest of proc's output does, by proc's own decision
/// (`NO_COLOR`, `CLICOLOR_FORCE`, a terminal), not by comfy-table's look at
/// stdout, which ignores `NO_COLOR` and says yes inside a build sandbox's
/// pseudo-terminal
fn follow_colour(table: &mut Table, colour: bool) {
    if colour {
        table.enforce_styling();
    } else {
        table.force_no_tty();
    }
}

/// A listener with the process that holds it, for one row of the ports table.
/// `process` is `None` when the holder ended since its port was listed.
#[derive(Clone, Copy, Debug)]
pub struct PortRow<'a> {
    /// The listening socket
    pub port: &'a PortInfo,
    /// Its holder, when it still runs
    pub process: Option<&'a Process>,
}

#[derive(Clone, Copy)]
enum PortColumn {
    Port,
    Proto,
    Address,
    Exposure,
    Pid,
    Name,
    User,
    Mem,
    Uptime,
    Path,
    Command,
}

impl PortColumn {
    fn header(self) -> &'static str {
        match self {
            PortColumn::Port => "PORT",
            PortColumn::Proto => "PROTO",
            PortColumn::Address => "ADDRESS",
            PortColumn::Exposure => "EXPOSURE",
            PortColumn::Pid => "PID",
            PortColumn::Name => "NAME",
            PortColumn::User => "USER",
            PortColumn::Mem => "MEM",
            PortColumn::Uptime => "UPTIME",
            PortColumn::Path => "PATH",
            PortColumn::Command => "COMMAND",
        }
    }

    /// The numbers that grow to the right line up on their right edge, as in
    /// the process table
    fn alignment(self) -> CellAlignment {
        match self {
            PortColumn::Mem | PortColumn::Uptime => CellAlignment::Right,
            _ => CellAlignment::Left,
        }
    }

    fn colour(self) -> Option<Color> {
        match self {
            PortColumn::Port | PortColumn::Pid => Some(Color::Cyan),
            PortColumn::Name | PortColumn::Proto => Some(Color::White),
            PortColumn::Address | PortColumn::Path | PortColumn::Command => Some(Color::DarkGrey),
            PortColumn::Exposure => None,
            PortColumn::User | PortColumn::Mem | PortColumn::Uptime => None,
        }
    }

    fn text(self, row: &PortRow, layout: &TableLayout) -> String {
        let unknown = || UNKNOWN.to_string();
        match self {
            PortColumn::Port => row.port.port.to_string(),
            PortColumn::Proto => format!("{:?}", row.port.protocol).to_uppercase(),
            PortColumn::Address => row.port.address.clone().unwrap_or_else(unknown),
            PortColumn::Exposure => match row.port.exposure() {
                Exposure::Local => "local",
                Exposure::Exposed => "exposed",
            }
            .to_string(),
            PortColumn::Pid => row.port.pid.to_string(),
            // the one name every view shows; a holder that has ended keeps the
            // name its socket gave
            PortColumn::Name => row.port.process_name.clone(),
            PortColumn::User => row
                .process
                .and_then(|p| known(&p.user))
                .map_or_else(unknown, str::to_string),
            PortColumn::Mem => row
                .process
                .filter(|p| p.read_by_os())
                .map_or_else(unknown, |p| format_memory(p.memory_mb)),
            PortColumn::Uptime => row
                .process
                .and_then(|p| p.start_time)
                .map_or_else(unknown, |start| {
                    format_duration(layout.screen.now_secs.saturating_sub(start))
                }),
            PortColumn::Path => row
                .process
                .and_then(|p| known(&p.exe_path))
                .map_or_else(unknown, str::to_string),
            PortColumn::Command => row
                .process
                .and_then(|p| known(&p.command))
                .map_or_else(unknown, str::to_string),
        }
    }
}

/// The ports table's columns, left to right; COMMAND is last
fn port_columns(layout: &TableLayout) -> Vec<PortColumn> {
    let mut columns = vec![
        PortColumn::Port,
        PortColumn::Proto,
        PortColumn::Address,
        PortColumn::Exposure,
        PortColumn::Pid,
        PortColumn::Name,
    ];
    if layout.verbose {
        columns.push(PortColumn::User);
        columns.push(PortColumn::Mem);
    }
    columns.push(PortColumn::Uptime);
    if layout.verbose {
        columns.push(PortColumn::Path);
    }
    columns.push(PortColumn::Command);
    columns
}

/// The table for `rows`, the listeners `proc ports` prints: a header line and
/// a line per listener, on the same width policy as [`process_table`]
pub fn port_table(rows: &[PortRow], layout: &TableLayout) -> String {
    let columns = port_columns(layout);
    let heads: Vec<Head> = columns
        .iter()
        .map(|column| Head {
            title: column.header(),
            alignment: column.alignment(),
            is_dir: false,
        })
        .collect();
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            columns
                .iter()
                .map(|column| column.text(row, layout))
                .collect()
        })
        .collect();
    let colours: Vec<Vec<Option<Color>>> = rows
        .iter()
        .map(|_| columns.iter().map(|column| column.colour()).collect())
        .collect();
    lay_out(&heads, cells, &colours, layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Protocol;

    const NOW: u64 = 1_000_000;

    fn screen(width: u16) -> Screen {
        Screen {
            width,
            now_secs: NOW,
        }
    }

    fn layout(width: u16, verbose: bool) -> TableLayout {
        TableLayout::from_flags(screen(width), verbose, false, false)
    }

    /// The text of a table without colour codes, which comfy-table adds only
    /// when stdout is a terminal
    fn plain(table: &str) -> String {
        let mut out = String::new();
        let mut chars = table.chars();
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
    fn the_table_colours_by_procs_decision_not_by_stdout() {
        for colour in [true, false] {
            let mut table = Table::new();
            follow_colour(&mut table, colour);
            assert_eq!(table.should_style(), colour);
        }
        assert_eq!(plain("\x1b[34;1mPID\x1b[0m 1"), "PID 1");
    }

    fn listener(port: u16, address: Option<&str>, pid: u32, name: &str) -> PortInfo {
        PortInfo {
            port,
            protocol: Protocol::Tcp,
            pid,
            process_name: name.to_string(),
            address: address.map(String::from),
        }
    }

    fn holder(pid: u32, command: &str) -> Process {
        Process {
            pid,
            name: "node".to_string(),
            exe_path: Some("/opt/node/bin/node".to_string()),
            cwd: None,
            command: Some(command.to_string()),
            argv0: None,
            raw: None,
            cpu_percent: 0.0,
            memory_mb: 26.8,
            status: ProcessStatus::Sleeping,
            user: Some("zee".to_string()),
            parent_pid: None,
            start_time: Some(NOW - 13),
            uptime_secs: Some(13),
            parent_name: None,
            cpu_time_secs: None,
            open_files: None,
            child_count: None,
        }
    }

    fn cells(line: &str) -> Vec<&str> {
        line.split_whitespace().collect()
    }

    #[test]
    fn a_listener_shows_its_port_its_exposure_and_what_its_holder_is() {
        let local = listener(4000, Some("127.0.0.1"), 7, "node");
        let open = listener(80, Some("0.0.0.0"), 8, "node");
        let node = holder(7, "node server.js");
        let rows = [
            PortRow {
                port: &local,
                process: Some(&node),
            },
            PortRow {
                port: &open,
                process: Some(&node),
            },
        ];
        let table = plain(&port_table(&rows, &layout(120, false)));
        let lines: Vec<&str> = table.lines().collect();
        assert_eq!(
            cells(lines[0]),
            ["PORT", "PROTO", "ADDRESS", "EXPOSURE", "PID", "NAME", "UPTIME", "COMMAND"]
        );
        assert_eq!(
            cells(lines[1]),
            [
                "4000",
                "TCP",
                "127.0.0.1",
                "local",
                "7",
                "node",
                "13s",
                "node",
                "server.js"
            ]
        );
        assert_eq!(cells(lines[2])[3], "exposed");
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn what_is_not_known_shows_a_dash_and_a_gone_holder_keeps_its_port_cells() {
        // no address: not known, so treated as reachable
        let unbound = listener(4000, None, 7, "stale-name");
        let gone = PortRow {
            port: &unbound,
            process: None,
        };
        let table = plain(&port_table(&[gone], &layout(120, true)));
        let row = cells(table.lines().nth(1).unwrap());
        // PORT PROTO ADDRESS EXPOSURE PID NAME USER MEM UPTIME PATH COMMAND
        assert_eq!(
            row,
            [
                "4000",
                "TCP",
                "-",
                "exposed",
                "7",
                "stale-name",
                "-",
                "-",
                "-",
                "-",
                "-"
            ]
        );

        // a holder that does not say: empty text is as unknown as none
        let mut quiet = holder(7, "");
        quiet.user = Some(String::new());
        quiet.exe_path = None;
        quiet.start_time = None;
        quiet.command = None;
        // memory the OS refused reads 0: unknown (macOS, Windows)
        quiet.memory_mb = 0.0;
        let table = plain(&port_table(
            &[PortRow {
                port: &unbound,
                process: Some(&quiet),
            }],
            &layout(120, true),
        ));
        let row = cells(table.lines().nth(1).unwrap());
        assert_eq!(row[6], "-", "{row:?}");
        // (a 0 is a reading where the OS does not refuse, as on Linux)
        let refused = cfg!(any(target_os = "macos", windows));
        assert_eq!(row[7], if refused { "-" } else { "0KB" }, "{row:?}");
        assert_eq!(&row[8..], ["-", "-", "-"], "{row:?}");
    }

    #[test]
    fn the_command_takes_the_width_left_and_is_cut_with_an_ellipsis() {
        let port = listener(4000, Some("127.0.0.1"), 7, "node");
        let long = "node ".to_string() + &"x".repeat(200);
        let node = holder(7, &long);
        let rows = [PortRow {
            port: &port,
            process: Some(&node),
        }];
        let table = plain(&port_table(&rows, &layout(80, false)));
        let row = table.lines().nth(1).unwrap();
        assert!(row.ends_with("… ") || row.ends_with('…'), "{row}");
        assert!(row.trim_end().width() < 80, "{row}");
        // the other columns keep their whole value
        assert_eq!(
            &cells(row)[..7],
            ["4000", "TCP", "127.0.0.1", "local", "7", "node", "13s"]
        );

        // wide characters count as two columns each
        let wide = holder(7, &"漢".repeat(100));
        let rows = [PortRow {
            port: &port,
            process: Some(&wide),
        }];
        let table = plain(&port_table(&rows, &layout(80, false)));
        for line in table.lines() {
            assert!(line.trim_end().width() < 80, "{line}");
        }
        assert!(table.contains('…'), "{table}");
    }

    #[test]
    fn verbose_adds_user_memory_and_path_and_cuts_nothing() {
        let port = listener(4000, Some("127.0.0.1"), 7, "node");
        let long = "node ".to_string() + &"x".repeat(200);
        let node = holder(7, &long);
        let rows = [PortRow {
            port: &port,
            process: Some(&node),
        }];
        let table = plain(&port_table(&rows, &layout(60, true)));
        let lines: Vec<&str> = table.lines().collect();
        assert_eq!(
            cells(lines[0]),
            [
                "PORT", "PROTO", "ADDRESS", "EXPOSURE", "PID", "NAME", "USER", "MEM", "UPTIME",
                "PATH", "COMMAND"
            ]
        );
        assert!(!table.contains('…'), "{table}");
        let row = cells(lines[1]);
        assert_eq!(&row[6..10], ["zee", "26.8MB", "13s", "/opt/node/bin/node"]);
        assert_eq!(row.last(), Some(&"x".repeat(200).as_str()));
    }

    #[test]
    fn no_listeners_is_a_header_alone() {
        let table = plain(&port_table(&[], &layout(120, false)));
        assert_eq!(table.lines().count(), 1, "{table}");
        assert!(table.contains("EXPOSURE"), "{table}");
    }
}
