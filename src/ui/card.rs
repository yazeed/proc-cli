//! The card: one view of a process that `on`, `info`, `for` and `why` all print.
//!
//! ```text
//!   Process:     python3 (PID 39158)
//!   Command:     python3 -m http.server 0 --bind 127.0.0.1
//!   Parent:      zsh (PID 38120)
//!   Status:      running
//!   Resources:   0.0% CPU, 26.8MB, 0.4s CPU time, 9 open files
//!   Started:     21:14:03 (up 13s)
//!   Children:    none
//!   Listening:   :53581 TCP on 127.0.0.1 (this machine only)
//!   Connections: 3 established
//!                127.0.0.1:53781 → python3 (PID 43581)
//! ```
//!
//! A line is left out when its value is unknown (another user's process on
//! macOS gives no open files or directory), never printed empty. Unless
//! `verbose`, every line fits `width` columns: the value is cut with `…`, the
//! label never is.

use crate::core::connections::{Connections, Peer};
use crate::core::{PortInfo, Process, ProcessStatus, Protocol};
use crate::ui::{colorize_status, format_duration, format_memory, truncate_path, truncate_string};
use std::io::{self, Write};
use unicode_width::UnicodeWidthStr;

/// Spaces before every label
const INDENT: usize = 2;
/// The label column: the longest label (`Connections:`) and one space
const LABEL_COLUMN: usize = 13;
/// Where values start
const VALUE_COLUMN: usize = INDENT + LABEL_COLUMN;
/// Peers listed per port unless `verbose`
const PEERS_SHOWN: usize = 5;

/// A listening port of the process, with the connections to it when known
pub struct CardPort<'a> {
    /// The port
    pub port: &'a PortInfo,
    /// Its connections; `None` where they are not known (not asked for, or the
    /// OS gives none)
    pub connections: Option<&'a Connections>,
}

/// How the card is drawn
pub struct CardOptions {
    /// Print every value whole and every peer
    pub verbose: bool,
    /// Terminal width in columns
    pub width: u16,
    /// The time now, in Unix seconds: the "today" check and the uptime read it
    pub now_secs: u64,
}

/// Writes the card for `process`, its listening `ports` and their connections
pub fn write_card(
    out: &mut impl Write,
    process: &Process,
    ports: &[CardPort],
    opts: &CardOptions,
) -> io::Result<()> {
    let card = Card { opts };
    card.put(
        out,
        "Process:",
        &format!("{} (PID {})", process.name, process.pid),
        cut_keeping_pid,
    )?;
    if let Some(command) = known(&process.command) {
        card.put(out, "Command:", command, truncate_string)?;
    }
    if let Some(cwd) = known(&process.cwd) {
        card.put(out, "Directory:", cwd, truncate_path)?;
    }
    if let Some(path) = known(&process.exe_path) {
        card.put(out, "Path:", path, truncate_path)?;
    }
    if let Some(user) = known(&process.user) {
        card.put(out, "User:", user, truncate_string)?;
    }
    if let Some(pid) = process.parent_pid {
        let parent = match known(&process.parent_name) {
            Some(name) => format!("{name} (PID {pid})"),
            None => format!("PID {pid}"),
        };
        card.put(out, "Parent:", &parent, cut_keeping_pid)?;
    }
    let status = status_word(process.status);
    card.put_painted(out, "Status:", status, truncate_string, |shown| {
        colorize_status(&process.status, shown).to_string()
    })?;
    card.put(out, "Resources:", &resources(process), truncate_string)?;
    if let Some(start) = process.start_time {
        card.put(
            out,
            "Started:",
            &started(start, opts.now_secs),
            truncate_string,
        )?;
    }
    match process.child_count {
        None => {}
        Some(0) => card.put(out, "Children:", "none", truncate_string)?,
        Some(count) => card.put(out, "Children:", &count.to_string(), truncate_string)?,
    }
    card.write_listening(out, ports)?;
    card.write_connections(out, ports)
}

