//! Hostile output: what every command does when stdout or stderr cannot take
//! its bytes.
//!
//! `println!` panics when a write fails, and Rust ignores SIGPIPE, so a reader
//! that stopped early (`proc list | head -n1`) used to end proc with a panic
//! and exit 101, after a `kill --yes` had already killed its target. The rule
//! (PHILOSOPHY.md: "a reader that stops early ends proc quietly") is:
//!
//! - a closed pipe on **stdout**: proc stops writing and exits with the code
//!   its own result calls for, the code it gives with a normal stdout. Nothing
//!   is said about the pipe;
//! - a closed pipe on **stderr**: ignored, the exit code stays;
//! - any other write error (`/dev/full`, Linux only): a failure, exit 1, with
//!   one line on stderr; a command that failed on its own keeps its code;
//! - `--help`, `--version`: a closed pipe is exit 0, quietly.
//!
//! The matrix runs every command, in human and `--json` output, plus the
//! orientation screen, help, version, completions and the man page, against
//! each sink, and compares with the same run on a normal stdout. Every
//! destructive row aims at a process the test started (named by PID or port) or
//! at a fresh temp directory. A closed pipe is made by dropping the read end of
//! an anonymous pipe before proc starts, which works the same way on every OS
//! (Windows has no SIGPIPE, but a write to a pipe nobody reads fails there too,
//! and Rust reports it as the same `BrokenPipe`). `/dev/full` exists only on
//! Linux.

mod built;

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A PID no OS hands out
const MISSING_PID: &str = "99999999";

/// One way of running proc: its arguments (placeholders filled in per run) and
/// whether `--json` can be added
struct Row {
    args: &'static [&'static str],
    json: bool,
}

const fn row(args: &'static [&'static str]) -> Row {
    Row { args, json: true }
}

/// A row with only human output (help, completions, the man page)
const fn plain(args: &'static [&'static str]) -> Row {
    Row { args, json: false }
}

/// Every command, with safe arguments: `--dry-run`, a process the test
/// started, `--in <fresh temp dir>`, or a PID no machine has.
///
/// Placeholders: `{pid}` and `{port}` of the process the test started, `{dir}`
/// its working directory (a fresh temp dir), `{exe}` its program, `{name}` its
/// name.
#[rustfmt::skip]
const ROWS: &[Row] = &[
    // bare proc, help, version, completions, man page
    plain(&[]),
    plain(&["--help"]),
    plain(&["-h"]),
    plain(&["--version"]),
    plain(&["list", "--help"]),
    plain(&["completions", "bash"]),
    plain(&["completions", "zsh"]),
    plain(&["completions", "fish"]),
    plain(&["completions", "elvish"]),
    plain(&["completions", "powershell"]),
    plain(&["manpage"]),
    // usage errors: stderr (human) or stdout (--json)
    row(&["list", "--bogus"]),
    // on, for, by, in, list, info, ports, tree, why: reading commands
    row(&["on", ":{port}"]),
    row(&["on", "{pid}"]),
    row(&["on", "{pid}", ":{port}"]),
    row(&["on", MISSING_PID]),
    row(&["on", "{pid},:{port}"]),
    row(&["for", "{exe}"]),
    row(&["by", "{name}", "--in", "{dir}"]),
    row(&["in", "{dir}"]),
    row(&["list", "--in", "{dir}"]),
    row(&["list", "--in", "{dir}", "--verbose"]),
    row(&["info", "{pid}"]),
    row(&["ports", "--in", "{dir}"]),
    row(&["tree", "{pid}"]),
    row(&["tree", "{pid}", "--ancestors"]),
    // a snapshot table; its --json form streams forever and has its own tests below
    plain(&["watch", "{pid}"]),
    row(&["why", "{pid}"]),
    row(&["why", ":{port}"]),
    row(&["stuck", "--in", "{dir}"]),
    row(&["stuck", "--kill", "--dry-run", "--in", "{dir}"]),
    // the test's own process is neither stuck nor an orphan: nothing to kill
    row(&["stuck", "--kill", "--yes", "--in", "{dir}"]),
    row(&["orphans", "--in", "{dir}"]),
    row(&["orphans", "--kill", "--yes", "--in", "{dir}"]),
    // commands that change processes: the exit code is about the action
    row(&["kill", "{pid}", "--dry-run"]),
    row(&["kill", "{pid}", "--yes"]),
    row(&["kill", "{pid}", MISSING_PID, "--yes"]),
    row(&["kill", MISSING_PID, "--yes"]),
    row(&["kill", "{pid}"]),
    row(&["stop", "{pid}", "--dry-run"]),
    row(&["stop", "{pid}", "--yes", "--timeout", "2"]),
    row(&["freeze", "{pid}", "--yes"]),
    row(&["freeze", "{pid}", "--dry-run"]),
    row(&["thaw", "{pid}", "--yes"]),
    row(&["unstick", "{pid}", "--dry-run"]),
    row(&["free", ":{port}", "--dry-run"]),
    row(&["free", ":{port}", "--yes"]),
    // wait: the process never exits, so it times out
    row(&["wait", "{pid}", "--timeout", "1"]),
];

