//! `by`, `for` and `stuck`, driven through the real binary.
//!
//! Every process these tests aim proc at is one the test started: a copy of the
//! listener example (`examples/listener.rs`) under a name and path of its own,
//! in a fresh temp directory. So `by <name>` and `for <path>` match only that
//! process, and `stuck` is always scoped with `--in <that directory>`. Each
//! process is killed and reaped on drop.

mod contract;

use assert_cmd::Command;
use proc_cli::core::filters::strip_verbatim_prefix;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

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

/// Run proc and return its stdout and exit code.
fn run_text(args: &[&str]) -> (String, i32) {
    let output = proc_cmd().args(args).output().unwrap();
    (
        String::from_utf8(output.stdout).unwrap(),
        output.status.code().unwrap(),
    )
}

/// How long a command that needs the busy process to read as busy is run
/// again before its result is judged
const BUSY_PATIENCE: Duration = Duration::from_secs(30);

/// Run proc until `saw_busy` says its JSON found the busy process, or until
/// [`BUSY_PATIENCE`] is up, and return the last run (so the test's own
/// assertions still fail on it, with its output).
///
/// `Owned::busy` returns once proc listed the spinner, but the command a test
/// then runs measures CPU again, over a window of its own. On a loaded CI
/// runner the spinner can get no CPU in that window (a full core read 9.5%
/// there), and proc, rightly, finds nothing stuck. A run that did not find it
/// changed nothing: `stuck --kill --yes` kills only what it lists, and the
/// rest here only read. So running it again is safe.
fn run_json_until(args: &[&str], saw_busy: impl Fn(&Value) -> bool) -> (Value, i32) {
    let deadline = Instant::now() + BUSY_PATIENCE;
    loop {
        let (json, code) = run_json(args);
        if saw_busy(&json) || Instant::now() >= deadline {
            return (json, code);
        }
    }
}

/// [`run_json_until`] for a command printed as text
fn run_text_until(args: &[&str], saw_busy: impl Fn(&str) -> bool) -> (String, i32) {
    let deadline = Instant::now() + BUSY_PATIENCE;
    loop {
        let (out, code) = run_text(args);
        if saw_busy(&out) || Instant::now() >= deadline {
            return (out, code);
        }
    }
}

/// `target/debug/examples/listener`, found relative to this test binary
/// (`target/debug/deps/cmd_find-<hash>`)
fn listener_path() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("examples");
    path.push(format!("listener{}", std::env::consts::EXE_SUFFIX));
    assert!(path.exists(), "build the example first: {}", path.display());
    path
}