/// Cuts `name (PID n)` in the name, so the PID stays whole; when even the PID
/// does not fit, the whole value is cut
fn cut_keeping_pid(value: &str, room: usize) -> String {
    if value.width() <= room {
        return value.to_string();
    }
    if let Some(at) = value.rfind(" (PID ") {
        let (name, pid) = value.split_at(at);
        if pid.width() < room {
            return truncate_string(name, room - pid.width()) + pid;
        }
    }
    truncate_string(value, room)
}

/// A text field that is known: present and not empty
fn known(field: &Option<String>) -> Option<&str> {
    field.as_deref().filter(|text| !text.is_empty())
}

/// The status as the JSON and the filters spell it
fn status_word(status: ProcessStatus) -> &'static str {
    match status {
        ProcessStatus::Running => "running",
        ProcessStatus::Sleeping => "sleeping",
        ProcessStatus::Stopped => "stopped",
        ProcessStatus::Zombie => "zombie",
        ProcessStatus::Dead => "dead",
        ProcessStatus::Unknown => "unknown",
    }
}

/// CPU, memory, CPU time and open files; the last two when the OS gave them
fn resources(process: &Process) -> String {
    let mut parts = vec![
        format!("{:.1}% CPU", process.cpu_percent),
        format_memory(process.memory_mb),
    ];
    if let Some(secs) = process.cpu_time_secs {
        parts.push(format!("{} CPU time", format_duration(secs)));
    }
    match process.open_files {
        None => {}
        Some(1) => parts.push("1 open file".to_string()),
        Some(count) => parts.push(format!("{count} open files")),
    }
    parts.join(", ")
}

/// `21:14:03 (up 13s)`, with the date in front when it was not today
fn started(start: u64, now: u64) -> String {
    let (_, month, day, hour, minute, second) = civil(start as i64, local_offset(start));
    let clock = format!("{hour:02}:{minute:02}:{second:02}");
    let when = if same_local_day(start, now) {
        clock
    } else {
        format!("{} {day} {clock}", MONTHS[month as usize - 1])
    };
    format!("{when} (up {})", format_duration(now.saturating_sub(start)))
}

/// Whether two Unix times are on the same local calendar day
fn same_local_day(a: u64, b: u64) -> bool {
    let day = |secs: u64| (secs as i64 + local_offset(secs)).div_euclid(86_400);
    day(a) == day(b)
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The local time zone's offset from UTC in seconds at `secs`, with the zone's
/// rules (daylight saving) as they were at that moment. Unix asks the C library,
/// as `ps` does; Windows asks kernel32. Where the system can't say it is 0 (UTC).
#[cfg(unix)]
fn local_offset(secs: u64) -> i64 {
    let time = secs as libc::time_t;
    // SAFETY: localtime_r writes only into `tm`, which is zeroed and ours. A
    // failure leaves it zeroed, which reads as UTC.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&time, &mut tm);
        tm.tm_gmtoff as i64
    }
}

/// A Windows file time: 100 ns ticks since 1601, in two halves
#[cfg(windows)]
#[repr(C)]
struct FileTime {
    low: u32,
    high: u32,
}

/// A Windows calendar time
#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SystemTime {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    milliseconds: u16,
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn FileTimeToSystemTime(file_time: *const FileTime, system_time: *mut SystemTime) -> i32;
    fn SystemTimeToTzSpecificLocalTime(
        zone: *const std::ffi::c_void,
        universal: *const SystemTime,
        local: *mut SystemTime,
    ) -> i32;
}

/// See the unix version. The two calendar times are compared as clock readings
/// within the same few days, so the difference is the offset.
#[cfg(windows)]
fn local_offset(secs: u64) -> i64 {
    let ticks = (secs + 11_644_473_600) * 10_000_000;
    let file_time = FileTime {
        low: ticks as u32,
        high: (ticks >> 32) as u32,
    };
    let mut universal = SystemTime::default();
    // SAFETY: each call reads one valid struct of ours and writes only into
    // another of ours, all `repr(C)` with the layout the API documents. A NULL
    // zone means the current time zone. `local` starts as a copy of
    // `universal`, so a call that fails leaves the two equal: offset 0, UTC
    unsafe { FileTimeToSystemTime(&file_time, &mut universal) };
    let mut local = universal;
    unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &universal, &mut local) };
    let seconds = |t: &SystemTime| {
        let (year, month, day) = (i64::from(t.year), i64::from(t.month), i64::from(t.day));
        (days_from_civil(year, month, day) * 86_400)
            + i64::from(t.hour) * 3_600
            + i64::from(t.minute) * 60
            + i64::from(t.second)
    };
    seconds(&local) - seconds(&universal)
}