// ---- the process the test aims proc at ----

/// The listener example, started with its cwd in a fresh temp directory.
/// Killed and reaped on drop, whatever a case did to it.
struct Fixture {
    child: Child,
    port: u16,
    /// Keeps the directory alive; `dir` is its path as processes report it
    _tmp: tempfile::TempDir,
    dir: PathBuf,
}

impl Fixture {
    fn start() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        // A process reports its cwd with symlinks resolved (/var is /private/var
        // on macOS), and `--in` compares paths literally
        #[cfg(unix)]
        let dir = tmp.path().canonicalize().unwrap();
        #[cfg(not(unix))]
        let dir = tmp.path().to_path_buf();
        let mut child = Command::new(listener_path())
            .current_dir(&dir)
            .stdout(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .expect("spawn the listener example");
        let mut line = String::new();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let read = stdout.read_line(&mut line);
        let port = read
            .ok()
            .and_then(|_| line.trim().strip_prefix("PORT "))
            .and_then(|p| p.parse().ok());
        match port {
            Some(port) => Fixture {
                child,
                port,
                _tmp: tmp,
                dir,
            },
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("the listener printed {line:?} instead of `PORT <n>`");
            }
        }
    }

    fn fill(&self, arg: &str) -> String {
        arg.replace("{pid}", &self.child.id().to_string())
            .replace("{port}", &self.port.to_string())
            .replace("{dir}", &self.dir.to_string_lossy())
            .replace("{exe}", &listener_path().to_string_lossy())
            .replace("{name}", "listener")
    }

    /// Whether the process is gone (and reaped), waiting up to 10 seconds
    fn gone_within_10s(&mut self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `target/debug/examples/listener[.exe]`
fn listener_path() -> PathBuf {
    built::example("listener")
}

fn needs_fixture(args: &[&str]) -> bool {
    args.iter().any(|a| {
        ["{pid}", "{port}", "{dir}", "{exe}", "{name}"]
            .iter()
            .any(|p| a.contains(p))
    })
}

fn filled(args: &[&str], json: bool, fixture: Option<&Fixture>) -> Vec<String> {
    let mut argv: Vec<String> = args
        .iter()
        .map(|a| match fixture {
            Some(f) => f.fill(a),
            None => a.to_string(),
        })
        .collect();
    if json {
        argv.push("--json".to_string());
    }
    argv
}

// ---- running proc against a sink ----

/// What stdout or stderr is connected to
#[derive(Clone, Copy, Debug, PartialEq)]
enum Sink {
    /// A pipe the test reads
    Capture,
    /// A pipe whose read end was closed before proc started
    ClosedPipe,
    /// `/dev/full`: every write fails with ENOSPC
    #[cfg(target_os = "linux")]
    Full,
}

impl Sink {
    fn stdio(self) -> Stdio {
        match self {
            Sink::Capture => Stdio::piped(),
            Sink::ClosedPipe => {
                let (reader, writer) = std::io::pipe().expect("make a pipe");
                drop(reader);
                Stdio::from(writer)
            }
            #[cfg(target_os = "linux")]
            Sink::Full => Stdio::from(
                std::fs::OpenOptions::new()
                    .write(true)
                    .open("/dev/full")
                    .expect("open /dev/full"),
            ),
        }
    }
}

struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Run `proc <argv>` with stdout and stderr connected to the sinks, and wait
/// for it (a minute at most). What a captured stream held is returned; a
/// stream that was not captured comes back empty.
fn run_proc(argv: &[String], stdout: Sink, stderr: Sink) -> Ran {
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("proc"))
        .args(argv)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(stdout.stdio())
        .stderr(stderr.stdio())
        .spawn()
        .expect("spawn proc");
    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .map(|stream| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            if let Some(mut stream) = stream {
                let _ = stream.read_to_end(&mut text);
            }
            String::from_utf8_lossy(&text).into_owned()
        })
    })
    .collect();

    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        match child.try_wait().expect("wait for proc") {
            Some(status) => break Some(status),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let mut texts = readers.into_iter().map(|r| r.join().unwrap());
    let (stdout, stderr) = (texts.next().unwrap(), texts.next().unwrap());
    assert!(
        status.is_some(),
        "proc {argv:?} did not finish in a minute\nstdout: {stdout}\nstderr: {stderr}"
    );
    Ran {
        code: status.and_then(|s| s.code()),
        stdout,
        stderr,
    }
}

