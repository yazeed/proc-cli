//! Shared formatting utilities used across commands and output
//!
//! Provides common string formatting, truncation, and colorization.

use crate::core::{Process, ProcessStatus};
use colored::*;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Format a duration in seconds to a human-readable string.
///
/// Examples: "45s", "3m 12s", "2h 15m", "1d 6h"
pub fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else if secs < 86400 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d {}h", secs / 86400, (secs % 86400) / 3600)
    }
}

/// Uptime as a trailing label for one-line process views (tree, why):
/// `"  up 3m 12s"`, or empty when unknown.
pub fn uptime_label(uptime_secs: Option<u64>) -> String {
    uptime_secs
        .map(|secs| format!("  up {}", format_duration(secs)))
        .unwrap_or_default()
}

/// Return "es" for plural process counts, "" for singular.
///
/// Usage: `format!("{} process{}", n, plural(n))` → "1 process" or "3 processes"
pub fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "es"
    }
}

/// Truncate `s` to `max_width` terminal columns, appending `…` when cut.
///
/// Counts display width rather than bytes, so CJK and emoji (two columns each)
/// stay aligned, and a multi-byte character is never split.
pub fn truncate_string(s: &str, max_width: usize) -> String {
    if s.width() <= max_width {
        return s.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    // One column is reserved for the ellipsis.
    let mut out = take_width(s, max_width - 1);
    out.push('…');
    out
}

/// Truncate a path to `max_width` terminal columns, eliding the middle.
///
/// The tail is what differs between rows (`.next/standalone` vs `.next/server`),
/// so it keeps the larger share and the root stays visible:
/// `/Users/zee/…/maliqe-website/.next/standalone`.
pub fn truncate_path(path: &str, max_width: usize) -> String {
    if path.width() <= max_width {
        return path.to_string();
    }
    match max_width {
        0 => String::new(),
        1 => "…".to_string(),
        _ => {
            let budget = max_width - 1;
            // Snap the head to a path separator so it reads as a real prefix
            // ("/Users/") rather than an arbitrary cut ("/Use").
            let head = take_width(path, budget / 3);
            let head = match head.rfind('/') {
                Some(i) => &head[..=i],
                None => "",
            };
            // Too narrow for a prefix worth printing: spend it all on the tail,
            // which is the part that differs between rows.
            if head.width() < 2 {
                let mut out = String::from("…");
                out.push_str(&take_width_from_end(path, budget));
                return out;
            }
            let tail_width = budget - head.width();
            let mut out = String::from(head);
            out.push('…');
            out.push_str(&take_width_from_end(path, tail_width));
            out
        }
    }
}

/// Longest prefix of `s` fitting in `max_width` columns, cut on a char boundary.
fn take_width(s: &str, max_width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let w = ch.width().unwrap_or(0);
        if used + w > max_width {
            break;
        }
        out.push(ch);
        used += w;
    }
    out
}

/// Longest suffix of `s` fitting in `max_width` columns, cut on a char boundary.
fn take_width_from_end(s: &str, max_width: usize) -> String {
    let mut chars = Vec::new();
    let mut used = 0;
    for ch in s.chars().rev() {
        let w = ch.width().unwrap_or(0);
        if used + w > max_width {
            break;
        }
        chars.push(ch);
        used += w;
    }
    chars.iter().rev().collect()
}

/// What the three flexible process-table columns may use at a given terminal
/// width, once the fixed columns are accounted for.
///
/// **Deprecated since 2.3.0:** the process table no longer uses it (see
/// [`crate::ui::table`]); it is kept for code that calls it and goes in 3.0.
#[deprecated(
    since = "2.3.0",
    note = "the process table no longer uses it; see proc_cli::ui::table"
)]
pub struct ProcessTableBudget {
    /// Column width for PID, including comfy-table's padding.
    pub pid: usize,
    /// Column width for CPU%.
    pub cpu: usize,
    /// Column width for MEM.
    pub mem: usize,
    /// Column width for UPTIME.
    pub uptime: usize,
    /// Column width for STATUS.
    pub status: usize,
    /// Content width for DIR.
    pub dir: usize,
    /// Content width for NAME.
    pub name: usize,
    /// Content width for ARGS.
    pub args: usize,
    /// UPTIME only fits next to a readable ARGS column on wider terminals.
    pub show_uptime: bool,
}

