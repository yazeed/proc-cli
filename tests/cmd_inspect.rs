//! `why`, `info` and the CLI entry point (`main`), driven through the real binary.
//!
//! Every process these tests look at is one the test started: a copy of the
//! listener example (`examples/listener.rs`) under a name and path of its own,
//! in a fresh temp directory, or (on Unix) the listener run by a `sh` the test
//! owns, so the listener has a parent worth tracing. Each is killed and reaped
//! on drop. The tests only read: nothing here kills, stops or frees a process.

mod contract;

use assert_cmd::Command;
use clap::CommandFactory;
use proc_cli::cli::Cli;
use proc_cli::core::filters::strip_verbatim_prefix;
#[cfg(target_os = "macos")]
use proc_cli::core::Process;
use serde_json::Value;
#[cfg(unix)]
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// No OS hands out a PID this high
const MISSING_PID: &str = "99999999";

fn proc_cmd() -> Command {
    let mut cmd = Command::cargo_bin("proc").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd
}

/// Run proc and parse stdout as exactly one JSON document, with the exit code.
fn run_json(args: &[&str]) -> (Value, i32) {
    let output = proc_cmd().args(args).output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout is not a single JSON document ({e}):\n{stdout}"));
    let code = output.status.code().unwrap();
    contract::check(&json, code, args);
    (json, code)
}

/// Run proc and return its stdout, stderr and exit code.
fn run_text(args: &[&str]) -> (String, String, i32) {
    let output = proc_cmd().args(args).output().unwrap();
    (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
        output.status.code().unwrap(),
    )
}

/// `target/debug/examples/listener`, found relative to this test binary
/// (`target/debug/deps/cmd_inspect-<hash>`)
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

/// A process name no process has, for this call alone: the other tests run
/// proc at the same time, so a name two tests share would match the other
/// test's process (with `--cmd`, any process whose command line has it).
fn nobody() -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    // Ends in `x`, so no name is the start of another (`-1x` in `-10x`)
    format!("nobody-{}-{n}x", std::process::id())
}

/// A port nothing listens on, from below every OS's ephemeral range (Linux
/// starts at 32768, macOS and Windows at 49152), so a listener another test
/// binds to port 0 can never be handed it. A port the OS just handed out and
/// took back could be: on CI a parallel test's listener got one between the
/// lookup and proc's run
fn free_port() -> u16 {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let start = 20_000 + std::process::id() as usize % 10_000;
    loop {
        let port = start + NEXT.fetch_add(1, Ordering::SeqCst);
        assert!(port < 32_768, "no unused port below the ephemeral range");
        if std::net::TcpListener::bind(("0.0.0.0", port as u16)).is_ok() {
            return port as u16;
        }
    }
}

/// Run the copy, with its cwd in `dir`. On Linux a file another thread has
/// just written can be "busy" (ETXTBSY) for an instant: a fork in a parallel
/// test still holds the write handle until it execs. That clears in
/// milliseconds, so try again.
fn spawn_copy(copy: &Path, args: &[&str], dir: &Path) -> Child {
    let mut attempts = 0;
    loop {
        let spawned = std::process::Command::new(copy)
            .args(args)
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

/// Whether two paths name the same place: compared canonicalized, because a
/// process reports its cwd and executable with symlinks resolved (macOS
/// `/private/var/..`) and, on Windows, as the long name (not 8.3 short).
fn same_place(a: &str, b: &str) -> bool {
    let canon = |p: &str| {
        let path = std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p));
        strip_verbatim_prefix(&path.to_string_lossy()).to_string()
    };
    canon(a) == canon(b)
}