/// Run one row in one output mode against the sinks, on a fresh process of
/// the test's own. The process's directory and program read `{dir}` and
/// `{exe}` in the text, as in the row, so two runs can be compared.
fn run_row(row_args: &[&str], json: bool, stdout: Sink, stderr: Sink) -> (Ran, Vec<String>) {
    let fixture = needs_fixture(row_args).then(Fixture::start);
    let argv = filled(row_args, json, fixture.as_ref());
    let mut ran = run_proc(&argv, stdout, stderr);
    if let Some(fixture) = &fixture {
        for text in [&mut ran.stdout, &mut ran.stderr] {
            *text = text
                .replace(&*fixture.dir.to_string_lossy(), "{dir}")
                .replace(&*listener_path().to_string_lossy(), "{exe}");
        }
    }
    (ran, argv)
}

/// The text with every number read as `N` and every run of spaces as one:
/// PIDs, ports, times and the column widths they set differ from one run to
/// the next
fn without_numbers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev = None;
    for c in text.chars() {
        let class = if c.is_ascii_digit() { '0' } else { c };
        if class != prev.unwrap_or('\0') || !matches!(class, '0' | ' ') {
            out.push(if class == '0' { 'N' } else { c });
        }
        prev = Some(class);
    }
    out
}

/// Strings that mean a write error got out as a panic or noise
fn noise_in(stderr: &str) -> Option<&'static str> {
    [
        "panicked",
        "RUST_BACKTRACE",
        "Broken pipe",
        "os error",
        "BrokenPipe",
    ]
    .into_iter()
    .find(|bad| stderr.contains(bad))
}

/// The one row of a failure report
fn shown(argv: &[String]) -> String {
    format!("proc {}", argv.join(" "))
}

/// Run `check` on every (row, mode), four at a time, and fail with every
/// problem found
fn for_every_case(check: impl Fn(&Row, bool) -> Result<(), String> + Sync) {
    let cases: Vec<(&Row, bool)> = ROWS
        .iter()
        .flat_map(|r| {
            let modes: &[bool] = if r.json { &[false, true] } else { &[false] };
            modes.iter().map(move |&json| (r, json))
        })
        .collect();
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::<(usize, String)>::new());
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some((row, json)) = cases.get(i) else {
                    break;
                };
                if let Err(why) = check(row, *json) {
                    failures.lock().unwrap().push((i, why));
                }
            });
        }
    });
    let mut failures = failures.into_inner().unwrap();
    failures.sort();
    let report: Vec<String> = failures.into_iter().map(|(_, why)| why).collect();
    assert!(
        report.is_empty(),
        "{} of {} cases failed:\n\n{}",
        report.len(),
        cases.len(),
        report.join("\n\n")
    );
}

#[test]
fn a_closed_stdout_gives_the_exit_code_a_normal_stdout_gives() {
    for_every_case(|row, json| {
        let (normal, argv) = run_row(row.args, json, Sink::Capture, Sink::Capture);
        let (closed, hostile_argv) = run_row(row.args, json, Sink::ClosedPipe, Sink::Capture);
        let (name, hostile_name) = (shown(&argv), shown(&hostile_argv));
        let code = closed
            .code
            .ok_or(format!("{hostile_name}: killed by a signal"))?;
        if let Some(bad) = noise_in(&closed.stderr) {
            return Err(format!(
                "{hostile_name}: stderr has {bad:?}:\n{}",
                closed.stderr
            ));
        }
        if !(0..=4).contains(&code) || Some(code) != normal.code {
            return Err(format!(
                "{hostile_name}: exit {code} with a closed stdout, {:?} with a normal one \
                 ({name})\nstderr: {}",
                normal.code, closed.stderr
            ));
        }
        // nothing is said about the pipe: stderr holds what it holds anyway
        if without_numbers(&closed.stderr) != without_numbers(&normal.stderr) {
            return Err(format!(
                "{hostile_name}: stderr differs from a normal stdout's ({name})\n\
                 closed: {:?}\nnormal: {:?}",
                closed.stderr, normal.stderr
            ));
        }
        Ok(())
    });
}