/// Days since 1970-01-01 of a calendar date (Hinnant's algorithm, the
/// inverse of `civil`)
#[cfg(windows)]
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Year, month (1-12), day, hour, minute and second of Unix time `secs` shifted
/// by `offset` seconds (days from civil, Howard Hinnant's algorithm)
fn civil(secs: i64, offset: i64) -> (i64, u32, u32, u32, u32, u32) {
    let local = secs + offset;
    let days = local.div_euclid(86_400);
    let rest = local.rem_euclid(86_400) as u32;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day, rest / 3_600, rest % 3_600 / 60, rest % 60)
}

/// `:53581 TCP on 127.0.0.1 (this machine only)`
fn listening(port: &PortInfo) -> String {
    let protocol = match port.protocol {
        Protocol::Tcp => "TCP",
        Protocol::Udp => "UDP",
    };
    match port.address.as_deref() {
        Some(address) => format!(
            ":{} {protocol} on {address} ({})",
            port.port,
            port.exposure_words()
        ),
        None => format!(":{} {protocol} ({})", port.port, port.exposure_words()),
    }
}

/// `3 established, 1 close_wait`: `established` first, then the rest by name
fn state_counts(connections: &Connections) -> String {
    let established = connections
        .by_state
        .get_key_value("established")
        .into_iter()
        .chain(
            connections
                .by_state
                .iter()
                .filter(|(state, _)| *state != "established"),
        );
    let parts: Vec<String> = established
        .map(|(state, count)| format!("{count} {state}"))
        .collect();
    if parts.is_empty() {
        format!("{} in all", connections.total)
    } else {
        parts.join(", ")
    }
}

/// `127.0.0.1:53781 → python3 (PID 43581)`; an IPv6 address in brackets
fn peer_line(peer: &Peer) -> String {
    let address = if peer.remote_address.contains(':') {
        format!("[{}]", peer.remote_address)
    } else {
        peer.remote_address.clone()
    };
    let end = format!("{address}:{}", peer.remote_port);
    match &peer.peer {
        None => end,
        Some(process) => match known(&process.name) {
            Some(name) => format!("{end} → {name} (PID {})", process.pid),
            None => format!("{end} → PID {}", process.pid),
        },
    }
}

struct Card<'a> {
    opts: &'a CardOptions,
}

