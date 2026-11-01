//! The human-readable output, through the real binary: colours on and off,
//! the table at different terminal widths, the verbose list, and the dry-run
//! preview.
//!
//! Most of the printing code is checked line by line next to it (src/ui);
//! these tests check what a person at a terminal, or a script reading a pipe,
//! actually gets.
//!
//! Every process these tests look at is one the test started: a copy of the
//! listener example (`examples/listener.rs`) under a name and path of its own,
//! in a fresh temp directory, so `--in <dir>` matches only it. Each process is
//! killed and reaped on drop.

use assert_cmd::Command;
use proc_cli::core::filters::strip_verbatim_prefix;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const ESC: &str = "\u{1b}";

/// proc with the colour variables cleared, so each test says what it wants
fn proc_cmd() -> Command {
    let mut cmd = Command::cargo_bin("proc").unwrap();
    cmd.env_remove("NO_COLOR").env_remove("CLICOLOR_FORCE");
    cmd
}

/// proc with colour off
fn plain_cmd() -> Command {
    let mut cmd = proc_cmd();
    cmd.env("NO_COLOR", "1");
    cmd
}

/// proc with colour forced on, though stdout is a pipe
fn coloured_cmd() -> Command {
    let mut cmd = proc_cmd();
    cmd.env("CLICOLOR_FORCE", "1");
    cmd
}

/// Run a command and return its stdout, stderr and exit code
fn run(mut cmd: Command, args: &[&str]) -> (String, String, i32) {
    let output = cmd.args(args).output().unwrap();
    (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
        output.status.code().unwrap(),
    )
}

/// `text` without its colour escape sequences
fn strip_colours(text: &str) -> String {
    regex::Regex::new("\u{1b}\\[[0-9;]*m")
        .unwrap()
        .replace_all(text, "")
        .into_owned()
}

/// `target/debug/examples/listener`, found relative to this test binary
/// (`target/debug/deps/human_output-<hash>`)
fn listener_path() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("examples");
    path.push(format!("listener{}", std::env::consts::EXE_SUFFIX));
    assert!(path.exists(), "build the example first: {}", path.display());
    path
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Run the copy, with its cwd in `dir`. On Linux a file another thread has
/// just written can be "busy" (ETXTBSY) for an instant: a fork in a parallel
/// test still holds the write handle until it execs. That clears in
/// milliseconds, so try again.
fn spawn_copy(copy: &Path, dir: &Path) -> Child {
    let mut attempts = 0;
    loop {
        let spawned = std::process::Command::new(copy)
            .current_dir(dir)
            .stdout(Stdio::piped())
            .stdin(Stdio::null())
            .spawn();
        match spawned {
            Ok(child) => return child,
            Err(e) if attempts < 100 && is_text_file_busy(&e) => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => panic!("spawn the listener copy: {e}"),
        }
    }
}

#[cfg(unix)]
fn is_text_file_busy(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(libc::ETXTBSY)
}

#[cfg(not(unix))]
fn is_text_file_busy(_: &std::io::Error) -> bool {
    false
}

/// A listener the test owns: a copy of the listener example, run with its cwd
/// in a fresh temp directory. Killed and reaped on drop.
struct Owned {
    child: Child,
    /// A name no other process has, short enough for any OS to report whole
    name: String,
    /// Keeps the directory alive
    _tmp: tempfile::TempDir,
    /// The directory as processes report it: symlinks resolved, no Windows
    /// `\\?\` prefix
    dir: PathBuf,
    /// The directory exactly as `tempfile` named it: on macOS `/var/folders/..`
    /// (reported by processes as `/private/var/..`), on Windows possibly an
    /// 8.3 short name. This is what a user would type.
    raw_dir: PathBuf,
}