/// A listener the test owns: a copy of the listener example, run with its cwd
/// in a fresh temp directory. Killed and reaped on drop.
struct Owned {
    child: Child,
    /// A name no other process has, short enough for any OS to report whole
    name: String,
    port: u16,
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
        Self::listener_with_args(&[])
    }

    /// A listener started with extra arguments (it ignores them, so they only
    /// put words in its command line)
    fn listener_with_args(args: &[&str]) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let raw_dir = tmp.path().to_path_buf();
        let dir = raw_dir.canonicalize().unwrap();
        let dir = PathBuf::from(strip_verbatim_prefix(&dir.to_string_lossy()));
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let name = format!("pci{}n{n}x", std::process::id());
        let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        std::fs::copy(listener_path(), raw_dir.join(&file)).expect("copy the listener example");
        // Started by its canonical path, so the executable path processes
        // report is that path on every OS; the cwd is the path as the user
        // would type it
        let mut child = spawn_copy(&dir.join(&file), args, &raw_dir);
        let mut line = String::new();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let read = stdout.read_line(&mut line);
        let port = read
            .ok()
            .and_then(|_| line.trim().strip_prefix("PORT "))
            .and_then(|p| p.parse().ok());
        let Some(port) = port else {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the listener printed {line:?} instead of `PORT <n>`");
        };
        Owned {
            child,
            name,
            port,
            _tmp: tmp,
            dir,
            raw_dir,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn pid_arg(&self) -> String {
        self.pid().to_string()
    }

    fn port_arg(&self) -> String {
        format!(":{}", self.port)
    }

    fn dir(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }

    fn raw_dir(&self) -> String {
        self.raw_dir.to_string_lossy().into_owned()
    }

    /// The file name of the executable copy (`.exe` on Windows, as processes
    /// report it)
    fn file_name(&self) -> String {
        format!("{}{}", self.name, std::env::consts::EXE_SUFFIX)
    }

    /// The path of the executable copy
    fn exe(&self) -> String {
        self.dir
            .join(self.file_name())
            .to_string_lossy()
            .into_owned()
    }

    /// What proc itself reports for this process (cwd, exe_path), for a
    /// failure message
    fn what_proc_saw(&self) -> String {
        let out = proc_cmd()
            .args(["info", &self.pid_arg(), "--json"])
            .output()
            .unwrap();
        format!(
            "proc info {} --json (raw dir {}, canonical dir {}):\n{}{}",
            self.pid(),
            self.raw_dir.display(),
            self.dir.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A listener whose parent is a `sh` the test owns: `sh` starts the listener in
/// the background and waits for it, so the chain is `... -> test -> sh ->
/// listener`. The script comes in on stdin, so `sh` has no arguments (and no
/// command summary in the tree). Killing the listener (by the PID `sh`
/// printed) ends `sh`.
#[cfg(unix)]
struct Parented {
    sh: Child,
    listener_pid: u32,
    port: u16,
    _tmp: tempfile::TempDir,
}

#[cfg(unix)]
impl Parented {
    fn start() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let listener = listener_path();
        let listener = listener.to_str().unwrap();
        assert!(!listener.contains('\''), "cannot quote {listener}");
        let mut sh = std::process::Command::new("sh")
            .current_dir(tmp.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn sh");
        // Dropping stdin after the script ends sh once the listener has
        let mut stdin = sh.stdin.take().unwrap();
        writeln!(stdin, "'{listener}' & echo CHILD $!; wait").expect("write the script to sh");
        drop(stdin);
        let stdout = BufReader::new(sh.stdout.take().unwrap());
        let (mut listener_pid, mut port) = (None, None);
        // `sh` prints CHILD <pid>, the listener prints PORT <n>, in either order
        for line in stdout.lines().take(2) {
            let line = line.unwrap_or_default();
            if let Some(pid) = line.strip_prefix("CHILD ") {
                listener_pid = pid.trim().parse().ok();
            } else if let Some(p) = line.strip_prefix("PORT ") {
                port = p.trim().parse().ok();
            }
        }
        let (Some(listener_pid), Some(port)) = (listener_pid, port) else {
            let _ = sh.kill();
            let _ = sh.wait();
            panic!("sh printed no `CHILD <pid>` and `PORT <n>`");
        };
        Parented {
            sh,
            listener_pid,
            port,
            _tmp: tmp,
        }
    }

    fn sh_pid(&self) -> u32 {
        self.sh.id()
    }
}

#[cfg(unix)]
impl Drop for Parented {
    fn drop(&mut self) {
        // SAFETY: signalling the one PID `sh` printed for the listener this
        // struct started; the signal has no memory effects.
        unsafe {
            libc::kill(self.listener_pid as libc::pid_t, libc::SIGKILL);
        }
        let _ = self.sh.kill();
        let _ = self.sh.wait();
    }
}

/// The first line of `text` that holds `needle`
fn line_with<'a>(text: &'a str, needle: &str) -> &'a str {
    text.lines()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no line holds {needle:?}:\n{text}"))
}

/// The line `why` marks as the target. Matched on how the line ends, because
/// the tree also prints the command lines of the test's ancestors (a shell
/// whose command line holds any text at all).
fn target_line(text: &str) -> &str {
    text.lines()
        .find(|l| l.ends_with("← target"))
        .unwrap_or_else(|| panic!("no target line in:\n{text}"))
}

/// How many spaces a line starts with
fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

// --- why: human output -------------------------------------------------------

#[test]
fn why_a_port_names_the_port_and_marks_the_process_holding_it() {
    let owned = Owned::listener();
    // The answer comes from several snapshots of the machine (listening ports,
    // process list, the cwd read), so give a just-started process a moment to
    // show up in all of them
    let header = format!("✓ Port {} (TCP):\n\n", owned.port);
    let (mut out, mut stderr, mut code) = (String::new(), String::new(), -1);
    for _ in 0..50 {
        (out, stderr, code) = run_text(&["why", &owned.port_arg()]);
        let has_dir = out
            .lines()
            .any(|l| l.trim_start().starts_with("Directory:"));
        if out.starts_with(&header) && has_dir {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(code, 0, "{out}{stderr}\n{}", owned.what_proc_saw());
    assert_eq!(stderr, "");
    assert!(out.starts_with(&header), "{out}\n{}", owned.what_proc_saw());
    let target = target_line(&out);
    assert!(
        target.contains(&format!("{} [{}]", owned.file_name(), owned.pid())),
        "{target}\n{}",
        owned.what_proc_saw()
    );
    assert!(target.contains("└── "), "{target}");
    // The listener is run without arguments, so nothing but the uptime sits
    // between its PID and the arrow
    let after_pid = target.split_once(&format!("[{}]", owned.pid())).unwrap().1;
    assert!(
        after_pid.trim_end_matches("← target").trim().is_empty()
            || after_pid.trim_start().starts_with("up "),
        "{target}"
    );
    // The directory it runs in is the card's, and the chain has no line of its own for it
    assert!(
        !out.lines().any(|l| l.trim_start().starts_with("dir: ")),
        "{out}"
    );
    let dir_line = out
        .lines()
        .find(|l| l.trim_start().starts_with("Directory:"))
        .unwrap_or_else(|| panic!("no Directory: in:\n{out}"));
    // Windows ends a working directory with `\`
    let dir = dir_line
        .trim()
        .strip_prefix("Directory:")
        .unwrap()
        .trim()
        .trim_end_matches(['\\', '/']);
    // The card cuts a long path in the middle, so the last part must be the directory's
    let owned_dir = owned.dir();
    let last = Path::new(&owned_dir)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(dir.ends_with(&last), "{dir}\n{}", owned.what_proc_saw());
}

#[test]
fn why_a_pid_names_the_process_and_not_a_port() {
    let owned = Owned::listener();
    let (out, _, code) = run_text(&["why", &owned.pid_arg()]);
    assert_eq!(code, 0);
    assert!(
        out.starts_with(&format!("✓ Ancestry for '{}':\n\n", owned.pid())),
        "{out}"
    );
    assert!(!out.contains("Port "), "{out}");
    assert!(target_line(&out).contains(&owned.file_name()));
}

#[test]
fn why_shows_the_targets_ports() {
    let owned = Owned::listener();
    let (out, _, code) = run_text(&["why", &owned.pid_arg()]);
    assert_eq!(code, 0, "{out}");
    // the ancestry is drawn as before, then the target's card
    assert!(
        out.starts_with(&format!("✓ Ancestry for '{}':\n\n", owned.pid())),
        "{out}"
    );
    let target = target_line(&out);
    let card = out.split_once(&format!("{target}\n")).unwrap().1;
    assert!(
        card.contains(&format!(
            "\n  Process:     {} (PID {})\n",
            owned.file_name(),
            owned.pid()
        )),
        "{out}"
    );
    let listening = card_field(&out, "Listening:");
    assert!(
        listening.starts_with(&format!(":{} TCP", owned.port)),
        "{listening}"
    );
    assert!(
        out.contains(&format!("  Process:     {} (PID", owned.file_name())),
        "{out}"
    );
    assert!(out.ends_with("\n\n"), "{out:?}");

    // by port, the header names the port and the card names the same process
    let (out, _, code) = run_text(&["why", &owned.port_arg()]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.starts_with(&format!("✓ Port {} (TCP):\n\n", owned.port)),
        "{out}"
    );
    assert!(
        card_field(&out, "Process:").ends_with(&format!("(PID {})", owned.pid())),
        "{out}"
    );
}

#[test]
fn why_a_name_traces_every_process_with_that_name() {
    let owned = Owned::listener();
    let (out, _, code) = run_text(&["why", &owned.name]);
    assert_eq!(code, 0);
    assert!(
        out.starts_with(&format!("✓ Ancestry for '{}':\n\n", owned.name)),
        "{out}"
    );
    assert!(target_line(&out).contains(&format!("[{}]", owned.pid())));
}

#[test]
fn why_a_name_is_the_whole_name_and_cmd_widens_it_to_the_command_line() {
    let word = nobody();
    let owned = Owned::listener_with_args(&[&word]);
    let part = &owned.name[..owned.name.len() - 1];

    // a part of the name, or a word of the command line, names nothing
    for text in [part, word.as_str()] {
        let (out, err, code) = run_text(&["why", text]);
        assert_eq!(code, 2, "{text}: {out}{err}");
    }
    let (out, err, _) = run_text(&["why", &word]);
    assert!(
        format!("{out}{err}").contains(&format!(
            "1 process has '{word}' in its command line (add --cmd to match those)"
        )),
        "{out}{err}"
    );

    for text in [part, word.as_str()] {
        let (out, _, code) = run_text(&["why", text, "--cmd"]);
        assert_eq!(code, 0, "{text}: {out}");
        assert!(
            target_line(&out).contains(&format!("[{}]", owned.pid())),
            "{out}"
        );
    }
}

#[test]
fn why_draws_the_test_process_above_the_listener_it_started() {
    let owned = Owned::listener();
    let (out, _, _) = run_text(&["why", &owned.pid_arg()]);
    let me = format!("[{}]", std::process::id());
    let parent = line_with(&out, &me);
    let target = target_line(&out);
    // Parent first, then its child one level in with a connector
    assert!(
        out.find(parent).unwrap() < out.find(target).unwrap(),
        "{out}"
    );
    assert_eq!(indent(target), indent(parent) + 4, "{out}");
    assert!(target.trim_start().starts_with("└── "), "{target}");
    // The parent is not the target, and has no arrow
    assert!(!parent.contains("← target"), "{parent}");
}

#[cfg(unix)]
#[test]
fn why_draws_the_shell_between_the_test_and_the_listener() {
    let parented = Parented::start();
    let (out, _, code) = run_text(&["why", &parented.listener_pid.to_string()]);
    assert_eq!(code, 0, "{out}");
    let sh = line_with(&out, &format!("[{}]", parented.sh_pid()));
    let target = target_line(&out);
    let me = line_with(&out, &format!("[{}]", std::process::id()));
    assert!(
        target.contains(&format!("[{}]", parented.listener_pid)),
        "{target}"
    );
    assert_eq!(indent(sh), indent(me) + 4, "{out}");
    assert_eq!(indent(target), indent(sh) + 4, "{out}");
    // Root first, target last
    assert!(out.find(me).unwrap() < out.find(sh).unwrap(), "{out}");
    assert!(out.find(sh).unwrap() < out.find(target).unwrap(), "{out}");
}

#[cfg(unix)]
#[test]
fn why_a_port_traces_a_listener_started_by_a_shell() {
    let parented = Parented::start();
    let (json, code) = run_json(&["why", &format!(":{}", parented.port), "--json"]);
    assert_eq!(code, 0, "{json}");
    let result = &json["results"][0];
    assert_eq!(result["process"]["pid"], parented.listener_pid);
    let ancestry = result["ancestry"].as_array().unwrap();
    let pids: Vec<u64> = ancestry
        .iter()
        .map(|a| a["pid"].as_u64().unwrap())
        .collect();
    let n = pids.len();
    assert!(n >= 3, "{json}");
    assert_eq!(
        pids[n - 3..],
        [
            u64::from(std::process::id()),
            u64::from(parented.sh_pid()),
            u64::from(parented.listener_pid)
        ],
        "{json}"
    );
    let targets: Vec<bool> = ancestry
        .iter()
        .map(|a| a["is_target"].as_bool().unwrap())
        .collect();
    assert_eq!(targets.iter().filter(|t| **t).count(), 1, "{json}");
    assert!(targets[n - 1], "{json}");
}

#[cfg(unix)]
#[test]
fn why_marks_a_stopped_process_with_its_own_symbol() {
    let owned = Owned::listener();
    // SAFETY: stopping the one listener this test started, by its PID; it is
    // killed on drop whatever state it is in.
    unsafe {
        libc::kill(owned.pid() as libc::pid_t, libc::SIGSTOP);
    }
    // The state change shows up in the process list a moment later
    let mut out = String::new();
    for _ in 0..50 {
        out = run_text(&["why", &owned.pid_arg()]).0;
        if target_line(&out).contains('◐') {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(target_line(&out).contains('◐'), "{out}");
}

#[test]
fn why_several_targets_prints_each_under_its_own_header() {
    let owned = Owned::listener();
    let targets = format!("{},{}", owned.pid(), owned.port_arg());
    let (out, _, code) = run_text(&["why", &targets]);
    assert_eq!(code, 0);
    let ancestry = out
        .find(&format!("✓ Ancestry for '{}':", owned.pid()))
        .expect("the PID's header");
    let port = out
        .find(&format!("✓ Port {} (TCP):", owned.port))
        .expect("the port's header");
    assert!(ancestry < port, "{out}");
    // A blank line separates the two answers
    let between = &out[ancestry..port];
    assert!(between.ends_with("\n\n"), "{between:?}");
    assert_eq!(
        out.lines().filter(|l| l.ends_with("← target")).count(),
        2,
        "{out}"
    );
}

#[test]
fn why_takes_several_targets_as_separate_arguments() {
    let owned = Owned::listener();
    let (pid, port) = (owned.pid_arg(), owned.port_arg());

    // each target under its own header, in the order typed
    let (out, _, code) = run_text(&["why", &pid, &port]);
    assert_eq!(code, 0);
    let ancestry = out
        .find(&format!("✓ Ancestry for '{pid}':"))
        .expect("the PID's header");
    let port_header = out
        .find(&format!("✓ Port {} (TCP):", owned.port))
        .expect("the port's header");
    assert!(ancestry < port_header, "{out}");

    // one JSON document with a result per target, and the same answer as the comma form
    let (separate, code) = run_json(&["why", &pid, &port, MISSING_PID, "--json"]);
    assert_eq!(code, 0);
    assert_eq!(separate["count"], 2, "{separate}");
    assert_eq!(separate["results"][0]["target"], pid);
    assert_eq!(separate["results"][1]["target"], port);
    let comma = format!("{pid},{port},{MISSING_PID}");
    let (joined, _) = run_json(&["why", &comma, "--json"]);
    assert_eq!(joined["count"], 2, "{joined}");
    let tail = format!("{port},{MISSING_PID}");
    let (mixed, _) = run_json(&["why", &pid, &tail, "--json"]);
    assert_eq!(mixed["count"], 2, "{mixed}");
    assert_eq!(mixed["results"][1]["target"], port);
}

#[test]
fn why_with_a_target_that_splits_into_nothing_is_invalid_input() {
    let (json, code) = run_json(&["why", ",", ",,", "--json"]);
    assert_eq!(code, 4, "{json}");
    assert_eq!(json["reason"], "invalid_input", "{json}");
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("a target can't be empty"), "{error}");
}

#[test]
fn why_warns_about_a_target_that_is_gone_and_still_answers_for_the_rest() {
    let owned = Owned::listener();
    let targets = format!("{MISSING_PID},{}", owned.pid());
    let (out, stderr, code) = run_text(&["why", &targets]);
    assert_eq!(code, 0, "{stderr}");
    // the comma list is the one thing stderr says
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.starts_with("note: comma-separated targets are deprecated"));
    assert!(
        out.contains(&format!("⚠ No process found matching '{MISSING_PID}'")),
        "{out}"
    );
    assert!(target_line(&out).contains(&format!("[{}]", owned.pid())));
}

#[test]
fn why_a_target_nothing_matches_warns_and_exits_not_found() {
    let (out, stderr, code) = run_text(&["why", MISSING_PID]);
    assert_eq!(code, 2);
    assert_eq!(
        out,
        format!(
            "⚠ No process found matching '{MISSING_PID}'\n  Try: proc list to list all processes\n"
        )
    );
    // The warning was the whole answer: nothing more goes to stderr
    assert_eq!(stderr, "");
}

#[test]
fn why_a_port_nobody_listens_on_warns_and_exits_not_found() {
    let port = free_port();
    let (out, stderr, code) = run_text(&["why", &format!(":{port}")]);
    assert_eq!(code, 2);
    assert!(
        out.contains(&format!("⚠ No process listening on port {port}")),
        "{out}"
    );
    assert_eq!(stderr, "");
}

#[test]
fn why_with_every_target_missing_warns_for_each_in_the_order_given() {
    let port = free_port();
    let (out, _, code) = run_text(&["why", &format!(":{port},{MISSING_PID}")]);
    assert_eq!(code, 2);
    // One warning per target, in the order given
    let port_warning = out
        .find("No process listening on port")
        .expect("port warning");
    let pid_warning = out.find("No process found matching").expect("pid warning");
    assert!(port_warning < pid_warning, "{out}");
}

// --- why: JSON ---------------------------------------------------------------

#[test]
fn why_json_a_port_carries_the_port_and_the_listener() {
    let owned = Owned::listener();
    // Wait for the listener, its port and its working directory to show up in every
    // snapshot the answer is built from
    let (mut json, mut code) = (Value::Null, -1);
    for _ in 0..50 {
        (json, code) = run_json(&["why", &owned.port_arg(), "--json"]);
        let result = &json["results"][0];
        if code == 0
            && result["port"] == owned.port
            && result["ports"][0]["port"] == owned.port
            && result["process"]["cwd"].is_string()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(code, 0, "{json}\n{}", owned.what_proc_saw());
    assert_eq!(json["action"], "why");
    assert_eq!(json["success"], true);
    assert_eq!(json["count"], 1);
    let result = &json["results"][0];
    assert_eq!(result["target"], owned.port_arg());
    assert_eq!(result["port"], owned.port);
    assert_eq!(result["protocol"], "tcp");
    assert_eq!(result["process"]["pid"], owned.pid());
    assert_eq!(result["process"]["name"], owned.file_name());
    assert!(
        same_place(result["process"]["cwd"].as_str().unwrap(), &owned.dir()),
        "{json}\n{}",
        owned.what_proc_saw()
    );
    let ports: Vec<u64> = result["ports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["port"].as_u64().unwrap())
        .collect();
    assert_eq!(ports, vec![u64::from(owned.port)], "{json}");
    let ancestry = result["ancestry"].as_array().unwrap();
    let last = ancestry.last().unwrap();
    assert_eq!(last["pid"], owned.pid());
    assert_eq!(last["is_target"], true);
    // The test process started it, so it is the entry just above
    let parent = &ancestry[ancestry.len() - 2];
    assert_eq!(parent["pid"], std::process::id());
    assert_eq!(parent["is_target"], false);
}

#[test]
fn why_json_a_pid_has_no_port_fields() {
    let owned = Owned::listener();
    let (json, code) = run_json(&["why", &owned.pid_arg(), "--json"]);
    assert_eq!(code, 0);
    let result = &json["results"][0];
    assert!(result.get("port").is_none(), "{json}");
    assert!(result.get("protocol").is_none(), "{json}");
    // The ports the process holds are listed all the same
    assert_eq!(result["ports"][0]["port"], owned.port, "{json}");
}

#[test]
fn why_json_with_one_target_gone_answers_for_the_rest() {
    let owned = Owned::listener();
    let targets = format!("{},{MISSING_PID}", owned.pid());
    let (json, code) = run_json(&["why", &targets, "--json"]);
    assert_eq!(code, 0);
    assert_eq!(json["success"], true);
    assert_eq!(json["count"], 1);
    assert_eq!(json["results"][0]["process"]["pid"], owned.pid());
}

#[test]
fn why_json_with_every_target_missing_is_the_first_error() {
    let port = free_port();
    let (json, code) = run_json(&["why", &format!("{MISSING_PID},:{port}"), "--json"]);
    assert_eq!(code, 2);
    assert_eq!(json["action"], "why");
    assert_eq!(json["success"], false);
    assert_eq!(json["exit_code"], 2);
    assert_eq!(
        json["error"],
        format!(
            "No process found matching '{MISSING_PID}'\n  Try: proc list to list all processes"
        )
    );
    assert!(json.get("needs_confirmation").is_none(), "{json}");
}

// --- info --------------------------------------------------------------------

/// The value of the card line labelled `label`
fn card_field(out: &str, label: &str) -> String {
    let prefix = format!("  {label:<13}");
    out.lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("no {label} line in:\n{out}"))
        .to_string()
}

#[test]
#[cfg_attr(
    any(target_os = "freebsd", target_os = "netbsd"),
    ignore = "the BSDs read no connections"
)]
fn info_shows_the_card_with_the_ports_it_listens_on() {
    let owned = Owned::listener();
    let (json, _) = run_json(&["info", &owned.pid_arg(), "--json"]);
    let known = &json["processes"][0];
    let (out, stderr, code) = run_text(&["info", &owned.pid_arg()]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stderr, "");
    assert_eq!(
        card_field(&out, "Process:"),
        format!("{} (PID {})", owned.file_name(), owned.pid())
    );
    // `-v` for the paths whole: a long one is cut to the terminal otherwise
    let (verbose, _, _) = run_text(&["info", &owned.pid_arg(), "-v"]);
    assert!(
        same_place(&card_field(&verbose, "Directory:"), &owned.dir()),
        "{verbose}\n{}",
        owned.what_proc_saw()
    );
    assert!(
        same_place(&card_field(&verbose, "Path:"), &owned.exe()),
        "{verbose}"
    );
    let parent = card_field(&out, "Parent:");
    assert!(
        parent.ends_with(&format!("(PID {})", std::process::id())),
        "{parent}"
    );
    if let Some(user) = known["user"].as_str() {
        assert_eq!(card_field(&out, "User:"), user);
    }
    assert_eq!(card_field(&out, "Status:"), known["status"]);
    assert!(card_field(&out, "Resources:").contains("% CPU, "), "{out}");
    assert!(card_field(&out, "Started:").contains("(up "), "{out}");
    assert!(!card_field(&out, "Command:").is_empty());
    // the port it listens on, with who may reach it
    let listening = card_field(&out, "Listening:");
    assert!(
        listening.starts_with(&format!(":{} TCP", owned.port)),
        "{listening}"
    );
    assert!(listening.contains('('), "{listening}");
    // a listener has connections to report (none yet, but the line is there)
    assert!(out.contains("  Connections: "), "{out}");
    assert!(out.ends_with("\n\n"), "{out:?}");
    // the old labels went with the old layout
    assert!(!out.contains("Parent PID:"), "{out}");
    assert!(!out.contains("  Name:"), "{out}");
}

#[test]
fn info_headline_names_the_process() {
    let owned = Owned::listener();
    let (out, _, code) = run_text(&["info", &owned.pid_arg()]);
    assert_eq!(code, 0);
    assert_eq!(
        out.lines().next().unwrap(),
        format!("✓ {} (PID {})", owned.file_name(), owned.pid())
    );
    assert_eq!(out.lines().nth(1), Some(""), "{out}");
}

#[test]
fn info_leaves_out_connections_for_a_process_that_listens_on_nothing() {
    // the test's own listener is the process; a shell it runs sleeps and listens on nothing
    let child = std::process::Command::new(if cfg!(windows) { "ping" } else { "sleep" })
        .args(if cfg!(windows) {
            vec!["-n", "300", "127.0.0.1"]
        } else {
            vec!["300"]
        })
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut child = ChildGuard(child);
    let (out, _, code) = run_text(&["info", &child.0.id().to_string()]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(card_field(&out, "Listening:"), "none");
    assert!(!out.contains("Connections:"), "{out}");
    let (json, _) = run_json(&["info", &child.0.id().to_string(), "--json"]);
    assert_eq!(json["processes"][0]["ports"], serde_json::json!([]));
    let _ = child.0.kill();
    let _ = child.0.wait();
}

/// Kills and reaps its child when dropped
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn info_json_lists_each_port_with_its_exposure_and_connection_counts() {
    let owned = Owned::listener();
    let (json, _) = run_json(&["info", &owned.pid_arg(), "--json"]);
    let process = &json["processes"][0];
    // every key it had is still there
    assert_eq!(process["pid"], owned.pid());
    assert!(process["name"].is_string());
    assert!(process["status"].is_string());
    let ports = process["ports"].as_array().unwrap();
    let mine = ports
        .iter()
        .find(|p| p["port"] == owned.port)
        .unwrap_or_else(|| panic!("no port {} in {json}", owned.port));
    assert_eq!(mine["pid"], owned.pid());
    assert_eq!(mine["protocol"], "tcp");
    assert!(
        matches!(mine["exposure"].as_str(), Some("local" | "exposed")),
        "{mine}"
    );
    if cfg!(not(any(target_os = "freebsd", target_os = "netbsd"))) {
        assert!(mine["connections"]["total"].is_u64(), "{mine}");
        assert!(mine["connections"]["by_state"].is_object(), "{mine}");
        // counts, not peers
        assert!(mine["connections"].get("peers").is_none(), "{mine}");
    } else {
        // the BSDs read none: the key is absent, never null
        assert!(mine.get("connections").is_none(), "{mine}");
    }
}

#[test]
fn info_verbose_prints_every_value_whole() {
    let long = "x".repeat(300);
    let owned = Owned::listener_with_args(&[&long]);
    let (out, _, code) = run_text(&["info", &owned.pid_arg()]);
    assert_eq!(code, 0);
    let command = card_field(&out, "Command:");
    assert!(command.ends_with('…'), "cut to the terminal: {command}");
    assert!(command.chars().count() < long.len(), "{command}");
    for flag in ["-v", "--verbose"] {
        let (out, _, code) = run_text(&["info", &owned.pid_arg(), flag]);
        assert_eq!(code, 0);
        let command = card_field(&out, "Command:");
        assert!(command.ends_with(&long), "{flag}: {command}");
    }
}

#[test]
fn info_a_port_and_its_pid_show_the_process_once() {
    let owned = Owned::listener();
    let (out, _, code) = run_text(&["info", &owned.port_arg(), &owned.pid_arg()]);
    assert_eq!(code, 0);
    assert_eq!(out.matches("  Process:").count(), 1, "{out}");
    let (json, _) = run_json(&["info", &owned.port_arg(), &owned.pid_arg(), "--json"]);
    assert_eq!(json["found_count"], 1);
    assert_eq!(json["not_found_count"], 0);
}

#[test]
fn info_warns_about_the_targets_it_did_not_find_and_still_shows_the_rest() {
    let owned = Owned::listener();
    let gone = free_port();
    let targets = format!("{},{MISSING_PID},:{gone}", owned.pid());
    let (out, stderr, code) = run_text(&["info", &targets]);
    assert_eq!(code, 0, "{stderr}");
    // the comma list is the one thing stderr says
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.starts_with("note: comma-separated targets are deprecated"));
    assert_eq!(out.matches("  Process:").count(), 1, "{out}");
    assert!(
        out.trim_end()
            .ends_with(&format!("⚠ Not found: {MISSING_PID}, :{gone}")),
        "{out}"
    );
    let (json, code) = run_json(&["info", &targets, "--json"]);
    assert_eq!(code, 0);
    assert_eq!(json["success"], true);
    assert_eq!(json["found_count"], 1);
    assert_eq!(json["not_found_count"], 2);
    assert_eq!(
        json["not_found"],
        serde_json::json!([MISSING_PID, format!(":{gone}")])
    );
}

#[test]
fn info_takes_several_targets_as_separate_arguments() {
    let first = Owned::listener();
    let second = Owned::listener();
    let gone = free_port();
    let (a, b) = (first.pid_arg(), second.pid_arg());
    let missing_port = format!(":{gone}");

    let (out, stderr, code) = run_text(&["info", &a, &b, MISSING_PID, &missing_port]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(out.matches("  Process:").count(), 2, "{out}");
    assert!(
        out.trim_end()
            .ends_with(&format!("⚠ Not found: {MISSING_PID}, :{gone}")),
        "{out}"
    );

    // the JSON lists keep one entry per target
    let (json, code) = run_json(&["info", &a, &b, MISSING_PID, &missing_port, "--json"]);
    assert_eq!(code, 0);
    assert_eq!(json["found_count"], 2, "{json}");
    assert_eq!(json["not_found_count"], 2, "{json}");
    assert_eq!(
        json["not_found"],
        serde_json::json!([MISSING_PID, missing_port]),
        "{json}"
    );

    // separate arguments and a comma list mix
    let tail = format!("{b},{MISSING_PID}");
    let (mixed, _) = run_json(&["info", &a, &tail, "--json"]);
    assert_eq!(mixed["found_count"], 2, "{mixed}");
    assert_eq!(mixed["not_found"], serde_json::json!([MISSING_PID]));
}

#[test]
fn info_shows_a_pid_given_twice_once() {
    let owned = Owned::listener();
    let pid = owned.pid_arg();
    let both = format!("{pid},{pid}");
    for args in [
        ["info", &pid, &pid, "--json"],
        ["info", &pid, &both, "--json"],
    ] {
        let (json, code) = run_json(&args);
        assert_eq!(code, 0, "{json}");
        assert_eq!(json["found_count"], 1, "{json}");
    }
}

#[test]
fn info_with_nothing_found_for_several_targets_names_each_one_in_the_list() {
    let (out, _, code) = run_text(&["info", MISSING_PID, "99999998"]);
    assert_eq!(code, 2);
    assert_eq!(out, format!("⚠ Not found: {MISSING_PID}, 99999998\n"));
    let (json, code) = run_json(&["info", MISSING_PID, "99999998", "--json"]);
    assert_eq!(code, 2);
    assert_eq!(
        json["not_found"],
        serde_json::json!([MISSING_PID, "99999998"])
    );
}

#[test]
fn info_with_nothing_found_warns_and_exits_not_found() {
    let (out, stderr, code) = run_text(&["info", MISSING_PID]);
    assert_eq!(code, 2);
    assert_eq!(out, format!("⚠ Not found: {MISSING_PID}\n"));
    assert_eq!(stderr, "");
    let (json, code) = run_json(&["info", MISSING_PID, "--json"]);
    assert_eq!(code, 2);
    assert_eq!(json["success"], false);
    assert_eq!(json["found_count"], 0);
    assert_eq!(json["not_found"], serde_json::json!([MISSING_PID]));
}

#[test]
fn info_in_keeps_a_process_running_in_the_directory_and_drops_one_that_is_not() {
    let owned = Owned::listener();
    // The directory as a user types it, which is not always how the OS reports it
    let (out, _, code) = run_text(&["info", &owned.pid_arg(), "--in", &owned.raw_dir()]);
    assert_eq!(code, 0, "{out}\n{}", owned.what_proc_saw());
    assert_eq!(out.matches("  Process:").count(), 1, "{out}");

    let elsewhere = tempfile::tempdir().unwrap();
    let (out, _, code) = run_text(&[
        "info",
        &owned.pid_arg(),
        "-i",
        elsewhere.path().to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{out}");
    assert!(!out.contains("  Process:"), "{out}");
}

#[test]
fn info_name_is_the_whole_name_and_cmd_widens_it_to_the_command_line() {
    let word = nobody();
    let owned = Owned::listener_with_args(&[&word]);
    let part = &owned.name[..owned.name.len() - 1];

    // the whole name, in any case
    for name in [owned.name.clone(), owned.name.to_uppercase()] {
        let (json, code) = run_json(&["info", &name, "--json"]);
        assert_eq!(code, 0, "{json}");
        assert_eq!(json["processes"][0]["pid"], owned.pid(), "{json}");
    }

    // a part of the name, or a word of the command line, is not found, and the
    // warning says --cmd would find the word
    for text in [part, word.as_str()] {
        let (json, code) = run_json(&["info", text, "--json"]);
        assert_eq!(code, 2, "{json}");
        assert_eq!(json["not_found"][0], text, "{json}");
    }
    let (out, err, code) = run_text(&["info", &word]);
    assert_eq!(code, 2, "{out}{err}");
    assert!(
        format!("{out}{err}").contains(&format!(
            "Not found: {word}\n  1 process has '{word}' in its command line (add --cmd to match those)"
        )),
        "{out}{err}"
    );

    for text in [part, word.as_str()] {
        let (json, code) = run_json(&["info", text, "--cmd", "--json"]);
        assert_eq!(code, 0, "{json}");
        assert!(
            json["processes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["pid"] == owned.pid()),
            "{json}"
        );
    }
}

#[test]
fn info_by_keeps_a_process_with_that_name_and_drops_one_without() {
    let owned = Owned::listener();
    let (out, _, code) = run_text(&["info", &owned.pid_arg(), "--by", &owned.name]);
    assert_eq!(code, 0);
    assert_eq!(out.matches("  Process:").count(), 1, "{out}");

    // --by is the whole name too; --cmd makes it any part of the name
    let part = &owned.name[..owned.name.len() - 1];
    let (_, _, code) = run_text(&["info", &owned.pid_arg(), "--by", part]);
    assert_eq!(code, 2);
    let (out, _, code) = run_text(&["info", &owned.pid_arg(), "--by", part, "--cmd"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.matches("  Process:").count(), 1, "{out}");

    let nobody = nobody();
    let (json, code) = run_json(&["info", &owned.pid_arg(), "-b", &nobody, "--json"]);
    assert_eq!(code, 2);
    // The target resolved; the filter is what dropped it, and the error says so
    let error = json["error"].as_str().unwrap();
    assert!(
        error.contains(&format!("ruled out by --by {nobody}")),
        "{json}"
    );
}

// --- main: the bare command, help, completions, man page ----------------------

#[test]
fn proc_alone_prints_the_orientation_screen() {
    let (out, stderr, code) = run_text(&[]);
    assert_eq!(code, 0);
    assert_eq!(stderr, "");
    assert!(
        out.starts_with("proc — semantic process management\n"),
        "{out}"
    );
    assert!(out.contains("A target is the same everywhere: :port, PID, or name."));
    for example in [
        "proc on :3000",
        "proc in .",
        "proc by node",
        "proc kill :3000",
    ] {
        assert!(out.contains(example), "{example} missing:\n{out}");
    }
    assert!(out.contains("Common:  on · in · by · ports · info · kill · stop · free · why"));
    assert!(out.contains("proc --help"));
    assert!(out.contains("https://proc-cli.com"));
}

#[test]
fn the_orientation_screen_counts_the_commands_there_are() {
    let commands = Cli::command()
        .get_subcommands()
        .filter(|c| !c.is_hide_set() && c.get_name() != "help")
        .count();
    let (out, _, _) = run_text(&[]);
    assert!(
        out.contains(&format!("all {commands} commands")),
        "{commands} commands, but the screen says:\n{out}"
    );
}

#[test]
fn the_commands_the_orientation_screen_shows_are_real_commands() {
    let (out, _, _) = run_text(&[]);
    let names: Vec<String> = Cli::command()
        .get_subcommands()
        .map(|c| c.get_name().to_string())
        .collect();
    let common = line_with(&out, "Common:");
    for word in common.trim_start_matches("  Common:").split('·') {
        let word = word.trim();
        assert!(names.iter().any(|n| n == word), "{word} is not a command");
    }
}

#[test]
fn help_and_version_print_and_exit_cleanly() {
    let (out, stderr, code) = run_text(&["--help"]);
    assert_eq!(code, 0);
    assert_eq!(stderr, "");
    assert!(
        out.contains("Commands:") && out.contains("Examples:"),
        "{out}"
    );

    let (out, _, code) = run_text(&["--version"]);
    assert_eq!(code, 0);
    // A build from the release tag prints the plain version; any other build
    // adds `-dev (branch@commit, ...)` after it.
    let line = out.lines().next().unwrap_or("");
    let version = line.strip_prefix("proc ").unwrap_or(line);
    assert!(
        version == env!("CARGO_PKG_VERSION")
            || version.starts_with(&format!("{}-dev (", env!("CARGO_PKG_VERSION"))),
        "{out}"
    );
    assert_eq!(version, proc_cli::VERSION);
}

/// The footer facts, as they must read in plain text
const FOOTER_FACTS: [&str; 9] = [
    "Targets:",
    ":3000 node   Several: separate arguments",
    "The process name, the file name of its executable or of argv[0],",
    "ignoring case (like pkill -x); 'node' is not 'Claude Helper'",
    "Any part of the name or the full command line (like pkill -f)",
    "Not a name: use --cmd, or proc for <file>",
    "Not expanded by proc, but by your shell if unquoted: use --cmd",
    "Deprecated, to be removed; prints a note on stderr",
    "put it after the targets, or write --in=DIR",
];

#[test]
fn the_help_ends_with_labelled_sections() {
    let (out, _, code) = run_text(&["--help"]);
    assert_eq!(code, 0);
    for fact in FOOTER_FACTS {
        assert!(out.contains(fact), "{fact:?} is not in\n{out}");
    }
    for header in ["Targets:", "Names:", "Notes:", "More:"] {
        assert!(
            out.lines().any(|l| l.starts_with(header)),
            "no {header} section in\n{out}"
        );
    }
    assert!(out
        .trim_end()
        .ends_with("More: https://github.com/yazeed/proc-cli"));
    // every line of the footer fits an 80-column terminal
    let footer = &out[out.find("\nTargets:").unwrap()..];
    for line in footer.lines() {
        assert!(line.chars().count() <= 80, "{line:?} is wider than 80");
    }
    // piped output has no colour codes
    assert!(!out.contains('\u{1b}'), "{out:?}");

    // the short help keeps the targets and points to the long one
    let (short, _, code) = run_text(&["-h"]);
    assert_eq!(code, 0);
    assert!(short.contains("Targets:\n  :3000"), "{short}");
    assert!(short.contains("More: proc --help for examples"), "{short}");
    assert!(!short.contains("Names:"), "{short}");
}

#[test]
fn the_footer_headers_use_clap_header_style_and_only_when_colour_is_on() {
    // the Command's own styles, which the footer must use: no look of its own
    let command = Cli::command();
    let header = command.get_styles().get_header();
    let literal = command.get_styles().get_literal();
    assert_eq!(
        format!("{:?}", command.get_styles()),
        format!("{:?}", proc_cli::cli::STYLES)
    );

    let forced = Command::cargo_bin("proc")
        .unwrap()
        .env_remove("NO_COLOR")
        .env("CLICOLOR_FORCE", "1")
        .arg("--help")
        .output()
        .unwrap();
    let out = String::from_utf8(forced.stdout).unwrap();
    for title in ["Usage:", "Targets:", "Names:", "Notes:", "More:"] {
        assert!(
            out.contains(&format!("{header}{title}{header:#}")),
            "{title} is not drawn in clap's header style:\n{out:?}"
        );
    }
    assert!(
        out.contains(&format!("{literal}--cmd{literal:#}")),
        "{out:?}"
    );

    // NO_COLOR wins over a forced colour
    let plain = Command::cargo_bin("proc")
        .unwrap()
        .env("NO_COLOR", "1")
        .env("CLICOLOR_FORCE", "1")
        .arg("--help")
        .output()
        .unwrap();
    assert!(!String::from_utf8(plain.stdout).unwrap().contains('\u{1b}'));
}

#[test]
fn the_man_page_and_completions_have_no_colour_codes() {
    for args in [
        &["manpage"][..],
        &["completions", "bash"][..],
        &["completions", "zsh"][..],
        &["completions", "fish"][..],
    ] {
        let output = Command::cargo_bin("proc")
            .unwrap()
            .env_remove("NO_COLOR")
            .env("CLICOLOR_FORCE", "1")
            .args(args)
            .output()
            .unwrap();
        let out = String::from_utf8(output.stdout).unwrap();
        assert!(!out.contains('\u{1b}'), "{args:?} has an escape code");
        assert!(!out.is_empty(), "{args:?}");
    }
}

#[test]
fn completions_print_a_script_for_each_shell() {
    for (shell, marker) in [
        ("bash", "complete -F _proc"),
        ("zsh", "#compdef proc"),
        ("fish", "complete -c proc"),
        ("powershell", "Register-ArgumentCompleter"),
        ("elvish", "edit:completion"),
    ] {
        let (out, stderr, code) = run_text(&["completions", shell]);
        assert_eq!(code, 0, "{shell}: {stderr}");
        assert!(out.contains(marker), "{shell}: no {marker:?} in\n{out}");
        assert!(out.contains("stuck"), "{shell}: a command is missing");
    }
}

#[test]
fn completions_for_an_unknown_shell_is_a_usage_error() {
    let (out, stderr, code) = run_text(&["completions", "nushell"]);
    assert_eq!(code, 4);
    assert_eq!(out, "");
    assert!(
        stderr.starts_with("error: invalid value 'nushell'"),
        "{stderr}"
    );
    assert!(stderr.contains("bash"), "the possible values: {stderr}");
    // Nothing to suggest for a value (only flags get a suggestion)
    assert!(!stderr.contains("did you mean"), "{stderr}");
}

#[test]
fn manpage_prints_the_man_page() {
    let (out, stderr, code) = run_text(&["manpage"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(out.contains(".TH proc 1"), "{out}");
    assert!(out.contains(".SH NAME"), "{out}");
    assert!(out.contains("proc\\-why"), "the why command: {out}");
}

/// The title line (`.TH`) names the version line of `--version` alone. The
/// whole `--version` text has a URL and the licence on lines of their own,
/// which left the title's quote open and showed them under the title in `man`
#[test]
fn the_man_page_title_names_only_the_version() {
    let (out, _, code) = run_text(&["manpage"]);
    assert_eq!(code, 0);
    let (version, _, _) = run_text(&["--version"]);
    let version = version.lines().next().unwrap();
    let title = out.lines().find(|l| l.starts_with(".TH ")).unwrap();
    assert!(title.contains(&format!("\"{version}\"")), "{title}");
    assert_eq!(title.matches('"').count() % 2, 0, "an open quote: {title}");
    assert!(
        !out.contains("https://github.com/yazeed/proc-cli\n"),
        "{out}"
    );
}

// --- main: usage errors -------------------------------------------------------

#[test]
fn an_unknown_command_is_a_usage_error() {
    let (out, stderr, code) = run_text(&["lisst"]);
    assert_eq!(code, 4);
    assert_eq!(out, "");
    assert!(
        stderr.starts_with("error: unrecognized subcommand 'lisst'"),
        "{stderr}"
    );
    // clap's own "tip:" lines are dropped; its suggestion is proc's one tip
    assert!(!stderr.contains("a similar"), "{stderr}");
    assert!(
        stderr.ends_with("\n  tip: did you mean 'list' or 'info'?\n"),
        "{stderr}"
    );
    // clap names the program after argv[0]'s file name: `proc.exe` on Windows
    let usage = format!("Usage: proc{} [COMMAND]", std::env::consts::EXE_SUFFIX);
    assert!(stderr.contains(&usage), "{stderr}");
}

#[test]
fn an_unknown_command_with_json_is_a_json_error_naming_the_input() {
    let (json, code) = run_json(&["lisst", "--json"]);
    assert_eq!(code, 4);
    assert_eq!(json["action"], "lisst");
    assert_eq!(json["success"], false);
    assert_eq!(json["exit_code"], 4);
    assert_eq!(
        json["error"],
        "unrecognized subcommand 'lisst'\n  Try: did you mean 'list' or 'info'?"
    );
    assert!(json.get("needs_confirmation").is_none(), "{json}");
}

#[test]
fn a_command_alias_is_named_by_the_command_in_json_errors() {
    // `ps` is `list`
    let (json, code) = run_json(&["ps", "--bogus", "--json"]);
    assert_eq!(code, 4);
    assert_eq!(json["action"], "list");
}

#[test]
fn a_misspelled_flag_gets_a_suggestion() {
    // Close to a flag by edit distance, but not part of it
    let (out, stderr, code) = run_text(&["list", "--verbse"]);
    assert_eq!(code, 4);
    assert_eq!(out, "");
    assert!(
        stderr.starts_with("error: unexpected argument '--verbse' found"),
        "{stderr}"
    );
    assert!(
        stderr.ends_with("\n  tip: did you mean '--verbose'?\n"),
        "{stderr}"
    );
    assert_eq!(stderr.matches("tip:").count(), 1, "{stderr}");

    // Part of a longer flag
    let (_, stderr, code) = run_text(&["list", "--cpu"]);
    assert_eq!(code, 4);
    assert!(
        stderr.ends_with("\n  tip: did you mean '--min-cpu'?\n"),
        "{stderr}"
    );
}

#[test]
fn a_misspelled_flag_with_json_puts_the_suggestion_in_the_error() {
    let (json, code) = run_json(&["list", "--cpu", "--json"]);
    assert_eq!(code, 4);
    assert_eq!(json["action"], "list");
    assert_eq!(json["success"], false);
    assert_eq!(json["exit_code"], 4);
    assert_eq!(
        json["error"],
        "unexpected argument '--cpu' found\n  Try: did you mean '--min-cpu'?"
    );
}

#[test]
fn a_flag_like_nothing_gets_no_suggestion() {
    let (_, stderr, code) = run_text(&["list", "--qqqqqqqqqq"]);
    assert_eq!(code, 4);
    assert!(
        stderr.starts_with("error: unexpected argument '--qqqqqqqqqq' found"),
        "{stderr}"
    );
    assert!(!stderr.contains("did you mean"), "{stderr}");
    let (json, _) = run_json(&["list", "--qqqqqqqqqq", "--json"]);
    assert_eq!(json["error"], "unexpected argument '--qqqqqqqqqq' found");
}

#[test]
fn a_usage_error_before_any_command_is_named_proc_in_json() {
    let (json, code) = run_json(&["--bogus", "--json"]);
    assert_eq!(code, 4);
    assert_eq!(json["action"], "proc");
    assert_eq!(json["error"], "unexpected argument '--bogus' found");
}

#[test]
fn a_target_that_is_not_a_port_is_a_usage_error() {
    let (out, stderr, code) = run_text(&["why", ":70000"]);
    assert_eq!(code, 4);
    assert_eq!(out, "");
    assert!(
        stderr.contains("':70000' is not a port; ports go from :1 to :65535"),
        "{stderr}"
    );
    let (json, code) = run_json(&["info", ":0", "--json"]);
    assert_eq!(code, 4);
    assert_eq!(json["action"], "info");
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("':0' is not a port"),
        "{json}"
    );
}

// --- main: which flags ask for JSON -------------------------------------------

#[test]
fn json_is_asked_for_by_the_long_flag_the_short_flag_or_a_cluster() {
    for args in [
        &["info", "--bogus", "--json"][..],
        &["info", "--bogus", "-j"],
        &["info", "--bogus", "-vj"],
        &["kill", "--bogus", "-yj"],
    ] {
        let (json, code) = run_json(args);
        assert_eq!(code, 4, "{args:?}");
        assert_eq!(json["success"], false, "{args:?}");
    }
}

#[test]
fn json_is_not_asked_for_by_a_flag_that_only_looks_like_it() {
    for args in [
        // `-bj` is `--by j`: the j is a value
        &["info", "-bj", "--bogus"][..],
        // a cluster without a j
        &["info", "-v", "--bogus"],
        // anything after `--` is not a flag
        &["info", "--bogus", "--", "--json"],
        // a long flag that merely holds the letters
        &["info", "--bogus", "--jsonx"],
    ] {
        let (out, stderr, code) = run_text(args);
        assert_eq!(code, 4, "{args:?}");
        assert_eq!(out, "", "{args:?}");
        assert!(stderr.starts_with("error: "), "{args:?}: {stderr}");
    }
}

#[test]
fn a_cluster_ending_in_a_value_flag_does_not_ask_for_json() {
    // `-Xj` where X takes a value: the j is that value, not `--json`
    for letter in ["b", "d", "i", "l", "n", "p", "r", "s", "S", "t"] {
        let cluster = format!("-{letter}j");
        let (out, stderr, code) = run_text(&["info", &cluster, "--bogus"]);
        assert_eq!(code, 4, "{cluster}");
        assert_eq!(out, "", "{cluster}: asked for JSON");
        assert!(stderr.starts_with("error: "), "{cluster}: {stderr}");
    }
}

// --- main: errors when a command runs -----------------------------------------

#[test]
fn a_command_that_fails_with_json_prints_the_error_as_json() {
    let (json, code) = run_json(&["why", &format!(":{}", free_port()), "--json"]);
    assert_eq!(code, 2);
    assert_eq!(json["action"], "why");
    assert_eq!(json["success"], false);
    assert_eq!(json["exit_code"], 2);
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .starts_with("No process listening on port "),
        "{json}"
    );
    assert!(json.get("needs_confirmation").is_none(), "{json}");
    assert!(json.get("processes").is_none(), "{json}");
}

#[test]
fn a_command_that_fails_without_json_prints_the_error_on_stderr() {
    // `stop` without `--yes` and with nobody to ask is invalid input; the
    // process is left alone
    let mut owned = Owned::listener();
    let (out, stderr, code) = run_text(&["stop", &owned.pid_arg()]);
    assert_eq!(code, 4);
    assert_eq!(out, "");
    assert!(
        stderr.starts_with("stop asks before changing processes"),
        "{stderr}"
    );
    // the call is valid; it only needs a yes
    assert!(!stderr.contains("Invalid input"), "{stderr}");
    assert!(
        owned.child.try_wait().unwrap().is_none(),
        "the listener was stopped"
    );
}

#[test]
fn a_missing_yes_with_json_lists_the_processes_that_would_change() {
    let mut owned = Owned::listener();
    let (json, code) = run_json(&["stop", &owned.pid_arg(), "--json"]);
    assert_eq!(code, 4);
    assert_eq!(json["action"], "stop");
    assert_eq!(json["success"], false);
    assert_eq!(json["exit_code"], 4);
    assert_eq!(json["needs_confirmation"], true);
    assert_eq!(json["processes"][0]["pid"], owned.pid());
    assert!(json["error"].as_str().unwrap().contains("--yes"), "{json}");
    // `reason` carries the classification; the text does not call the call invalid
    assert_eq!(json["reason"], "needs_confirmation", "{json}");
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .starts_with("stop changes processes"),
        "{json}"
    );
    assert!(
        owned.child.try_wait().unwrap().is_none(),
        "the listener was stopped"
    );
}

#[test]
fn a_failure_the_command_already_reported_prints_nothing_more() {
    let (out, stderr, code) = run_text(&["info", MISSING_PID]);
    assert_eq!(code, 2);
    assert!(!out.is_empty());
    assert_eq!(stderr, "");
}

/// PID 1 is root's. As a user, macOS gives proc no status, no memory, no CPU
/// time and none of its sockets, so nothing of those is shown, and an empty
/// list of ports is not claimed to mean "listens on nothing". As root, proc
/// reads it all, and there is nothing to check here.
#[cfg(target_os = "macos")]
#[test]
fn another_users_process_shows_only_what_proc_could_read() {
    if unsafe { libc::geteuid() } != 0 {
        let (card, _, code) = run_text(&["info", "1"]);
        assert_eq!(code, 0, "{card}");
        assert!(card.contains("Process:"), "{card}");
        for absent in ["Listening:", "Connections:", "Status:", "Resources:", "0KB"] {
            assert!(!card.contains(absent), "{absent} in:\n{card}");
        }
        let (on, _, _) = run_text(&["on", "1"]);
        assert!(!on.contains("Listening:") && !on.contains("0KB"), "{on}");

        // `ports` is new in `info`, so it is absent; `on` has always had its list
        let (info, _) = run_json(&["info", "1", "--json"]);
        assert!(info["processes"][0].get("ports").is_none(), "{info}");
        let (on, _) = run_json(&["on", "1", "--json"]);
        assert_eq!(on["ports"], serde_json::json!([]), "{on}");

        // the table: a dash where the OS gave no CPU and no memory. A name
        // lookup leaves out the ancestors of proc, so a test run that descends
        // from launchd itself (an orphaned background run) cannot see its row.
        let mut at = std::process::id();
        let mut under_launchd = false;
        while let Some(parent) = Process::find_by_pid(at).unwrap().and_then(|p| p.parent_pid) {
            under_launchd |= parent == 1;
            at = parent;
        }
        if !under_launchd {
            let (table, _, _) = run_text(&["by", "launchd"]);
            let row = table
                .lines()
                .find(|l| l.split_whitespace().next() == Some("1"))
                .unwrap_or_else(|| panic!("no row for PID 1:\n{table}"));
            assert!(!row.contains("0KB") && !row.contains("0.0"), "{row}");
        }
    }
}