impl Card<'_> {
    /// One line: the label padded to the value column, then the value cut to
    /// what is left of the width unless verbose
    fn put(
        &self,
        out: &mut impl Write,
        label: &str,
        value: &str,
        cut: fn(&str, usize) -> String,
    ) -> io::Result<()> {
        self.put_painted(out, label, value, cut, |shown| shown.to_string())
    }

    /// `put`, with `paint` applied to the value after it is cut
    fn put_painted(
        &self,
        out: &mut impl Write,
        label: &str,
        value: &str,
        cut: fn(&str, usize) -> String,
        paint: impl Fn(&str) -> String,
    ) -> io::Result<()> {
        let shown = if self.opts.verbose {
            value.to_string()
        } else {
            match (self.opts.width as usize).saturating_sub(VALUE_COLUMN) {
                // No room for a value: the label stays whole, the value is `…`
                0 => "…".to_string(),
                room => cut(value, room),
            }
        };
        writeln!(out, "{:INDENT$}{label:<LABEL_COLUMN$}{}", "", paint(&shown))
    }

    fn write_listening(&self, out: &mut impl Write, ports: &[CardPort]) -> io::Result<()> {
        if ports.is_empty() {
            return self.put(out, "Listening:", "none", truncate_string);
        }
        for (index, entry) in ports.iter().enumerate() {
            let label = if index == 0 { "Listening:" } else { "" };
            self.put(out, label, &listening(entry.port), truncate_string)?;
        }
        Ok(())
    }

    /// The connections of the ports that have any, and nothing when none of
    /// the ports says (a process that listens on nothing, or an OS that reads
    /// no connections)
    fn write_connections(&self, out: &mut impl Write, ports: &[CardPort]) -> io::Result<()> {
        let reporting: Vec<(&PortInfo, &Connections)> = ports
            .iter()
            .filter_map(|entry| entry.connections.map(|found| (entry.port, found)))
            .collect();
        if reporting.is_empty() {
            return Ok(());
        }
        let busy: Vec<_> = reporting
            .iter()
            .filter(|(_, found)| found.total > 0)
            .collect();
        if busy.is_empty() {
            return self.put(out, "Connections:", "none", truncate_string);
        }
        let mut label = "Connections:";
        for (port, found) in busy {
            let counts = state_counts(found);
            let value = if ports.len() > 1 {
                format!(":{} {counts}", port.port)
            } else {
                counts
            };
            self.put(out, label, &value, truncate_string)?;
            label = "";
            let shown = if self.opts.verbose {
                found.peers.len()
            } else {
                found.peers.len().min(PEERS_SHOWN)
            };
            for peer in &found.peers[..shown] {
                self.put(out, "", &peer_line(peer), truncate_string)?;
            }
            // `total` rather than the list: the list may be cut before it gets here
            let more = found.total.saturating_sub(shown);
            if more > 0 {
                let hint = if self.opts.verbose {
                    ""
                } else {
                    " (-v for all)"
                };
                let line = format!("… and {more} more{hint}");
                self.put(out, "", &line, truncate_string)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::connections::PeerProcess;
    use std::collections::BTreeMap;
    use unicode_width::UnicodeWidthStr;

    fn bare() -> Process {
        Process {
            pid: 39158,
            name: "python3".to_string(),
            exe_path: None,
            cwd: None,
            command: None,
            argv0: None,
            raw: None,
            cpu_percent: 0.0,
            memory_mb: 26.8,
            status: ProcessStatus::Running,
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

    fn full() -> Process {
        let mut p = bare();
        p.command = Some("python3 -m http.server 0 --bind 127.0.0.1".to_string());
        p.cwd = Some("/private/tmp/srv".to_string());
        p.exe_path = Some("/Users/zee/.local/bin/python3".to_string());
        p.user = Some("zee".to_string());
        p.parent_pid = Some(38120);
        p.parent_name = Some("zsh".to_string());
        p.cpu_time_secs = Some(4);
        p.open_files = Some(9);
        p.child_count = Some(0);
        p
    }

    fn port(number: u16, address: Option<&str>) -> PortInfo {
        PortInfo {
            port: number,
            protocol: Protocol::Tcp,
            pid: 39158,
            process_name: "python3".to_string(),
            address: address.map(str::to_string),
        }
    }

    fn peer(port: u16, name: Option<&str>) -> Peer {
        Peer {
            holder_pid: 39158,
            state: "established".to_string(),
            remote_address: "127.0.0.1".to_string(),
            remote_port: port,
            peer: Some(PeerProcess {
                pid: 43581,
                name: name.map(str::to_string),
            }),
        }
    }

    fn connections(states: &[(&str, usize)], peers: Vec<Peer>, total: usize) -> Connections {
        Connections {
            total,
            by_state: states
                .iter()
                .map(|(state, count)| (state.to_string(), *count))
                .collect::<BTreeMap<_, _>>(),
            peers,
            peers_truncated: false,
        }
    }

    /// A time of day about noon, local, so that nothing a test adds or takes
    /// away crosses midnight in any time zone
    fn noon() -> u64 {
        let base = 1_700_000_000u64;
        let local = base as i64 + local_offset(base);
        (base as i64 - local.rem_euclid(86_400) + 43_200) as u64
    }

    fn options(width: u16, verbose: bool) -> CardOptions {
        CardOptions {
            verbose,
            width,
            now_secs: noon(),
        }
    }

    /// The card with colour codes removed
    fn render(process: &Process, ports: &[CardPort], opts: &CardOptions) -> Vec<String> {
        let mut out = Vec::new();
        write_card(&mut out, process, ports, opts).unwrap();
        let text = String::from_utf8(out).unwrap();
        plain(&text).lines().map(str::to_string).collect()
    }

    /// `text` without its colour codes
    fn plain(text: &str) -> String {
        let mut plain = String::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                for skipped in chars.by_ref() {
                    if skipped == 'm' {
                        break;
                    }
                }
            } else {
                plain.push(c);
            }
        }
        plain
    }

    #[test]
    fn colour_codes_are_stripped_before_comparing() {
        assert_eq!(plain("\u{1b}[31mred\u{1b}[0m and plain"), "red and plain");
    }

    #[test]
    fn a_value_without_a_pid_or_too_narrow_for_it_is_cut_whole() {
        // no `(PID n)` to keep
        assert_eq!(
            cut_keeping_pid("PID 4242", 6),
            truncate_string("PID 4242", 6)
        );
        // the PID alone is wider than the room
        let value = "name (PID 4242)";
        assert_eq!(cut_keeping_pid(value, 8), truncate_string(value, 8));
    }

    /// The clock time of `secs` as the card writes it
    fn clock(secs: u64) -> String {
        let (_, _, _, h, m, s) = civil(secs as i64, local_offset(secs));
        format!("{h:02}:{m:02}:{s:02}")
    }

    #[test]
    fn the_card_lists_every_fact_in_order_and_aligned() {
        let mut process = full();
        process.cpu_percent = 0.04;
        process.start_time = Some(noon() - 13);
        let listening = port(53581, Some("127.0.0.1"));
        let found = connections(
            &[("established", 3)],
            (0..3).map(|i| peer(53781 + i, Some("python3"))).collect(),
            3,
        );
        let ports = [CardPort {
            port: &listening,
            connections: Some(&found),
        }];
        let lines = render(&process, &ports, &options(120, false));
        assert_eq!(
            lines,
            vec![
                "  Process:     python3 (PID 39158)".to_string(),
                "  Command:     python3 -m http.server 0 --bind 127.0.0.1".to_string(),
                "  Directory:   /private/tmp/srv".to_string(),
                "  Path:        /Users/zee/.local/bin/python3".to_string(),
                "  User:        zee".to_string(),
                "  Parent:      zsh (PID 38120)".to_string(),
                "  Status:      running".to_string(),
                "  Resources:   0.0% CPU, 26.8MB, 4s CPU time, 9 open files".to_string(),
                format!("  Started:     {} (up 13s)", clock(noon() - 13)),
                "  Children:    none".to_string(),
                "  Listening:   :53581 TCP on 127.0.0.1 (this machine only)".to_string(),
                "  Connections: 3 established".to_string(),
                "               127.0.0.1:53781 → python3 (PID 43581)".to_string(),
                "               127.0.0.1:53782 → python3 (PID 43581)".to_string(),
                "               127.0.0.1:53783 → python3 (PID 43581)".to_string(),
            ]
        );
    }

    #[test]
    fn unknown_facts_leave_no_line() {
        // Another user's process on macOS: no directory, user, parent, start,
        // CPU time, open files or child count
        let lines = render(&bare(), &[], &options(80, false));
        assert_eq!(
            lines,
            vec![
                "  Process:     python3 (PID 39158)".to_string(),
                "  Status:      running".to_string(),
                "  Resources:   0.0% CPU, 26.8MB".to_string(),
                "  Listening:   none".to_string(),
            ]
        );
        // Empty text is unknown too: no label with nothing after it
        let mut empty = bare();
        empty.command = Some(String::new());
        empty.cwd = Some(String::new());
        empty.exe_path = Some(String::new());
        empty.user = Some(String::new());
        empty.parent_pid = Some(1);
        empty.parent_name = Some(String::new());
        let lines = render(&empty, &[], &options(80, false));
        assert_eq!(lines.len(), 5);
        assert!(lines.contains(&"  Parent:      PID 1".to_string()));
        for line in &lines {
            assert!(line[VALUE_COLUMN..].trim() != "", "{line:?}");
        }
    }

    #[test]
    fn a_narrow_terminal_cuts_long_values_on_one_line() {
        let mut process = full();
        process.command = Some(format!("node {}", "--flag=value ".repeat(40)));
        process.cwd = Some(format!("/Users/zee/{}/project", "deep/".repeat(20)));
        process.start_time = Some(noon() - 13);
        let listening = port(3000, Some("192.168.100.200"));
        let found = connections(&[("established", 1)], vec![peer(53781, Some("curl"))], 1);
        let ports = [CardPort {
            port: &listening,
            connections: Some(&found),
        }];
        let lines = render(&process, &ports, &options(40, false));
        for line in &lines {
            assert!(line.width() <= 40, "{line:?} is {} wide", line.width());
        }
        let command = lines.iter().find(|l| l.contains("Command:")).unwrap();
        assert!(command.starts_with("  Command:     node --flag"));
        assert!(command.ends_with('…'));
        let directory = lines.iter().find(|l| l.contains("Directory:")).unwrap();
        assert!(directory.contains('…'));
        let listening_line = lines.iter().find(|l| l.contains("Listening:")).unwrap();
        assert!(listening_line.ends_with('…'));
        // One line per fact: no wrapping, no lost lines
        assert_eq!(
            lines.len(),
            render(&process, &ports, &options(200, false)).len()
        );
        // A width with no room for a value still shows the label and `…`
        let squeezed = render(&process, &ports, &options(10, false));
        assert_eq!(squeezed[0], "  Process:     …");
        assert!(squeezed.iter().all(|line| line.ends_with('…')));
    }

    #[test]
    fn verbose_prints_everything_uncut() {
        let mut process = full();
        let command = format!("node {}", "--flag=value ".repeat(40));
        process.command = Some(command.clone());
        let cwd = format!("/Users/zee/{}/project", "deep/".repeat(20));
        process.cwd = Some(cwd.clone());
        let lines = render(&process, &[], &options(40, true));
        assert!(lines.contains(&format!("  Command:     {command}")));
        assert!(lines.contains(&format!("  Directory:   {cwd}")));
        assert!(lines.iter().all(|line| !line.contains('…')));
    }

    #[test]
    fn five_peers_then_a_count() {
        let listening = port(3000, Some("127.0.0.1"));
        let peers: Vec<Peer> = (0..8).map(|i| peer(50000 + i, Some("curl"))).collect();
        let found = connections(&[("established", 8)], peers, 8);
        let ports = [CardPort {
            port: &listening,
            connections: Some(&found),
        }];
        let lines = render(&bare(), &ports, &options(120, false));
        let start = lines
            .iter()
            .position(|l| l.contains("Connections:"))
            .unwrap();
        assert_eq!(lines.len() - start, 1 + 5 + 1);
        assert_eq!(lines[start], "  Connections: 8 established");
        assert_eq!(
            lines[start + 1],
            "               127.0.0.1:50000 → curl (PID 43581)"
        );
        assert_eq!(
            lines[start + 5],
            "               127.0.0.1:50004 → curl (PID 43581)"
        );
        assert_eq!(lines[start + 6], "               … and 3 more (-v for all)");

        // -v lists all eight and no count
        let all = render(&bare(), &ports, &options(120, true));
        assert_eq!(all.len() - start, 1 + 8);
        assert!(all.iter().all(|line| !line.contains("more")));

        // The JSON cap cut the list at 8 of 12: the count says 7 more, or 4 with -v
        let capped = connections(
            &[("established", 12)],
            (0..8).map(|i| peer(50000 + i, None)).collect(),
            12,
        );
        let ports = [CardPort {
            port: &listening,
            connections: Some(&capped),
        }];
        let lines = render(&bare(), &ports, &options(120, false));
        assert_eq!(
            lines.last().unwrap(),
            "               … and 7 more (-v for all)"
        );
        let lines = render(&bare(), &ports, &options(120, true));
        assert_eq!(lines.last().unwrap(), "               … and 4 more");
    }

    #[test]
    fn no_ports_reads_none() {
        let lines = render(&full(), &[], &options(80, false));
        assert!(lines.contains(&"  Listening:   none".to_string()));
        assert!(lines.iter().all(|line| !line.contains("Connections:")));
    }

    #[test]
    fn a_listening_process_without_connections_says_none() {
        let listening = port(3000, Some("0.0.0.0"));
        let found = Connections::default();
        let ports = [CardPort {
            port: &listening,
            connections: Some(&found),
        }];
        let lines = render(&bare(), &ports, &options(120, false));
        assert_eq!(
            &lines[lines.len() - 2..],
            [
                "  Listening:   :3000 TCP on 0.0.0.0 (all interfaces, reachable from the network)",
                "  Connections: none",
            ]
        );
    }

    #[test]
    fn ports_whose_connections_are_not_known_leave_the_line_out() {
        let listening = port(3000, None);
        let ports = [CardPort {
            port: &listening,
            connections: None,
        }];
        let lines = render(&bare(), &ports, &options(120, false));
        assert_eq!(
            lines.last().unwrap(),
            "  Listening:   :3000 TCP (address not known, treated as reachable from the network)"
        );
        assert!(lines.iter().all(|line| !line.contains("Connections:")));
    }

    #[test]
    fn several_ports_get_a_connections_line_each_with_connections() {
        let first = port(3000, Some("127.0.0.1"));
        let mut second = port(5432, Some("::"));
        second.protocol = Protocol::Udp;
        let third = port(8080, Some("10.0.0.5"));
        let on_first = connections(
            &[("close_wait", 1), ("established", 2), ("fin_wait1", 1)],
            (0..4).map(|i| peer(50000 + i, None)).collect(),
            4,
        );
        let quiet = Connections::default();
        let mut v6 = peer(50001, Some("psql"));
        v6.remote_address = "::1".to_string();
        let on_third = connections(&[("established", 1)], vec![v6], 1);
        let ports = [
            CardPort {
                port: &first,
                connections: Some(&on_first),
            },
            CardPort {
                port: &second,
                connections: Some(&quiet),
            },
            CardPort {
                port: &third,
                connections: Some(&on_third),
            },
        ];
        let lines = render(&bare(), &ports, &options(120, false));
        let start = lines.iter().position(|l| l.contains("Listening:")).unwrap();
        assert_eq!(
            &lines[start..],
            [
                "  Listening:   :3000 TCP on 127.0.0.1 (this machine only)",
                "               :5432 UDP on :: (all interfaces, reachable from the network)",
                "               :8080 TCP on 10.0.0.5 (network address 10.0.0.5)",
                "  Connections: :3000 2 established, 1 close_wait, 1 fin_wait1",
                "               127.0.0.1:50000 → PID 43581",
                "               127.0.0.1:50001 → PID 43581",
                "               127.0.0.1:50002 → PID 43581",
                "               127.0.0.1:50003 → PID 43581",
                "               :8080 1 established",
                "               [::1]:50001 → psql (PID 43581)",
            ]
        );
    }

    #[test]
    fn a_peer_with_nobody_visible_is_the_address_alone() {
        let listening = port(3000, Some("127.0.0.1"));
        let mut stranger = peer(50000, None);
        stranger.peer = None;
        let found = connections(&[], vec![stranger], 2);
        let ports = [CardPort {
            port: &listening,
            connections: Some(&found),
        }];
        let lines = render(&bare(), &ports, &options(120, false));
        let start = lines
            .iter()
            .position(|l| l.contains("Connections:"))
            .unwrap();
        assert_eq!(lines[start], "  Connections: 2 in all");
        assert_eq!(lines[start + 1], "               127.0.0.1:50000");
        assert_eq!(lines[start + 2], "               … and 1 more (-v for all)");
    }

    #[test]
    fn parent_children_and_open_files_read_as_the_views_describe() {
        let mut process = bare();
        process.parent_pid = Some(1);
        process.child_count = Some(2);
        process.open_files = Some(1);
        process.cpu_time_secs = Some(192);
        process.cpu_percent = 12.34;
        process.memory_mb = 1500.0;
        process.status = ProcessStatus::Sleeping;
        let lines = render(&process, &[], &options(120, false));
        assert!(lines.contains(&"  Parent:      PID 1".to_string()));
        assert!(lines.contains(&"  Children:    2".to_string()));
        assert!(lines.contains(&"  Status:      sleeping".to_string()));
        assert!(lines.contains(
            &"  Resources:   12.3% CPU, 1.5GB, 3m 12s CPU time, 1 open file".to_string()
        ));
        for status in [
            ProcessStatus::Stopped,
            ProcessStatus::Zombie,
            ProcessStatus::Dead,
            ProcessStatus::Unknown,
        ] {
            process.status = status;
            let lines = render(&process, &[], &options(120, false));
            assert!(lines.contains(&format!("  Status:      {}", status_word(status))));
        }
    }

    #[test]
    fn a_start_on_another_day_carries_its_date() {
        let mut process = bare();
        let start = noon() - 3 * 86_400 - 60;
        process.start_time = Some(start);
        let lines = render(&process, &[], &options(120, false));
        let offset = local_offset(start);
        let (_, month, day, _, _, _) = civil(start as i64, offset);
        let expected = format!(
            "  Started:     {} {day} {} (up 3d 0h)",
            MONTHS[month as usize - 1],
            clock(start)
        );
        assert!(lines.contains(&expected), "{lines:?}");
        // A start after now (clocks differ) reads as up 0s, never underflows
        process.start_time = Some(noon() + 5);
        let lines = render(&process, &[], &options(120, false));
        assert!(lines.iter().any(|l| l.ends_with("(up 0s)")), "{lines:?}");
    }

    #[test]
    fn a_long_parent_name_is_cut_and_its_pid_stays() {
        let mut process = bare();
        process.parent_pid = Some(4242);
        process.parent_name = Some("verylongprocessnamethatdoesnotfit".to_string());
        let lines = render(&process, &[], &options(40, false));
        let parent = lines.iter().find(|l| l.contains("Parent:")).unwrap();
        assert_eq!(parent, "  Parent:      verylongproce… (PID 4242)");
        assert_eq!(parent.width(), 40);
        // Too narrow even for the PID: the whole value is cut
        let lines = render(&process, &[], &options(24, false));
        assert!(
            lines.contains(&"  Parent:      verylong…".to_string()),
            "{lines:?}"
        );
        // Wide enough: nothing is cut
        let lines = render(&process, &[], &options(120, false));
        assert!(lines
            .contains(&"  Parent:      verylongprocessnamethatdoesnotfit (PID 4242)".to_string()));
    }

    #[test]
    fn civil_time_is_the_calendar() {
        // 2023-11-14 22:13:20 UTC
        assert_eq!(civil(1_700_000_000, 0), (2023, 11, 14, 22, 13, 20));
        // Five hours ahead crosses into the 15th; two hours behind stays on the 14th
        assert_eq!(civil(1_700_000_000, 5 * 3600), (2023, 11, 15, 3, 13, 20));
        assert_eq!(civil(1_700_000_000, -2 * 3600), (2023, 11, 14, 20, 13, 20));
        // The epoch, a leap day and a year's end
        assert_eq!(civil(0, 0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil(1_709_164_800, 0), (2024, 2, 29, 0, 0, 0));
        assert_eq!(civil(1_735_689_599, 0), (2024, 12, 31, 23, 59, 59));
        // Before the epoch
        assert_eq!(civil(-1, 0), (1969, 12, 31, 23, 59, 59));
    }

    #[test]
    fn the_local_offset_is_a_sane_zone() {
        let offset = local_offset(1_700_000_000);
        assert!((-14 * 3600..=14 * 3600).contains(&offset), "{offset}");
        assert!(same_local_day(noon(), noon() + 60));
        assert!(!same_local_day(noon(), noon() + 86_400));
    }

    #[test]
    fn a_failed_write_is_an_error_not_a_panic() {
        let listening = port(3000, Some("127.0.0.1"));
        let found = connections(&[("established", 1)], vec![peer(50000, None)], 1);
        let ports = [CardPort {
            port: &listening,
            connections: Some(&found),
        }];
        let mut process = full();
        process.start_time = Some(noon() - 13);
        crate::test_support::assert_stops_at_a_failed_write(|out| {
            write_card(out, &process, &ports, &options(80, false))
        });
    }
}