#[test]
fn both_streams_closed_change_no_exit_code() {
    for_every_case(|row, json| {
        let (normal, argv) = run_row(row.args, json, Sink::Capture, Sink::Capture);
        let (closed, hostile_argv) = run_row(row.args, json, Sink::ClosedPipe, Sink::ClosedPipe);
        let hostile_name = shown(&hostile_argv);
        let code = closed
            .code
            .ok_or(format!("{hostile_name}: killed by a signal"))?;
        if !(0..=4).contains(&code) || Some(code) != normal.code {
            return Err(format!(
                "{hostile_name}: exit {code} with both streams closed, {:?} with normal ones \
                 ({})",
                normal.code,
                shown(&argv)
            ));
        }
        Ok(())
    });
}

#[test]
fn a_closed_stderr_changes_no_exit_code() {
    for_every_case(|row, json| {
        let (normal, argv) = run_row(row.args, json, Sink::Capture, Sink::Capture);
        let (closed, hostile_argv) = run_row(row.args, json, Sink::Capture, Sink::ClosedPipe);
        let hostile_name = shown(&hostile_argv);
        let code = closed
            .code
            .ok_or(format!("{hostile_name}: killed by a signal"))?;
        // stderr is gone, so a panic can only be seen as its exit code, 101
        if !(0..=4).contains(&code) || Some(code) != normal.code {
            return Err(format!(
                "{hostile_name}: exit {code} with a closed stderr, {:?} with a normal one ({})",
                normal.code,
                shown(&argv)
            ));
        }
        // what goes to stdout is the same either way
        if closed.stdout.is_empty() != normal.stdout.is_empty() {
            return Err(format!(
                "{hostile_name}: stdout changed with a closed stderr"
            ));
        }
        Ok(())
    });
}

/// `/dev/full` takes no write: every row fails the way a full disk does
#[cfg(target_os = "linux")]
#[test]
fn a_stdout_that_cannot_take_a_write_is_exit_1_with_a_message() {
    for_every_case(|row, json| {
        let (normal, argv) = run_row(row.args, json, Sink::Capture, Sink::Capture);
        let (full, hostile_argv) = run_row(row.args, json, Sink::Full, Sink::Capture);
        let hostile_name = shown(&hostile_argv);
        let code = full
            .code
            .ok_or(format!("{hostile_name}: killed by a signal"))?;
        if full.stderr.contains("panicked") {
            return Err(format!("{hostile_name}: panicked:\n{}", full.stderr));
        }
        // a failed write turns a success into exit 1; a command that failed
        // on its own keeps its code. A row that printed nothing on stdout
        // before has nothing to fail.
        let wrote = !normal.stdout.is_empty();
        let want = match normal.code {
            Some(0) if wrote => 1,
            Some(code) => code,
            None => return Err(format!("{}: killed by a signal", shown(&argv))),
        };
        if code != want {
            return Err(format!(
                "{hostile_name}: exit {code} on /dev/full, expected {want} \
                 (a normal stdout gives {:?})\nstderr: {}",
                normal.code, full.stderr
            ));
        }
        if want == 1 && normal.code == Some(0) {
            let said = full
                .stderr
                .lines()
                .filter(|l| l.starts_with("proc: cannot write to stdout: "))
                .count();
            if said != 1 {
                return Err(format!(
                    "{hostile_name}: expected one `proc: cannot write to stdout: ...` line, \
                     got {:?}",
                    full.stderr
                ));
            }
        }
        Ok(())
    });
}