impl Owned {
    fn listener() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let raw_dir = tmp.path().to_path_buf();
        let dir = raw_dir.canonicalize().unwrap();
        let dir = PathBuf::from(strip_verbatim_prefix(&dir.to_string_lossy()));
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let name = format!("pho{}n{n}x", std::process::id());
        let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        std::fs::copy(listener_path(), raw_dir.join(&file)).expect("copy the listener example");
        // Started by its canonical path, so the executable path processes
        // report is that path on every OS; the cwd is the path as the user
        // would type it
        let mut child = spawn_copy(&dir.join(&file), &raw_dir);
        let mut line = String::new();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let read = stdout.read_line(&mut line);
        if read.is_err() || !line.starts_with("PORT ") {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the listener printed {line:?} instead of `PORT <n>`");
        }
        Owned {
            child,
            name,
            _tmp: tmp,
            dir,
            raw_dir,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// The directory the way a user types it, for `--in`
    fn raw_dir(&self) -> String {
        self.raw_dir.to_string_lossy().into_owned()
    }

    /// The name the process shows in the NAME column: its file name, which
    /// carries `.exe` on Windows
    fn shown_name(&self) -> String {
        format!("{}{}", self.name, std::env::consts::EXE_SUFFIX)
    }

    /// Whether this is the NAME cell of the process: its whole name, or the
    /// start of it cut short with `…` when the terminal is narrow
    fn name_cell_matches(&self, cell: &str) -> bool {
        let name = self.shown_name();
        match cell.strip_suffix('…') {
            Some(start) => name.starts_with(start),
            None => cell == name,
        }
    }

    /// The directory as proc prints it: resolved, without a `\\?\` prefix
    fn printed_dir(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }

    /// What proc itself reports for this process, for a failure message
    fn what_proc_saw(&self) -> String {
        let (out, err, _) = run(plain_cmd(), &["info", &self.pid().to_string(), "--json"]);
        format!(
            "proc info {} --json (raw dir {}, canonical dir {}):\n{out}{err}",
            self.pid(),
            self.raw_dir.display(),
            self.dir.display(),
        )
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A fresh empty directory: the path a user would type for it, and the path
/// proc prints for it (resolved, without a Windows `\\?\` prefix)
fn empty_dir() -> (tempfile::TempDir, String, String) {
    let tmp = tempfile::tempdir().unwrap();
    let typed = tmp.path().to_string_lossy().into_owned();
    let resolved = tmp.path().canonicalize().unwrap();
    let printed = strip_verbatim_prefix(&resolved.to_string_lossy()).to_string();
    (tmp, typed, printed)
}

// ---- colour ----

#[test]
fn colour_is_off_when_stdout_is_a_pipe() {
    let owned = Owned::listener();
    let (out, _, code) = run(proc_cmd(), &["list", "--in", &owned.raw_dir()]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains(ESC), "escape codes in piped output: {out:?}");
}

#[test]
fn no_colour_turns_it_off() {
    let owned = Owned::listener();
    let (out, _, code) = run(plain_cmd(), &["list", "--in", &owned.raw_dir(), "-v"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains(ESC), "escape codes with NO_COLOR: {out:?}");
}

#[test]
fn clicolor_force_colours_a_pipe_and_overrides_no_colour() {
    let owned = Owned::listener();
    let dir = owned.printed_dir();
    for with_no_colour in [false, true] {
        let mut cmd = coloured_cmd();
        if with_no_colour {
            cmd.env("NO_COLOR", "1");
        }
        let (out, _, code) = run(cmd, &["list", "--in", &owned.raw_dir()]);
        assert_eq!(code, 0, "{out}");
        // The headline: a bold green tick, the count in bold cyan, the context grey
        let headline = out.lines().next().unwrap();
        assert_eq!(
            headline,
            format!(
                "{ESC}[1;32m✓{ESC}[0m Found {ESC}[1;36m1{ESC}[0m process{ESC}[90m in {dir}{ESC}[0m"
            ),
            "NO_COLOR set: {with_no_colour}\n{}",
            owned.what_proc_saw()
        );
    }
}

#[test]
fn coloured_text_is_the_plain_text_plus_escape_codes() {
    let owned = Owned::listener();
    let dir = owned.raw_dir();
    // The headline and the process's name, status and directory are the same
    // in both runs; the CPU and uptime figures move between runs, so compare
    // the lines that hold none
    for args in [vec!["list", "--in", &dir], vec!["list", "--in", &dir, "-v"]] {
        let (coloured, _, _) = run(coloured_cmd(), &args);
        let (plain, _, _) = run(plain_cmd(), &args);
        assert!(coloured.contains(ESC), "{coloured:?}");
        let stable = |text: &str| -> Vec<String> {
            text.lines()
                .filter(|l| {
                    let l = l.trim_start();
                    l.starts_with("✓") || l.starts_with("cwd:") || l.starts_with("exe:")
                })
                .map(str::to_string)
                .collect()
        };
        assert_eq!(
            stable(&strip_colours(&coloured)),
            stable(&plain),
            "{args:?}\n{}",
            owned.what_proc_saw()
        );
    }
}

#[test]
fn a_warning_is_yellow_with_colour_and_plain_without() {
    let (_tmp, typed, dir) = empty_dir();
    let args = ["list", "--in", &typed];

    let (out, _, code) = run(plain_cmd(), &args);
    assert_eq!(code, 0);
    assert_eq!(out, format!("⚠ No processes found in {dir}\n"));

    let (out, _, code) = run(coloured_cmd(), &args);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        format!("{ESC}[1;33m⚠{ESC}[0m {ESC}[33mNo processes found in {dir}{ESC}[0m\n")
    );
}

#[test]
fn a_dry_run_ends_with_a_yellow_summary_under_the_table() {
    let owned = Owned::listener();
    let pid = owned.pid().to_string();
    let (out, err, code) = run(coloured_cmd(), &["kill", &pid, "--dry-run"]);
    assert_eq!(code, 0, "{out}{err}");
    let last = out.lines().last().unwrap();
    assert_eq!(
        last,
        format!("{ESC}[1;33m⚠{ESC}[0m {ESC}[33mDry run: would kill 1 process{ESC}[0m")
    );
}

// ---- the table and the verbose list ----

/// The cells of a table row, with the column padding gone
fn cells(line: &str) -> Vec<&str> {
    line.split_whitespace().collect()
}

#[test]
fn a_process_is_one_row_under_the_column_headers() {
    let owned = Owned::listener();
    let (out, _, code) = run(plain_cmd(), &["list", "--in", &owned.raw_dir()]);
    assert_eq!(code, 0, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with("✓ Found 1 process in "), "{out}");
    assert_eq!(lines[1], "", "{out}");

    let header = cells(lines[2]);
    // Terminals narrower than 100 columns have no UPTIME column
    let expected: Vec<&str> = if header.contains(&"UPTIME") {
        vec![
            "PID", "DIR", "NAME", "ARGS", "CPU%", "MEM", "UPTIME", "STATUS",
        ]
    } else {
        vec!["PID", "DIR", "NAME", "ARGS", "CPU%", "MEM", "STATUS"]
    };
    assert_eq!(header, expected, "{out}");

    let row = cells(lines[3]);
    assert_eq!(row[0], owned.pid().to_string(), "{out}");
    assert!(owned.name_cell_matches(row[2]), "{} in {out}", row[2]);
    assert_eq!(row[3], "-", "no arguments: {out}");
    assert!(
        matches!(*row.last().unwrap(), "Running" | "Sleeping"),
        "{out}"
    );
    assert_eq!(lines.len(), 5, "headline, blank, header, row, blank: {out}");
    assert_eq!(lines[4], "", "{out}");
}

#[test]
fn verbose_lists_the_details_under_each_process() {
    let owned = Owned::listener();
    let (out, _, code) = run(
        plain_cmd(),
        &["list", "--in", &owned.raw_dir(), "--verbose"],
    );
    assert_eq!(code, 0, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    let first = lines
        .iter()
        .position(|l| l.starts_with(&owned.pid().to_string()))
        .unwrap_or_else(|| panic!("no process line:\n{out}"));
    let head = regex::Regex::new(&format!(
        r"^{} {} \[(Running|Sleeping)\]  \d+\.\d% CPU  \S+  .+$",
        owned.pid(),
        regex::escape(&owned.shown_name())
    ))
    .unwrap();
    assert!(head.is_match(lines[first]), "{}\n{out}", lines[first]);
    let details: Vec<&str> = lines[first + 1..]
        .iter()
        .take_while(|l| l.starts_with("    "))
        .copied()
        .collect();
    let labels: Vec<&str> = details
        .iter()
        .map(|l| l.trim_start().split(':').next().unwrap())
        .collect();
    assert_eq!(
        labels,
        ["cmd", "exe", "cwd", "parent", "uptime"],
        "{out}\n{}",
        owned.what_proc_saw()
    );
    assert!(
        details[3] == format!("    parent: {}", std::process::id()),
        "the test started it: {out}"
    );
    // A blank line closes each process, and one more ends the list
    assert_eq!(&lines[first + 6..], ["", ""], "{out}");
}

#[test]
fn a_dry_run_shows_the_table_then_the_summary() {
    let owned = Owned::listener();
    let pid = owned.pid().to_string();
    let (out, err, code) = run(plain_cmd(), &["kill", &pid, "--dry-run"]);
    assert_eq!(code, 0, "{out}{err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "✓ Found 1 process");
    assert_eq!(cells(lines[3])[0], pid, "{out}");
    assert_eq!(*lines.last().unwrap(), "⚠ Dry run: would kill 1 process");
    // Nothing was killed
    let (alive, _, _) = run(plain_cmd(), &["info", &pid, "--json"]);
    assert!(alive.contains(&format!("\"pid\": {pid}")), "{alive}");
}

// ---- the terminal's width (Unix: the width comes from the terminal itself) ----

#[cfg(unix)]
mod width {
    use super::*;
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    use std::io::Read;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// Run `proc <args>` in a pseudo-terminal `cols` wide and return everything
    /// it printed, colour codes removed. Killed and reaped on every path.
    fn in_terminal(cols: u16, args: &[&str]) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 40,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("open a pseudo-terminal");
        let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_proc"));
        cmd.args(args);
        cmd.env("TERM", "xterm-256color");
        cmd.env_remove("NO_COLOR");
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .expect("spawn proc in the pty");
        // Our copy of the child's end stays open until everything is read:
        // when the last copy closes, macOS and NetBSD throw away output not
        // yet read (terminal.rs, `finish_reading`)
        let slave = pair.slave;

        let mut reader = pair.master.try_clone_reader().expect("pty reader");
        let captured = Arc::new(Mutex::new(Vec::<u8>::new()));
        let sink = Arc::clone(&captured);
        let reading = std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            // Linux reports the closed terminal as an error (EIO), macOS as 0 bytes
            while let Ok(n) = reader.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                sink.lock().unwrap().extend_from_slice(&chunk[..n]);
            }
        });
        let text = || String::from_utf8_lossy(&captured.lock().unwrap()).into_owned();
        let finished = loop {
            if let Some(status) = child.try_wait().expect("poll the child") {
                break Some(status);
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        // Whatever it printed last is still in the pipe: wait until the reader
        // has taken it all, then close our copy so the reader reaches the end
        let drained = Instant::now() + Duration::from_secs(2);
        let fd = pair.master.as_raw_fd().expect("the pty's descriptor");
        let unread = || {
            let mut waiting: libc::c_int = 0;
            // SAFETY: FIONREAD writes one int, into `waiting`
            let ok = unsafe { libc::ioctl(fd, libc::FIONREAD as _, &mut waiting) } == 0;
            if ok {
                waiting
            } else {
                0
            }
        };
        while unread() > 0 && Instant::now() < drained {
            std::thread::sleep(Duration::from_millis(5));
        }
        drop(slave);
        while !reading.is_finished() && Instant::now() < drained {
            std::thread::sleep(Duration::from_millis(5));
        }
        let shown = text();
        let status =
            finished.unwrap_or_else(|| panic!("proc did not exit; it printed:\n{shown:?}"));
        assert_eq!(status.exit_code(), 0, "{shown:?}");
        strip_colours(&shown).replace("\r\n", "\n")
    }

    fn header_of(out: &str) -> Vec<String> {
        out.lines()
            .find(|l| l.contains("PID") && l.contains("STATUS"))
            .unwrap_or_else(|| panic!("no table header in:\n{out}"))
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn a_terminal_under_100_columns_has_no_uptime_column() {
        let owned = Owned::listener();
        let out = in_terminal(80, &["list", "--in", &owned.raw_dir()]);
        assert_eq!(
            header_of(&out),
            ["PID", "DIR", "NAME", "ARGS", "CPU%", "MEM", "STATUS"],
            "{out}"
        );
        // The table, not the headline above it (which holds the whole path)
        let table = out.lines().skip_while(|l| !l.contains("STATUS"));
        for line in table {
            assert!(line.chars().count() < 80, "{line:?} overflows 80 columns");
        }
    }

    #[test]
    fn a_wider_terminal_adds_the_uptime_column() {
        let owned = Owned::listener();
        let out = in_terminal(140, &["list", "--in", &owned.raw_dir()]);
        assert_eq!(
            header_of(&out),
            ["PID", "DIR", "NAME", "ARGS", "CPU%", "MEM", "UPTIME", "STATUS"],
            "{out}"
        );
    }
}