/// Column budget for a process table rendered at `width` terminal columns.
///
/// Both process tables used to size their columns from this.
///
/// Pre-truncating each flexible column to its share is what keeps one process
/// on one line: comfy-table wraps any cell that does not fit, and a wrapped
/// cell destroys the alignment that makes a table readable.
///
/// **Deprecated since 2.3.0:** the process table no longer uses it (see
/// [`crate::ui::table`]); it is kept for code that calls it and goes in 3.0.
#[deprecated(
    since = "2.3.0",
    note = "the process table no longer uses it; see proc_cli::ui::table"
)]
#[allow(deprecated)]
pub fn process_table_budget(width: u16) -> ProcessTableBudget {
    // Fixed columns: content plus comfy-table's 1-column padding on each side.
    const PID_W: usize = 8;
    const CPU_W: usize = 8;
    const MEM_W: usize = 11;
    const UPTIME_W: usize = 10;
    const STATUS_W: usize = 12;
    // Each flexible column renders as its content width plus comfy-table's
    // 2 columns of padding, so reserve those 6 up front.
    const FLEX_PADDING: usize = 6;
    // Measured slack: with arrangement disabled the fixed columns render a few
    // columns wider than their constraints, and a line exactly as wide as the
    // terminal wraps in some emulators. Verified against rendered output at
    // 80/100/120/160/200 columns.
    const SAFETY: usize = 7;

    let show_uptime = width >= 100;
    let fixed = PID_W + CPU_W + MEM_W + STATUS_W + if show_uptime { UPTIME_W } else { 0 };
    let avail = (width as usize)
        .saturating_sub(fixed + FLEX_PADDING + SAFETY)
        .max(24);
    let name = (avail / 4).clamp(8, 16);
    let rest = avail - name;
    let args = (rest / 2).max(12);
    let dir = rest - args;
    ProcessTableBudget {
        pid: PID_W,
        cpu: CPU_W,
        mem: MEM_W,
        uptime: UPTIME_W,
        status: STATUS_W,
        dir,
        name,
        args,
        show_uptime,
    }
}

/// Format memory in MB to a human-readable string with adaptive units.
///
/// Examples: "512KB", "6.0MB", "3.0GB"
pub fn format_memory(memory_mb: f64) -> String {
    if memory_mb < 1.0 {
        format!("{:.0}KB", memory_mb * 1000.0)
    } else if memory_mb < 1000.0 {
        format!("{:.1}MB", memory_mb)
    } else {
        format!("{:.1}GB", memory_mb / 1000.0)
    }
}

/// CPU (`"0.3%"`) and memory (`"6.1MB"`) of a process, when the OS gave them; `None`
/// when it did not (on macOS and Windows, a process proc was not allowed to read),
/// so no view shows a 0 that nobody measured. The one place the one-line views (tree, wait) take both from.
pub fn cpu_and_memory(process: &Process) -> Option<(String, String)> {
    let cpu = format!("{:.1}%", process.cpu_percent);
    let memory = format_memory(process.memory_mb);
    process.read_by_os().then_some((cpu, memory))
}

/// The CPU and memory of a tree line, `" 0.3% 6.1MB"`, or empty when the OS gave none
pub fn usage_label(process: &Process) -> String {
    cpu_and_memory(process).map_or_else(String::new, |(cpu, memory)| format!(" {cpu} {memory}"))
}

