//! `watch` without a terminal: the NDJSON stream, the one-shot table, and what
//! its targets, filters and sort keys select.
//!
//! The terminal screen is in tests/terminal.rs. Every process a test looks for
//! is one it started: a copy of the listener example (`examples/listener.rs`)
//! under a name and path of its own, in a fresh temp directory, so a name or
//! `--in` filter matches only it. A stream is read frame by frame with a
//! deadline, and the child is killed and reaped on every path out of a test.

use assert_cmd::Command;
use proc_cli::core::filters::strip_verbatim_prefix;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// How long a test waits for a frame or an exit before it gives up
const LIMIT: Duration = Duration::from_secs(60);

fn proc_cmd() -> Command {
    let mut cmd = Command::cargo_bin("proc").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd
}

/// `target/debug/examples/listener`, found relative to this test binary
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
fn unique_name() -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    // Ends in `x`, so no name is the start of another (`-1x` in `-10x`)
    format!("pcw{}n{n}x", std::process::id())
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
                std::thread::sleep(Duration::from_millis(20));
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
    name: String,
    port: u16,
    _tmp: tempfile::TempDir,
    /// The directory as processes report it: symlinks resolved, no Windows
    /// `\\?\` prefix
    dir: PathBuf,
}

impl Owned {
    fn listener() -> Self {
        Self::listener_named(&unique_name())
    }

    fn listener_named(name: &str) -> Self {
        Self::start_named(name, &[])
    }

    /// A listener started with extra arguments (it ignores them, so they only
    /// put words in its command line)
    fn listener_with_args(args: &[&str]) -> Self {
        Self::start_named(&unique_name(), args)
    }

    fn start_named(name: &str, args: &[&str]) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let raw_dir = tmp.path().to_path_buf();
        let dir = raw_dir.canonicalize().unwrap();
        let dir = PathBuf::from(strip_verbatim_prefix(&dir.to_string_lossy()));
        let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        std::fs::copy(listener_path(), raw_dir.join(&file)).expect("copy the listener example");
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
            name: name.to_string(),
            port,
            _tmp: tmp,
            dir,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn dir(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }

    fn port_arg(&self) -> String {
        format!(":{}", self.port)
    }

    /// End the process and reap it, as a test does to make one disappear
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        self.stop();
    }
}

/// `proc watch <args> --json`, read one frame at a time. The child is killed
/// and reaped on drop.
struct Stream {
    child: Child,
    frames: Option<Receiver<String>>,
}