/// A fresh temp directory, as processes report paths: symlinks resolved (/var
/// is /private/var on macOS, and `--in` compares paths literally) and no
/// Windows `\\?\` prefix
fn fresh_dir() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().canonicalize().unwrap();
    let dir = PathBuf::from(strip_verbatim_prefix(&dir.to_string_lossy()));
    (tmp, dir)
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Run the copy, with its cwd in `dir`. On Linux a file another test thread has
/// just written can be "busy" (ETXTBSY) for an instant: a fork in a parallel
/// test still holds the write handle until it execs. That clears in
/// milliseconds, so try again, a bounded number of times. (Avoiding the race at
/// the source would mean running the original example, which brings back
/// processes that share one exe path; cmd_ports.rs retries too.)
fn spawn_copy(exe: &Path, args: &[&str], dir: &Path) -> Child {
    let mut attempts = 0;
    loop {
        let spawned = std::process::Command::new(exe)
            .args(args)
            .current_dir(dir)
            .stdout(Stdio::piped())
            .stdin(Stdio::null())
            .spawn();
        match spawned {
            Ok(child) => return child,
            Err(e) if attempts < 50 && is_text_file_busy(&e) => {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => panic!("spawn the listener copy: {e:?}"),
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

/// Held for as long as a busy process runs, so no more than one spins at a time
static BUSY: Mutex<()> = Mutex::new(());

/// A process the test owns: a copy of the listener example, run with its cwd in
/// a fresh temp directory. Killed and reaped on drop, whatever a test did to it.
struct Owned {
    child: Child,
    port: u16,
    /// A name no other process has, short enough for any OS to report whole
    name: String,
    exe: PathBuf,
    /// Keeps the directory alive; `dir` is its path as processes report it
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    /// Released after the child is killed (fields drop after `Drop::drop`)
    _busy: Option<MutexGuard<'static, ()>>,
}

impl Owned {
    /// A listener that sleeps
    fn listener() -> Self {
        Self::start(&[], None)
    }

    /// A second process running the same executable as `first`
    fn sharing_exe_with(first: &Owned) -> Self {
        Self::start(&[], Some(&first.exe))
    }

    /// A listener that spins on a core: stuck by proc's measure (over half a
    /// core in two readings a second apart). Returns once proc sees it as stuck.
    fn busy() -> Self {
        let guard = BUSY.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut busy = Self::start(&["--busy"], None);
        busy._busy = Some(guard);
        // proc has no CPU reading for a process that has used no CPU yet, so a
        // process started this instant is not found until a later look
        let (json, _) = run_json_until(
            &["stuck", "--timeout", "0", "--in", &busy.dir(), "--json"],
            |json| lists(json, "processes", busy.pid()),
        );
        if !lists(&json, "processes", busy.pid()) {
            panic!("never seen as stuck: {json}\n{}", busy.what_proc_saw());
        }
        busy
    }

    /// A listener with extra arguments (it ignores them, so they only put words
    /// in its command line)
    fn with_args(args: &[&str]) -> Self {
        Self::start(args, None)
    }

    /// A listener whose name is longer than the 15 characters Linux reports
    /// for a process name
    fn long_named() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        Self::start_as(
            format!("pcf-a-long-name-{}n{n}x", std::process::id()),
            &[],
            None,
        )
    }

    fn start(args: &[&str], exe: Option<&Path>) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        Self::start_as(format!("pcf{}n{n}x", std::process::id()), args, exe)
    }

    fn start_as(name: String, args: &[&str], exe: Option<&Path>) -> Self {
        let (tmp, dir) = fresh_dir();
        let exe = match exe {
            Some(shared) => shared.to_path_buf(),
            None => {
                let copy = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
                std::fs::copy(listener_path(), &copy).expect("copy the listener example");
                copy
            }
        };
        let mut child = spawn_copy(&exe, args, &dir);
        let mut line = String::new();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let read = stdout.read_line(&mut line);
        let port = read
            .ok()
            .and_then(|_| line.trim().strip_prefix("PORT "))
            .and_then(|p| p.parse().ok());
        match port {
            Some(port) => Owned {
                child,
                port,
                name,
                exe,
                _tmp: tmp,
                dir,
                _busy: None,
            },
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("the listener printed {line:?} instead of `PORT <n>`");
            }
        }
    }

    /// What proc and the OS say about this process, for a failure message.
    /// `list` and `info` measure CPU over their own 0.2 s window
    /// (`Process::measure_cpu`), a different path from `stuck`'s two readings a
    /// second apart, so a busy process shows a figure in both or in neither.
    fn what_proc_saw(&mut self) -> String {
        let dir = self.dir();
        let pid = self.pid().to_string();
        let text = |args: &[&str]| {
            let out = proc_cmd().args(args).output().unwrap();
            format!(
                "exit {:?}\n{}{}",
                out.status.code(),
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        };
        format!(
            "proc list --in {dir} --json:\n{}\nproc info {pid} --json:\n{}\nthe child's try_wait: {:?}",
            text(&["list", "--in", &dir, "--json"]),
            text(&["info", &pid, "--json"]),
            self.child.try_wait()
        )
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn dir(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }

    fn exe(&self) -> String {
        self.exe.to_string_lossy().into_owned()
    }

    /// Whether the child is still there: not exited (it is ours, so this also reaps it)
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Wait for the child to end
    fn ended_within(&mut self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if !self.alive() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Whether the JSON list `key` has a process with this PID
fn lists(json: &Value, key: &str, pid: u32) -> bool {
    json[key].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|p| p["pid"].as_u64() == Some(u64::from(pid)))
    })
}

/// Whether the JSON names a process with this PID anywhere: in the list it
/// shows (`processes`) or in what a kill reports (`succeeded`)
fn names(json: &Value, pid: u32) -> bool {
    match json {
        Value::Object(map) => {
            map.get("pid").and_then(Value::as_u64) == Some(u64::from(pid))
                || map.values().any(|value| names(value, pid))
        }
        Value::Array(items) => items.iter().any(|value| names(value, pid)),
        _ => false,
    }
}

/// Whether the text has this PID as a word of its own
fn mentions(text: &str, pid: u32) -> bool {
    let pid = pid.to_string();
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word == pid)
}

// ---- by ----

/// The status proc reports for the process, which is also what `--status` takes
fn status_of(p: &Owned) -> String {
    let (json, _) = run_json(&["by", &p.name, "--json"]);
    json["processes"][0]["status"]
        .as_str()
        .unwrap_or_else(|| panic!("no status in {json}"))
        .to_string()
}

#[test]
fn by_lists_the_process_as_a_table() {
    let p = Owned::listener();
    let (out, code) = run_text(&["by", &p.name]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Found 1 process by"), "{out}");
    assert!(out.contains(&p.pid().to_string()), "{out}");
}

#[test]
fn by_lists_the_process_as_json() {
    let p = Owned::listener();
    let (json, code) = run_json(&["by", &p.name, "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["action"], "by");
    assert_eq!(json["count"], 1, "{json}");
    assert!(lists(&json, "processes", p.pid()), "{json}");
}

/// A name nothing runs, for this call alone
fn unused_word() -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("word{}n{n}x", std::process::id())
}

#[test]
fn by_matches_the_whole_name_in_any_case_and_not_a_part_of_it() {
    let p = Owned::listener();
    let (json, code) = run_json(&["by", &p.name.to_uppercase(), "--json"]);
    assert_eq!(code, 0, "{json}");
    assert!(lists(&json, "processes", p.pid()), "{json}");

    // the name without its last character is not the name
    let part = &p.name[..p.name.len() - 1];
    let (json, code) = run_json(&["by", part, "--json"]);
    assert_eq!(code, 2, "{json}");
    assert_eq!(json["success"], false, "{json}");
}

#[test]
fn by_cmd_matches_a_part_of_the_name() {
    let p = Owned::listener();
    let part = &p.name[..p.name.len() - 1];
    let (json, code) = run_json(&["by", part, "--cmd", "--json"]);
    assert_eq!(code, 0, "{json}");
    assert!(lists(&json, "processes", p.pid()), "{json}");
}

#[test]
fn by_ignores_a_word_only_in_the_command_line_and_says_cmd_would_find_it() {
    let word = unused_word();
    let p = Owned::with_args(&[&word]);

    let output = proc_cmd().args(["by", &word]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let err = String::from_utf8(output.stderr).unwrap();
    assert!(err.contains("No process found matching"), "{err}");
    assert!(
        err.contains(&format!(
            "1 process has '{word}' in its command line (add --cmd to match those)"
        )),
        "{err}"
    );

    let (json, code) = run_json(&["by", &word, "--cmd", "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["count"], 1, "{json}");
    assert!(lists(&json, "processes", p.pid()), "{json}");
}

#[test]
fn by_an_empty_name_is_invalid_input_not_a_search() {
    // "" names nothing; it was a search that found nothing (exit 2), and every
    // command line contains it, so it must not count them either
    let output = proc_cmd().args(["by", ""]).output().unwrap();
    assert_eq!(output.status.code(), Some(4));
    let err = String::from_utf8(output.stderr).unwrap();
    assert!(err.contains("can't be empty"), "stderr: {err}");
    assert!(!err.contains("command line"), "stderr: {err}");
}

#[test]
fn by_finds_a_long_name_that_linux_cuts_through_the_executable_name() {
    // Linux reports only the first 15 characters of the name; the executable's
    // file name has the rest
    let p = Owned::long_named();
    assert!(p.name.len() > 15, "{}", p.name);
    let (json, code) = run_json(&["by", &p.name, "--json"]);
    assert_eq!(code, 0, "{json}");
    assert!(lists(&json, "processes", p.pid()), "{json}");
}

#[test]
fn the_by_filters_follow_the_same_rule_as_a_name_target() {
    let word = unused_word();
    let p = Owned::with_args(&[&word]);
    let part = p.name[..p.name.len() - 1].to_string();
    let exe = p.exe();
    let dir = p.dir();

    // `for FILE --by X` and `in DIR --by X`: the whole name only
    for by in [p.name.as_str(), &p.name.to_uppercase()] {
        let (json, code) = run_json(&["for", &exe, "--by", by, "--json"]);
        assert_eq!(code, 0, "{json}");
        let (json, code) = run_json(&["in", &dir, "--by", by, "--json"]);
        assert_eq!(code, 0, "{json}");
        assert!(lists(&json, "processes", p.pid()), "{json}");
    }
    for by in [part.as_str(), word.as_str()] {
        let (json, code) = run_json(&["for", &exe, "--by", by, "--json"]);
        assert_eq!(code, 2, "for --by {by}: {json}");
        let (json, code) = run_json(&["in", &dir, "--by", by, "--json"]);
        assert_eq!(code, 2, "in --by {by}: {json}");
    }

    // with --cmd, a part of the name or a word of the command line
    for by in [part.as_str(), word.as_str()] {
        let (json, code) = run_json(&["for", &exe, "--by", by, "--cmd", "--json"]);
        assert_eq!(code, 0, "for --by {by} --cmd: {json}");
        let (json, code) = run_json(&["in", &dir, "--by", by, "--cmd", "--json"]);
        assert_eq!(code, 0, "in --by {by} --cmd: {json}");
        assert!(lists(&json, "processes", p.pid()), "{json}");
    }
}

#[test]
fn by_in_keeps_only_processes_in_that_directory() {
    let p = Owned::listener();
    let (json, _) = run_json(&["by", &p.name, "--in", &p.dir(), "--json"]);
    assert!(lists(&json, "processes", p.pid()), "{json}");

    let (_other, other_dir) = fresh_dir();
    let (json, code) = run_json(&[
        "by",
        &p.name,
        "--in",
        &other_dir.to_string_lossy(),
        "--json",
    ]);
    assert_eq!(code, 0, "{json}");
    assert!(!lists(&json, "processes", p.pid()), "{json}");
}

#[test]
fn by_min_cpu_keeps_a_busy_process_and_drops_an_idle_one() {
    let idle = Owned::listener();
    let (json, _) = run_json(&["by", &idle.name, "--min-cpu", "50", "--json"]);
    assert!(!lists(&json, "processes", idle.pid()), "{json}");

    // a full core measured can read under 50 on a loaded CI runner (49% seen)
    let busy = Owned::busy();
    let (json, _) = run_json_until(&["by", &busy.name, "--min-cpu", "20", "--json"], |json| {
        lists(json, "processes", busy.pid())
    });
    assert!(lists(&json, "processes", busy.pid()), "{json}");
}

#[test]
fn by_min_mem_drops_a_process_using_less() {
    let p = Owned::listener();
    let (json, _) = run_json(&["by", &p.name, "--min-mem", "1000000", "--json"]);
    assert!(!lists(&json, "processes", p.pid()), "{json}");
}

#[test]
fn by_status_drops_a_process_in_another_state() {
    let p = Owned::listener();
    let (json, code) = run_json(&["by", &p.name, "--status", "zombie", "--json"]);
    assert_eq!(code, 0, "{json}");
    assert!(!lists(&json, "processes", p.pid()), "{json}");
}

#[test]
fn by_status_keeps_a_process_in_that_state() {
    let p = Owned::listener();
    let status = status_of(&p);
    let (json, code) = run_json(&["by", &p.name, "--status", &status, "--json"]);
    assert_eq!(code, 0, "{json}");
    assert!(lists(&json, "processes", p.pid()), "{json}");
}

#[test]
fn by_min_uptime_drops_a_process_that_is_younger() {
    let p = Owned::listener();
    let (json, _) = run_json(&["by", &p.name, "--min-uptime", "0", "--json"]);
    assert!(lists(&json, "processes", p.pid()), "{json}");

    let (json, _) = run_json(&["by", &p.name, "--min-uptime", "99999999", "--json"]);
    assert!(!lists(&json, "processes", p.pid()), "{json}");
}

#[test]
fn by_parent_keeps_only_children_of_that_pid() {
    let p = Owned::listener();
    let me = std::process::id().to_string();
    let (json, _) = run_json(&["by", &p.name, "--parent", &me, "--json"]);
    assert!(lists(&json, "processes", p.pid()), "{json}");

    let (json, _) = run_json(&["by", &p.name, "--parent", "4294967295", "--json"]);
    assert!(!lists(&json, "processes", p.pid()), "{json}");
}

// ---- for ----

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
fn for_one_hit_shows_the_card() {
    let p = Owned::listener();
    let (out, code) = run_text(&["for", &p.exe()]);
    assert_eq!(code, 0, "{out}");
    let head = format!("✓ Found 1 process for {}\n\n  Process:     ", p.exe());
    assert!(out.starts_with(&head), "{out}");
    let file_name = Path::new(&p.exe())
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        card_field(&out, "Process:"),
        format!("{file_name} (PID {})", p.pid())
    );
    // `-v` for the directory whole: a long one is cut to the terminal otherwise
    let (verbose, _) = run_text(&["for", &p.exe(), "-v"]);
    // shown as the OS gives it: Windows ends a working directory with `\`
    let shown = card_field(&verbose, "Directory:");
    assert_eq!(shown.trim_end_matches('\\'), p.dir(), "{verbose}");
    assert!(
        card_field(&out, "Listening:").starts_with(&format!(":{} TCP", p.port)),
        "{out}"
    );
    assert!(card_field(&out, "Resources:").contains("% CPU, "), "{out}");
    assert!(card_field(&out, "Started:").contains("(up "), "{out}");
    assert!(!card_field(&out, "Command:").is_empty(), "{out}");
    assert!(out.contains("  Connections: "), "{out}");
    assert!(out.ends_with("\n\n"), "{out:?}");
    // the old labels went with the old layout
    assert!(!out.contains("Uptime:"), "{out}");
}

#[test]
fn for_status_keeps_a_process_in_that_state() {
    let p = Owned::listener();
    let (by, _) = run_json(&["by", &p.name, "--json"]);
    let status = by["processes"][0]["status"].as_str().unwrap().to_string();
    let (json, code) = run_json(&["for", &p.exe(), "--status", &status, "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["results"][0]["process"]["pid"], p.pid(), "{json}");
}

#[test]
fn for_verbose_prints_the_card_values_whole() {
    let long = "x".repeat(300);
    let p = Owned::with_args(&[&long]);
    let (out, code) = run_text(&["for", &p.exe()]);
    assert_eq!(code, 0, "{out}");
    let command = card_field(&out, "Command:");
    assert!(command.ends_with('…'), "cut to the terminal: {command}");
    let (out, code) = run_text(&["for", &p.exe(), "--verbose"]);
    assert_eq!(code, 0, "{out}");
    assert!(card_field(&out, "Command:").ends_with(&long), "{out}");
}

#[test]
fn for_several_processes_shows_a_table_and_who_listens_where() {
    let first = Owned::listener();
    let second = Owned::sharing_exe_with(&first);
    let (out, code) = run_text(&["for", &first.exe()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Found 2 processes for"), "{out}");
    assert!(out.contains(&first.pid().to_string()), "{out}");
    assert!(out.contains(&second.pid().to_string()), "{out}");
    assert!(out.contains("Listening:"), "{out}");
    assert!(out.contains(&format!(":{}", first.port)), "{out}");
    assert!(out.contains(&format!(":{}", second.port)), "{out}");
}

/// Several processes with no port to list: the table, and no `Listening:` list
#[test]
fn for_several_processes_that_listen_on_nothing_shows_only_the_table() {
    let first = Owned::with_args(&["--no-port"]);
    let second = Owned::start(&["--no-port"], Some(&first.exe));
    let (out, code) = run_text(&["for", &first.exe()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Found 2 processes for"), "{out}");
    assert!(out.contains(&first.pid().to_string()), "{out}");
    assert!(out.contains(&second.pid().to_string()), "{out}");
    assert!(!out.contains("Listening:"), "{out}");
}

#[test]
fn for_json_lists_each_process_with_its_ports() {
    let p = Owned::listener();
    let (json, code) = run_json(&["for", &p.exe(), "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["action"], "for");
    assert_eq!(json["count"], 1, "{json}");
    let found = &json["results"][0];
    assert_eq!(found["process"]["pid"], p.pid());
    assert_eq!(found["ports"][0]["port"], p.port, "{json}");
}

#[test]
fn for_resolves_a_relative_path_from_the_current_directory() {
    let p = Owned::listener();
    let file = p.exe.file_name().unwrap().to_string_lossy().into_owned();
    let output = proc_cmd()
        .current_dir(&p.dir)
        .args(["for", &file, "--json"])
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(0), "{json}");
    assert_eq!(json["results"][0]["process"]["pid"], p.pid(), "{json}");
}

#[test]
fn for_expands_a_tilde_to_home() {
    let p = Owned::listener();
    let file = p.exe.file_name().unwrap().to_string_lossy().into_owned();
    let output = proc_cmd()
        .env("HOME", &p.dir)
        .args(["for", &format!("~/{file}"), "--json"])
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(0), "{json}");
    assert_eq!(json["results"][0]["process"]["pid"], p.pid(), "{json}");
}

#[test]
fn a_bare_tilde_is_home() {
    // HOME is a directory that isn't there, so the lookup ends as a missing
    // file, which names `~` and shows it was read as HOME
    let output = proc_cmd()
        .env("HOME", "/no/such/home/for/proc")
        .args(["for", "~", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json["error"]
        .as_str()
        .unwrap()
        .contains("File not found: ~"));
}

#[test]
fn a_tilde_path_with_no_home_is_left_as_written() {
    let output = proc_cmd()
        .env_remove("HOME")
        .args(["for", "~/no-such-file-for-proc", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let error = json["error"].as_str().unwrap();
    assert!(
        error.contains("File not found: ~/no-such-file-for-proc"),
        "{error}"
    );
}

#[test]
fn for_a_file_nothing_runs_or_holds_is_not_found() {
    let (_tmp, dir) = fresh_dir();
    let file = dir.join("idle.txt");
    std::fs::write(&file, "nothing opens this").unwrap();
    let (json, code) = run_json(&["for", &file.to_string_lossy(), "--json"]);
    assert_eq!(code, 2, "{json}");
    assert_eq!(json["success"], false);
}

#[test]
fn for_filters_drop_what_does_not_match() {
    let p = Owned::listener();
    let exe = p.exe();

    // Everything is filtered out, which is "not found"
    let (_other, other_dir) = fresh_dir();
    for args in [
        vec!["--in", other_dir.to_str().unwrap()],
        vec!["--min-cpu", "50"],
        vec!["--min-mem", "1000000"],
        vec!["--min-uptime", "99999999"],
        vec!["--by", "no-such-process-name"],
    ] {
        let mut argv = vec!["for", &exe, "--json"];
        argv.extend(args.iter());
        let (json, code) = run_json(&argv);
        assert_eq!(code, 2, "{argv:?}: {json}");
    }
}

/// Whether the program the open-file lookup runs is installed: lsof, or the
/// base system's fstat on FreeBSD and NetBSD. Windows has no open-file
/// lookup, and a minimal Linux image has no lsof.
#[cfg(unix)]
fn has_open_file_lookup() -> bool {
    #[cfg(any(target_os = "freebsd", target_os = "netbsd"))]
    let (program, probe) = ("fstat", &["-p", "1"][..]);
    #[cfg(not(any(target_os = "freebsd", target_os = "netbsd")))]
    let (program, probe) = ("lsof", &["-v"][..]);
    let found = std::process::Command::new(program)
        .args(probe)
        .output()
        .is_ok();
    if !found {
        proc_cli::err_println!(
            "{program} is not installed here, so there is no open-file lookup to test"
        );
    }
    found
}

/// A process that has `file` open but isn't running it: `exec sleep` with the
/// file on descriptor 3. Killed and reaped on drop.
#[cfg(unix)]
struct Holder(Child);

#[cfg(unix)]
impl Holder {
    fn start(file: &Path) -> Self {
        Holder(
            std::process::Command::new("sh")
                .args(["-c", "exec sleep 300 3<\"$1\"", "sh"])
                .arg(file)
                .stdin(Stdio::null())
                .spawn()
                .expect("spawn a process that holds the file open"),
        )
    }
}

#[cfg(unix)]
impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Run `proc for <file>` until it lists every PID in `pids`, or give up
#[cfg(unix)]
fn for_lists_all(file: &Path, pids: &[u32]) -> Option<Value> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (json, code) = run_json(&["for", &file.to_string_lossy(), "--json"]);
        let listed = |pid: u32| {
            json["results"].as_array().is_some_and(|items| {
                items
                    .iter()
                    .any(|r| r["process"]["pid"].as_u64() == Some(u64::from(pid)))
            })
        };
        if code == 0 && pids.iter().all(|pid| listed(*pid)) {
            return Some(json);
        }
        if Instant::now() > deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(unix)]
#[test]
fn for_finds_a_process_that_has_the_file_open() {
    if !has_open_file_lookup() {
        return;
    }
    let (_tmp, dir) = fresh_dir();
    let file = dir.join("held-open.txt");
    std::fs::write(&file, "held").unwrap();
    let holder = Holder::start(&file);
    assert!(
        for_lists_all(&file, &[holder.0.id()]).is_some(),
        "the open-file lookup never showed {} holding the file",
        holder.0.id()
    );
}

/// Several processes with no port to list: the table, and no `Listening:` list
#[cfg(unix)]
#[test]
fn for_several_processes_without_ports_shows_only_the_table() {
    if !has_open_file_lookup() {
        return;
    }
    let (_tmp, dir) = fresh_dir();
    let file = dir.join("held-open.txt");
    std::fs::write(&file, "held").unwrap();
    let first = Holder::start(&file);
    let second = Holder::start(&file);
    let pids = [first.0.id(), second.0.id()];
    assert!(
        for_lists_all(&file, &pids).is_some(),
        "the open-file lookup never showed both"
    );

    let (out, code) = run_text(&["for", &file.to_string_lossy()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Found 2 processes for"), "{out}");
    assert!(out.contains(&pids[0].to_string()), "{out}");
    assert!(out.contains(&pids[1].to_string()), "{out}");
    assert!(!out.contains("Listening:"), "{out}");
}

// ---- stuck ----

#[test]
fn stuck_timeout_zero_finds_a_busy_process() {
    let busy = Owned::busy();
    let (json, code) = run_json_until(
        &["stuck", "--timeout", "0", "--in", &busy.dir(), "--json"],
        |json| lists(json, "processes", busy.pid()),
    );
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["action"], "stuck");
    assert!(lists(&json, "processes", busy.pid()), "{json}");
}

#[test]
fn stuck_lists_a_busy_process_and_leaves_it_alone() {
    let mut busy = Owned::busy();
    let (out, code) = run_text_until(&["stuck", "--timeout", "0", "--in", &busy.dir()], |out| {
        mentions(out, busy.pid())
    });
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Found 1 potentially stuck process"), "{out}");
    assert!(out.contains(&busy.pid().to_string()), "{out}");
    assert!(busy.alive());
}

#[test]
fn stuck_leaves_an_idle_process_out() {
    let idle = Owned::listener();
    let (json, code) = run_json(&["stuck", "--timeout", "0", "--in", &idle.dir(), "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["count"], 0, "{json}");
}

#[test]
fn stuck_by_name_keeps_only_that_name() {
    let busy = Owned::busy();
    let dir = busy.dir();
    let part = &busy.name[..busy.name.len() - 1];
    // the whole name only, until --cmd
    let (json, _) = run_json(&["stuck", "-t", "0", "-i", &dir, "--by", part, "--json"]);
    assert!(!lists(&json, "processes", busy.pid()), "{json}");
    let (json, _) = run_json_until(
        &[
            "stuck", "-t", "0", "-i", &dir, "--by", part, "--cmd", "--json",
        ],
        |json| lists(json, "processes", busy.pid()),
    );
    assert!(lists(&json, "processes", busy.pid()), "{json}");
    let (json, _) = run_json_until(
        &["stuck", "-t", "0", "-i", &dir, "--by", &busy.name, "--json"],
        |json| lists(json, "processes", busy.pid()),
    );
    assert!(lists(&json, "processes", busy.pid()), "{json}");
    let (json, _) = run_json(&[
        "stuck",
        "-t",
        "0",
        "-i",
        &dir,
        "--by",
        "no-such-name",
        "--json",
    ]);
    assert!(!lists(&json, "processes", busy.pid()), "{json}");
}

#[test]
fn stuck_kill_dry_run_says_what_it_would_kill() {
    let mut busy = Owned::busy();
    let dir = busy.dir();
    let (out, code) = run_text_until(
        &[
            "stuck",
            "--timeout",
            "0",
            "--in",
            &dir,
            "--kill",
            "--dry-run",
        ],
        |out| mentions(out, busy.pid()),
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Dry run: would kill 1 stuck process"), "{out}");

    let (json, code) = run_json_until(
        &[
            "stuck",
            "--timeout",
            "0",
            "--in",
            &dir,
            "--kill",
            "--dry-run",
            "--json",
        ],
        |json| lists(json, "processes", busy.pid()),
    );
    assert_eq!(code, 0, "{json}");
    assert!(lists(&json, "processes", busy.pid()), "{json}");
    assert!(busy.alive(), "a dry run kills nothing");
}

#[test]
fn stuck_kill_without_yes_and_json_asks_for_yes_and_lists_what_it_would_kill() {
    let mut busy = Owned::busy();
    let (json, code) = run_json_until(
        &[
            "stuck",
            "--timeout",
            "0",
            "--in",
            &busy.dir(),
            "--kill",
            "--json",
        ],
        |json| lists(json, "processes", busy.pid()),
    );
    assert_eq!(code, 4, "{json}");
    assert_eq!(json["success"], false);
    assert_eq!(json["needs_confirmation"], true, "{json}");
    assert!(lists(&json, "processes", busy.pid()), "{json}");
    assert!(busy.alive(), "nothing is killed without --yes");
}

#[test]
fn stuck_kill_yes_kills_the_stuck_process() {
    let mut busy = Owned::busy();
    let (out, code) = run_text_until(
        &[
            "stuck",
            "--timeout",
            "0",
            "--in",
            &busy.dir(),
            "--kill",
            "--yes",
        ],
        |out| mentions(out, busy.pid()),
    );
    assert_eq!(code, 0, "{out}");
    assert!(busy.ended_within(Duration::from_secs(5)), "{out}");
}

#[test]
fn stuck_kill_yes_json_reports_the_kill() {
    let mut busy = Owned::busy();
    let (json, code) = run_json_until(
        &[
            "stuck",
            "--timeout",
            "0",
            "--in",
            &busy.dir(),
            "--kill",
            "--yes",
            "--json",
        ],
        |json| names(json, busy.pid()),
    );
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["action"], "stuck");
    assert_eq!(json["success"], true, "{json}");
    assert!(busy.ended_within(Duration::from_secs(5)), "{json}");
}