/// A stderr that takes no write is a failure too, though there is nowhere left
/// to say so: a row that wrote to stderr and exited 0 exits 1, quietly. A row
/// that wrote nothing there, or failed on its own, keeps its code.
#[cfg(target_os = "linux")]
#[test]
fn a_stderr_that_cannot_take_a_write_is_exit_1() {
    for_every_case(|row, json| {
        let (normal, argv) = run_row(row.args, json, Sink::Capture, Sink::Capture);
        let (full, hostile_argv) = run_row(row.args, json, Sink::Capture, Sink::Full);
        let hostile_name = shown(&hostile_argv);
        let code = full
            .code
            .ok_or(format!("{hostile_name}: killed by a signal"))?;
        let want = match normal.code {
            Some(0) if !normal.stderr.is_empty() => 1,
            Some(code) => code,
            None => return Err(format!("{}: killed by a signal", shown(&argv))),
        };
        if code != want {
            return Err(format!(
                "{hostile_name}: exit {code} with stderr on /dev/full, expected {want} \
                 (a normal stderr gives {:?}: {:?})",
                normal.code, normal.stderr
            ));
        }
        // what goes to stdout is there either way (not the same text: `for
        // {exe}` lists every listener the other cases have running)
        if full.stdout.is_empty() != normal.stdout.is_empty() {
            return Err(format!(
                "{hostile_name}: stdout changed with stderr on /dev/full\n\
                 full: {:?}\nnormal: {:?}",
                full.stdout, normal.stdout
            ));
        }
        Ok(())
    });
}

// ---- the worst case: the action happens, then the report cannot be read ----

#[test]
fn kill_stop_and_free_act_and_exit_0_when_the_reader_is_gone() {
    for (command, extra) in [
        ("kill", vec![]),
        ("stop", vec!["--timeout", "2"]),
        ("free", vec![]),
    ] {
        for json in [false, true] {
            let mut fixture = Fixture::start();
            let target = if command == "free" {
                format!(":{}", fixture.port)
            } else {
                fixture.child.id().to_string()
            };
            let mut argv = vec![command.to_string(), target, "--yes".to_string()];
            argv.extend(extra.iter().map(|s| s.to_string()));
            if json {
                argv.push("--json".to_string());
            }
            let ran = run_proc(&argv, Sink::ClosedPipe, Sink::Capture);
            let name = shown(&argv);
            assert_eq!(ran.code, Some(0), "{name}: stderr: {}", ran.stderr);
            assert_eq!(ran.stderr, "", "{name} said something about the pipe");
            assert!(
                fixture.gone_within_10s(),
                "{name} reported success but the process is still running"
            );
        }
    }
}

#[test]
fn a_target_that_is_not_found_is_still_exit_2_when_the_reader_is_gone() {
    for json in [false, true] {
        let mut argv = vec!["on".to_string(), MISSING_PID.to_string()];
        if json {
            argv.push("--json".to_string());
        }
        let ran = run_proc(&argv, Sink::ClosedPipe, Sink::Capture);
        assert_eq!(
            ran.code,
            Some(2),
            "{}: stderr: {}",
            shown(&argv),
            ran.stderr
        );
        assert_eq!(noise_in(&ran.stderr), None, "{}", ran.stderr);
    }
}

#[test]
fn help_and_version_end_quietly_when_the_reader_is_gone() {
    for args in [["--help"], ["-h"], ["--version"]] {
        let argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ran = run_proc(&argv, Sink::ClosedPipe, Sink::Capture);
        assert_eq!(ran.code, Some(0), "{args:?}: stderr: {}", ran.stderr);
        assert_eq!(ran.stderr, "", "{args:?}");
    }
}

#[test]
fn a_stream_of_json_frames_ends_when_the_reader_is_gone() {
    let fixture = Fixture::start();
    let argv = vec![
        "watch".to_string(),
        fixture.child.id().to_string(),
        "--json".to_string(),
    ];
    let ran = run_proc(&argv, Sink::ClosedPipe, Sink::Capture);
    assert_eq!(ran.code, Some(0), "stderr: {}", ran.stderr);
    assert_eq!(noise_in(&ran.stderr), None, "{}", ran.stderr);
}

#[cfg(target_os = "linux")]
#[test]
fn a_stream_of_json_frames_fails_when_stdout_cannot_take_a_write() {
    let fixture = Fixture::start();
    let argv = vec![
        "watch".to_string(),
        fixture.child.id().to_string(),
        "--json".to_string(),
    ];
    let ran = run_proc(&argv, Sink::Full, Sink::Capture);
    assert_eq!(ran.code, Some(1), "stderr: {}", ran.stderr);
    assert!(
        ran.stderr.starts_with("proc: cannot write to stdout: "),
        "{}",
        ran.stderr
    );
}