impl Stream {
    fn start(args: &[&str]) -> Self {
        let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("proc"))
            .arg("watch")
            .args(args)
            .arg("--json")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn proc watch --json");
        let stdout = child.stdout.take().unwrap();
        let (sender, frames) = mpsc::channel();
        // Ends at end-of-file, or once nobody receives any more lines (which
        // closes the pipe and lets the child end by itself)
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Stream {
            child,
            frames: Some(frames),
        }
    }

    /// Stop reading and wait for the child to end by itself: it finds the pipe
    /// closed when it writes its next frame and exits 0. Unlike a kill, this
    /// lets it write the coverage profile of the run.
    fn finish(mut self) {
        drop(self.frames.take());
        let deadline = Instant::now() + LIMIT;
        loop {
            if let Some(status) = self.child.try_wait().expect("poll proc watch") {
                assert!(status.success(), "proc watch ended with {status:?}");
                return;
            }
            assert!(
                Instant::now() < deadline,
                "proc watch kept streaming into a closed pipe"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The next frame, parsed. Every line of the stream must be one JSON document.
    fn next(&self) -> Value {
        let frames = self.frames.as_ref().expect("the stream is open");
        let line = match frames.recv_timeout(LIMIT) {
            Ok(line) => line,
            Err(RecvTimeoutError::Timeout) => panic!("no frame in {LIMIT:?}"),
            Err(RecvTimeoutError::Disconnected) => panic!("the stream ended"),
        };
        let json: Value = serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("a stream line is not JSON ({e}): {line}"));
        assert_eq!(json["action"], "watch", "{json}");
        assert_eq!(json["success"], true, "{json}");
        assert_eq!(
            json["count"].as_u64().unwrap() as usize,
            json["processes"].as_array().unwrap().len(),
            "count matches the list: {json}"
        );
        json
    }

    /// Frames until one satisfies `done`; the limit is on time, not frames
    fn until(&self, what: &str, done: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + LIMIT;
        loop {
            let frame = self.next();
            if done(&frame) {
                return frame;
            }
            assert!(
                Instant::now() < deadline,
                "no frame with {what} in {LIMIT:?}; last was {frame}"
            );
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The first frame of `proc watch <args> --json`, with the child left to end
/// by itself (a short interval, so that is quick)
fn first_frame(args: &[&str]) -> Value {
    let mut args = args.to_vec();
    args.extend(["--interval", "0.1"]);
    let stream = Stream::start(&args);
    let frame = stream.next();
    stream.finish();
    frame
}

fn pids(frame: &Value) -> Vec<u64> {
    frame["processes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["pid"].as_u64().unwrap())
        .collect()
}

fn lists(frame: &Value, pid: u32) -> bool {
    pids(frame).contains(&u64::from(pid))
}

// ---- the stream ----

#[test]
fn the_stream_sends_a_frame_after_frame() {
    let owned = Owned::listener();
    let pid = owned.pid().to_string();
    let stream = Stream::start(&[&pid, "--interval", "0.1"]);
    for _ in 0..4 {
        let frame = stream.next();
        assert_eq!(pids(&frame), [u64::from(owned.pid())], "{frame}");
        // the process name is the executable's file name: `.exe` on Windows
        let name = format!("{}{}", owned.name, std::env::consts::EXE_SUFFIX);
        assert_eq!(frame["processes"][0]["name"], name.as_str(), "{frame}");
    }
    stream.finish();
}

#[test]
fn the_interval_is_the_time_between_frames() {
    let owned = Owned::listener();
    let pid = owned.pid().to_string();
    let stream = Stream::start(&[&pid, "-n", "1"]);
    stream.next();
    let first = Instant::now();
    stream.next();
    let gap = first.elapsed();
    // One second is slept between frames; allow for the first frame having
    // been read late
    assert!(
        gap >= Duration::from_millis(700),
        "the next frame came after {gap:?}"
    );
    stream.finish();
}

#[test]
fn the_stream_ends_cleanly_when_its_reader_goes_away() {
    let owned = Owned::listener();
    let pid = owned.pid().to_string();
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("proc"))
        .args(["watch", &pid, "--json", "--interval", "0.1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn proc watch --json");
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    let read = stdout.read_line(&mut first);
    // Close our end of the pipe, as `| head -1` does
    drop(stdout);

    let deadline = Instant::now() + LIMIT;
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll proc watch") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("proc watch kept streaming into a closed pipe");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut stderr = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut stderr).unwrap();
    read.expect("read the first frame");
    assert!(first.contains("\"action\":\"watch\""), "{first}");
    assert!(status.success(), "exit {status:?}; stderr: {stderr}");
    assert_eq!(stderr, "", "a closed pipe is not an error");
}

#[test]
fn a_process_shows_up_and_goes_away_between_frames() {
    let name = unique_name();
    let stream = Stream::start(&[&name, "--interval", "0.1"]);
    let before = stream.next();
    assert_eq!(before["count"], 0, "nothing is called {name} yet: {before}");

    let mut owned = Owned::listener_named(&name);
    let pid = owned.pid();
    let seen = stream.until("the new process", |f| lists(f, pid));
    assert_eq!(seen["count"], 1, "{seen}");

    owned.stop();
    let gone = stream.until("the process gone", |f| f["count"] == 0);
    assert!(!lists(&gone, pid), "{gone}");
    stream.finish();
}

// ---- targets ----

#[test]
fn a_pid_target_watches_that_process() {
    let owned = Owned::listener();
    let frame = first_frame(&[&owned.pid().to_string()]);
    assert_eq!(pids(&frame), [u64::from(owned.pid())], "{frame}");
}

#[test]
fn a_port_target_watches_the_process_on_that_port() {
    let owned = Owned::listener();
    let frame = first_frame(&[&owned.port_arg()]);
    assert_eq!(pids(&frame), [u64::from(owned.pid())], "{frame}");
}

#[test]
fn a_name_target_watches_the_processes_with_that_name() {
    let owned = Owned::listener();
    let frame = first_frame(&[&owned.name]);
    assert_eq!(pids(&frame), [u64::from(owned.pid())], "{frame}");
}

#[test]
fn a_name_target_is_the_whole_name_and_cmd_widens_it_to_the_command_line() {
    let word = unique_name();
    let owned = Owned::listener_with_args(&[&word]);
    let part = &owned.name[..owned.name.len() - 1];

    // the name in any case; a part of it, or a word of the command line, is nothing
    let frame = first_frame(&[&owned.name.to_uppercase()]);
    assert_eq!(pids(&frame), [u64::from(owned.pid())], "{frame}");
    assert_eq!(first_frame(&[part])["count"], 0);
    assert_eq!(first_frame(&[&word])["count"], 0);

    // with --cmd both find it
    for text in [part, word.as_str()] {
        let frame = first_frame(&[text, "--cmd"]);
        assert!(lists(&frame, owned.pid()), "{text}: {frame}");
    }

    // --by follows the same rule
    let pid = owned.pid().to_string();
    assert_eq!(first_frame(&[&pid, "--by", part])["count"], 0);
    let frame = first_frame(&[&pid, "--by", part, "--cmd"]);
    assert_eq!(pids(&frame), [u64::from(owned.pid())], "{frame}");
}

#[test]
fn several_targets_are_watched_together_and_listed_once() {
    let first = Owned::listener();
    let second = Owned::listener();
    let target = format!(
        "{},{},{},{}",
        first.pid(),
        first.port_arg(),
        first.name,
        second.name
    );
    let frame = first_frame(&["--sort", "pid", &target]);
    let mut want = vec![u64::from(first.pid()), u64::from(second.pid())];
    want.sort_unstable();
    assert_eq!(pids(&frame), want, "{frame}");
}

#[test]
fn several_targets_can_be_separate_arguments() {
    let first = Owned::listener();
    let second = Owned::listener();
    let mut want = vec![u64::from(first.pid()), u64::from(second.pid())];
    want.sort_unstable();

    let (pid, port) = (first.pid().to_string(), first.port_arg());
    let frame = first_frame(&["--sort", "pid", &pid, &port, &first.name, &second.name]);
    assert_eq!(pids(&frame), want, "{frame}");

    // a comma list mixes with them
    let list = format!("{},{}", first.name, second.name);
    let frame = first_frame(&["--sort", "pid", &pid, &list]);
    assert_eq!(pids(&frame), want, "{frame}");
}

#[test]
fn a_target_nothing_matches_is_an_empty_frame() {
    let nobody = unique_name();
    let frame = first_frame(&["99999999"]);
    assert_eq!(frame["count"], 0, "{frame}");
    assert_eq!(first_frame(&[&nobody])["count"], 0);
}

#[test]
fn a_port_nobody_listens_on_is_an_empty_frame() {
    let mut owned = Owned::listener();
    let port = owned.port_arg();
    owned.stop();
    let frame = first_frame(&[&port]);
    assert_eq!(frame["count"], 0, "{frame}");
}

// ---- filters ----

#[test]
fn in_keeps_the_processes_started_in_that_directory() {
    let owned = Owned::listener();
    let other = Owned::listener();
    let frame = first_frame(&["--in", &owned.dir()]);
    assert_eq!(pids(&frame), [u64::from(owned.pid())], "{frame}");
    assert!(!lists(&frame, other.pid()), "{frame}");

    // With a target too, both must hold
    let both = first_frame(&[&other.pid().to_string(), "-i", &owned.dir()]);
    assert_eq!(both["count"], 0, "{both}");
}

#[test]
fn by_keeps_the_processes_with_that_name() {
    let owned = Owned::listener();
    let other = Owned::listener();
    let pids_target = format!("{},{}", owned.pid(), other.pid());
    let frame = first_frame(&["--sort", "pid", &pids_target, "--by", &owned.name]);
    assert_eq!(pids(&frame), [u64::from(owned.pid())], "{frame}");
    let none = first_frame(&[&owned.pid().to_string(), "-b", &unique_name()]);
    assert_eq!(none["count"], 0, "{none}");
}

#[test]
fn min_cpu_keeps_only_the_processes_above_it() {
    let owned = Owned::listener();
    let pid = owned.pid().to_string();
    let all = first_frame(&[&pid, "--min-cpu", "0"]);
    assert_eq!(pids(&all), [u64::from(owned.pid())], "{all}");
    // An idle listener is far below this
    let busy = first_frame(&[&pid, "--min-cpu", "1000"]);
    assert_eq!(busy["count"], 0, "{busy}");
}

#[test]
fn min_mem_keeps_only_the_processes_above_it() {
    let owned = Owned::listener();
    let pid = owned.pid().to_string();
    let all = first_frame(&[&pid, "--min-mem", "0"]);
    assert_eq!(pids(&all), [u64::from(owned.pid())], "{all}");
    // A terabyte, in MB
    let big = first_frame(&[&pid, "--min-mem", "1000000"]);
    assert_eq!(big["count"], 0, "{big}");
}

#[test]
fn limit_shows_only_the_first_few() {
    let frame = first_frame(&["--limit", "3"]);
    assert_eq!(frame["count"], 3, "{frame}");
    assert_eq!(frame["processes"].as_array().unwrap().len(), 3);
}

// ---- sort keys ----

#[test]
fn sort_by_pid_puts_the_lowest_first() {
    let frame = first_frame(&["--sort", "pid"]);
    let all = pids(&frame);
    assert!(all.len() > 1, "{frame}");
    assert!(all.windows(2).all(|w| w[0] <= w[1]), "{all:?}");
}

#[test]
fn sort_by_name_is_alphabetical_ignoring_case() {
    let frame = first_frame(&["-s", "name"]);
    let names: Vec<String> = frame["processes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_lowercase())
        .collect();
    assert!(names.len() > 1, "{frame}");
    assert!(names.windows(2).all(|w| w[0] <= w[1]), "{names:?}");
}

#[test]
fn sort_by_mem_puts_the_biggest_first() {
    let frame = first_frame(&["--sort", "mem"]);
    let mem: Vec<f64> = frame["processes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["memory_mb"].as_f64().unwrap())
        .collect();
    assert!(mem.len() > 1, "{frame}");
    assert!(mem.windows(2).all(|w| w[0] >= w[1]), "{mem:?}");
}

#[test]
fn sort_by_cpu_puts_the_busiest_first_and_is_the_default() {
    for args in [&["--sort", "cpu"][..], &[][..]] {
        let frame = first_frame(args);
        let cpu: Vec<f64> = frame["processes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["cpu_percent"].as_f64().unwrap())
            .collect();
        assert!(cpu.len() > 1, "{frame}");
        assert!(cpu.windows(2).all(|w| w[0] >= w[1]), "{args:?}: {cpu:?}");
    }
}

// ---- a process with no working directory ----

// Only Linux lists a zombie: macOS drops it from the process list
#[cfg(target_os = "linux")]
#[test]
fn a_zombie_has_no_directory_so_in_drops_it() {
    // A child that has exited and not been waited for is a zombie
    let mut zombie = std::process::Command::new("sh")
        .args(["-c", "exit 0"])
        .stdin(Stdio::null())
        .spawn()
        .expect("spawn a shell");
    let pid = zombie.id().to_string();
    let dir = tempfile::tempdir().unwrap();
    let dir_arg = dir
        .path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let deadline = Instant::now() + LIMIT;
    let seen = loop {
        let frame = first_frame(&[&pid]);
        if frame["count"] == 1 && frame["processes"][0]["status"] == "zombie" {
            break frame;
        }
        if Instant::now() >= deadline {
            let _ = zombie.wait();
            panic!("the exited shell was never listed as a zombie: {frame}");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let filtered = first_frame(&[&pid, "--in", &dir_arg]);
    let _ = zombie.wait();
    assert_eq!(
        seen["processes"][0]["pid"].as_u64().unwrap().to_string(),
        pid
    );
    assert_eq!(filtered["count"], 0, "{filtered}");
}

// ---- the one-shot table, without a terminal and without --json ----

#[test]
fn without_a_terminal_it_prints_one_table_and_exits() {
    let owned = Owned::listener();
    let output = proc_cmd()
        .args(["watch", &owned.pid().to_string()])
        .timeout(LIMIT)
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Found 1 process"), "{text}");
    assert_eq!(
        text.matches(&owned.pid().to_string()).count(),
        1,
        "one row: {text}"
    );
}

#[test]
fn the_one_shot_table_takes_the_same_targets() {
    let owned = Owned::listener();
    for target in [owned.port_arg(), owned.name.clone()] {
        let output = proc_cmd()
            .args(["watch", &target])
            .timeout(LIMIT)
            .output()
            .unwrap();
        assert!(output.status.success(), "{target}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains(&owned.pid().to_string()), "{target}: {text}");
    }
}

#[test]
fn the_one_shot_table_can_be_verbose() {
    let owned = Owned::listener();
    let output = proc_cmd()
        .args(["watch", &owned.pid().to_string(), "--verbose"])
        .timeout(LIMIT)
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains(&owned.pid().to_string()), "{text}");
    assert!(text.contains(&owned.name), "{text}");
}

/// The column names of the table `proc <args>` prints
fn table_header(args: &[&str]) -> Vec<String> {
    let output = proc_cmd().args(args).timeout(LIMIT).output().unwrap();
    assert!(output.status.success(), "{args:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    text.lines()
        .find(|line| line.contains("PID") && line.contains("COMMAND"))
        .unwrap_or_else(|| panic!("no table header for {args:?}:\n{text}"))
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

#[test]
fn watch_uses_the_same_columns() {
    let owned = Owned::listener();
    let pid = owned.pid().to_string();
    let dir = owned.dir();
    // The same header as `by` prints, default, with -v and scoped to a directory
    for extra in [vec![], vec!["--verbose"], vec!["--in", &dir]] {
        let mut watch = vec!["watch", &pid];
        watch.extend(&extra);
        let mut by = vec!["by", &owned.name];
        by.extend(&extra);
        let columns = table_header(&watch);
        assert_eq!(columns, table_header(&by), "{extra:?}");
        assert_eq!(columns[0], "PID");
        assert_eq!(
            columns[1],
            if extra.contains(&"--in") {
                "DIR"
            } else {
                "NAME"
            }
        );
        assert_eq!(columns.last().unwrap(), "COMMAND");
        assert_eq!(
            columns.contains(&"PPID".to_string()),
            extra == ["--verbose"]
        );
    }
}

#[test]
fn watch_with_a_bad_interval_is_refused() {
    let output = proc_cmd()
        .args(["watch", "--interval", "0.01"])
        .timeout(LIMIT)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
}