/// Colorize a process status string based on the status variant.
pub fn colorize_status(status: &ProcessStatus, status_str: &str) -> ColoredString {
    match status {
        ProcessStatus::Running => status_str.green(),
        ProcessStatus::Sleeping | ProcessStatus::Idle => status_str.blue(),
        ProcessStatus::Stopped | ProcessStatus::DiskWait => status_str.yellow(),
        ProcessStatus::Zombie => status_str.red(),
        _ => status_str.white(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    /// Strings that previously panicked: byte slicing cut inside a multi-byte char.
    const NON_ASCII: &[&str] = &[
        "éééééééééééééééééééééééééééééééééééééééé",
        "café-ünïcødé-très-lông-serveur",
        "/Users/zee/Sites/café-website/.next/standalone/ünïcødé",
        "日本語のプロセス名前テストですこれは長い",
        "🚀🚀🚀 deploy 🚀🚀🚀 a very long emoji argument line 🚀🚀🚀",
        "Ωmega-Δelta-Σigma-Πi-Φphi-Ψpsi-very-long-name",
    ];

    /// A record with the figures given and no CPU time
    fn process_with(cpu_percent: f32, memory_mb: f64) -> Process {
        Process {
            pid: 7,
            name: "p7".to_string(),
            exe_path: None,
            cwd: None,
            command: None,
            argv0: None,
            raw: None,
            cpu_percent,
            memory_mb,
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
    fn the_figures_of_a_process_the_os_read_are_shown() {
        // memory in use is a reading on every OS
        let read = process_with(1.5, 6.1);
        assert_eq!(
            cpu_and_memory(&read),
            Some(("1.5%".to_string(), "6.1MB".to_string()))
        );
        assert_eq!(usage_label(&read), " 1.5% 6.1MB");
    }

    #[test]
    fn a_process_the_os_gave_nothing_for_gets_no_zero_figures() {
        // 0% and 0KB with no CPU time: not read on macOS and Windows, a reading elsewhere
        let unread = process_with(0.0, 0.0);
        if cfg!(any(target_os = "macos", windows)) {
            assert_eq!(cpu_and_memory(&unread), None);
            assert_eq!(usage_label(&unread), "");
        } else {
            assert_eq!(usage_label(&unread), " 0.0% 0KB");
        }
    }

    #[test]
    fn truncate_string_never_panics_at_any_width() {
        for s in NON_ASCII {
            for max in 0..=s.len() + 2 {
                let _ = truncate_string(s, max);
            }
        }
    }

    #[test]
    fn truncate_path_never_panics_at_any_width() {
        for s in NON_ASCII {
            for max in 0..=s.len() + 2 {
                let _ = truncate_path(s, max);
            }
        }
    }

    #[test]
    fn truncate_string_respects_display_width() {
        for s in NON_ASCII {
            for max in 1..=40 {
                let out = truncate_string(s, max);
                let width = out.width();
                assert!(
                    width <= max,
                    "width {width} exceeded max {max} for {s:?} -> {out:?}"
                );
            }
        }
    }

    #[test]
    fn truncate_path_respects_display_width() {
        for s in NON_ASCII {
            for max in 1..=40 {
                let out = truncate_path(s, max);
                let width = out.width();
                assert!(
                    width <= max,
                    "width {width} exceeded max {max} for {s:?} -> {out:?}"
                );
            }
        }
    }

    #[test]
    fn short_values_pass_through_unchanged() {
        assert_eq!(truncate_string("node", 20), "node");
        assert_eq!(truncate_path("/usr/bin", 20), "/usr/bin");
        // Exactly at the limit is not truncated
        assert_eq!(truncate_string("abcde", 5), "abcde");
    }

    #[test]
    fn truncate_path_elides_the_middle_keeping_head_and_tail() {
        let p = "/Users/zee/Sites/maliqe-website/.next/standalone";
        let out = truncate_path(p, 30);
        assert!(out.contains('…'), "expected a middle ellipsis in {:?}", out);
        assert!(out.starts_with('/'), "head should survive: {:?}", out);
        assert!(
            out.ends_with("standalone"),
            "distinguishing tail should survive: {:?}",
            out
        );
    }

    #[test]
    fn truncate_path_falls_back_to_tail_only_when_too_narrow() {
        let p = "/Users/zee/Sites/weirdpredictionmarkets";
        let out = truncate_path(p, 14);
        assert!(out.width() <= 14, "{:?} is {} wide", out, out.width());
        assert!(
            out.starts_with('…'),
            "expected tail-only form, got {:?}",
            out
        );
        assert!(out.ends_with("markets"), "tail should survive: {:?}", out);
    }

    #[test]
    fn truncate_path_head_snaps_to_a_separator() {
        let p = "/Users/zee/Sites/maliqe-website/.next/standalone";
        let out = truncate_path(p, 34);
        assert!(out.width() <= 34, "{:?} is {} wide", out, out.width());
        let head = out.split('…').next().unwrap();
        assert!(
            head.ends_with('/'),
            "head {:?} should end at a separator",
            head
        );
    }

    #[test]
    fn cjk_counts_as_two_columns() {
        // 10 CJK chars = 20 columns; at max 10 we must emit at most 10 columns
        let s = "日本語のプロセス名前";
        let out = truncate_string(s, 10);
        assert!(out.width() <= 10, "got width {} for {:?}", out.width(), out);
    }

    #[test]
    fn durations_use_the_two_largest_units() {
        for (secs, shown) in [
            (0, "0s"),
            (59, "59s"),
            (60, "1m 0s"),
            (3599, "59m 59s"),
            (3600, "1h 0m"),
            (86399, "23h 59m"),
            (86400, "1d 0h"),
            (90000 + 3600 * 5, "1d 6h"),
        ] {
            assert_eq!(format_duration(secs), shown, "{secs} seconds");
        }
    }

    #[test]
    fn the_uptime_label_is_empty_when_the_uptime_is_unknown() {
        assert_eq!(uptime_label(None), "");
        assert_eq!(uptime_label(Some(192)), "  up 3m 12s");
    }

    #[test]
    fn only_one_process_is_singular() {
        assert_eq!(plural(0), "es");
        assert_eq!(plural(1), "");
        assert_eq!(plural(2), "es");
    }

    #[test]
    fn memory_switches_unit_at_a_megabyte_and_at_a_gigabyte() {
        assert_eq!(format_memory(0.0), "0KB");
        assert_eq!(format_memory(0.5), "500KB");
        assert_eq!(format_memory(1.0), "1.0MB");
        assert_eq!(format_memory(999.94), "999.9MB");
        assert_eq!(format_memory(1000.0), "1.0GB");
        assert_eq!(format_memory(3060.0), "3.1GB");
    }

    #[test]
    fn each_status_has_its_own_colour() {
        for (status, colour) in [
            (ProcessStatus::Running, Color::Green),
            (ProcessStatus::Sleeping, Color::Blue),
            (ProcessStatus::Stopped, Color::Yellow),
            (ProcessStatus::Zombie, Color::Red),
            (ProcessStatus::Dead, Color::White),
            (ProcessStatus::Idle, Color::Blue),
            (ProcessStatus::Starting, Color::White),
            (ProcessStatus::DiskWait, Color::Yellow),
            (ProcessStatus::Unknown, Color::White),
        ] {
            let coloured = colorize_status(&status, "word");
            assert_eq!(coloured.fgcolor, Some(colour), "{status:?}");
            assert_eq!(coloured.input, "word");
        }
    }

    #[test]
    #[allow(deprecated)]
    fn the_budget_adds_uptime_from_100_columns_and_always_fits_the_minimum() {
        let narrow = process_table_budget(99);
        assert!(!narrow.show_uptime);
        let wide = process_table_budget(100);
        assert!(wide.show_uptime);
        // Never below the floor, however small the terminal
        let tiny = process_table_budget(10);
        assert_eq!(tiny.dir + tiny.name + tiny.args, 24);
        assert_eq!((tiny.name, tiny.args, tiny.dir), (8, 12, 4));
        // At 120 columns the three flexible columns share what the fixed ones leave
        let b = process_table_budget(120);
        assert_eq!((b.name, b.args, b.dir), (14, 22, 22));
        assert_eq!(
            (b.pid, b.cpu, b.mem, b.uptime, b.status),
            (8, 8, 11, 10, 12)
        );
    }
}
