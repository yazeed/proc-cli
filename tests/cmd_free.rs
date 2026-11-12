//! `free`, driven through the real binary.
//!
//! Every port these tests free is one a listener the test started holds (the
//! listener example, `examples/listener.rs`), named as `:<port>`. Each process
//! is killed and reaped on drop.

mod built;
mod contract;

use assert_cmd::Command;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Stdio};
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

/// `target/debug/examples/listener`
fn listener_path() -> PathBuf {
    built::example("listener")
}

/// A port nothing listens on, from below every OS's ephemeral range (Linux
/// starts at 32768, macOS and Windows at 49152), so a listener another test
/// binds to port 0 can never be handed it. A port the OS just handed out and
/// took back could be: on CI a parallel test's listener got one between the
/// lookup and proc's run. A bind alone doesn't show the port is free: on macOS
/// binding 0.0.0.0 succeeds while another process listens on 127.0.0.1 there
/// (std sets SO_REUSEADDR; measured), so nothing may answer on loopback either
fn unused_port() -> u16 {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let start = 20_000 + std::process::id() as usize % 10_000;
    loop {
        let port = start + NEXT.fetch_add(1, Ordering::SeqCst);
        assert!(port < 32_768, "no unused port below the ephemeral range");
        let port = port as u16;
        if TcpListener::bind(("0.0.0.0", port)).is_ok() && !answers_on_loopback(port) {
            return port;
        }
    }
}

/// Whether something accepts a connection on 127.0.0.1 or [::1] at `port`
fn answers_on_loopback(port: u16) -> bool {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
    [
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
    ]
    .into_iter()
    .any(|host| {
        TcpStream::connect_timeout(&SocketAddr::new(host, port), Duration::from_millis(500)).is_ok()
    })
}

/// A listener the test owns. Killed and reaped on drop, whatever a test did to it.
struct Owned {
    child: Child,
    port: u16,
    /// With `--respawn`: the process group (the parent's PID) that holds both the parent
    /// and the forked copy that takes the port after the parent is gone. Killing
    /// the group needs no PID read from the child's output, and signals no PID
    /// individually, so it can't hit a PID that has been reused.
    #[cfg_attr(not(unix), allow(dead_code))]
    group: Option<i32>,
    /// With `--respawn` on Windows, which has no fork or process group: the PID
    /// of the copy the listener started, which takes the port after the parent
    /// is gone. Read from the listener's own `CHILD <pid>` line, and stopped on
    /// drop by that PID, only while it is still a `listener` process.
    #[cfg(windows)]
    takeover: std::cell::Cell<Option<u32>>,
}

impl Owned {
    fn listener() -> Self {
        Self::start(&[])
    }

    fn start(args: &[&str]) -> Self {
        let mut command = std::process::Command::new(listener_path());
        command
            .args(args)
            .stdout(Stdio::piped())
            .stdin(Stdio::null());
        #[cfg(unix)]
        if args.contains(&"--respawn") {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().expect("spawn the listener example");
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
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
        let child_id = child.id();
        let owned = Owned {
            child,
            port,
            group: args
                .contains(&"--respawn")
                .then(|| i32::try_from(child_id).expect("pid fits")),
            #[cfg(windows)]
            takeover: std::cell::Cell::new(None),
        };
        // Checked only now, with `owned` already holding the group: a panic here
        // drops it, which kills the parent and the forked copy
        if args.contains(&"--respawn") {
            let mut line = String::new();
            let _ = stdout.read_line(&mut line);
            let child_pid = line
                .trim()
                .strip_prefix("CHILD ")
                .and_then(|p| p.parse::<u32>().ok());
            assert!(child_pid.is_some(), "expected `CHILD <pid>`, got {line:?}");
            #[cfg(windows)]
            owned.takeover.set(child_pid);
        }
        owned
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn target(&self) -> String {
        format!(":{}", self.port)
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
        #[cfg(unix)]
        if let Some(group) = self.group {
            // SAFETY: the group is the one this test made for its listener and
            // the forked copy; once both are gone the call just fails
            unsafe {
                libc::kill(-group, libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        if let Some(pid) = self.takeover.get() {
            // The copy was started by the listener this test started. The image
            // filter keeps a reused PID from being anything but a listener.
            let _ = std::process::Command::new("taskkill")
                .args([
                    "/F",
                    "/PID",
                    &pid.to_string(),
                    "/FI",
                    "IMAGENAME eq listener.exe",
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
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

#[test]
fn free_only_takes_ports() {
    let (out, code) = run_text(&["free", "1234"]);
    assert_eq!(code, 4, "{out}");

    let (json, code) = run_json(&["free", "node", "--json"]);
    assert_eq!(code, 4, "{json}");
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("only works with port targets"), "{error}");
    assert!(error.contains("got 'node'"), "{error}");
}

#[test]
fn free_a_port_nothing_holds_is_already_done() {
    // --dry-run, not --yes: the port was free when picked, but another program
    // could take it before proc runs, and this test must not kill that. A port
    // nothing holds returns before the dry-run check, so the same lines run
    let port = format!(":{}", unused_port());
    let (out, code) = run_text(&["free", &port, "--dry-run"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("No process listening on port"), "{out}");
    assert!(
        out.contains("All specified ports are already free"),
        "{out}"
    );

    let (json, code) = run_json(&["free", &port, "--dry-run", "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["action"], "free");
    assert_eq!(json["count"], 0, "{json}");
}

#[test]
fn free_dry_run_warns_about_the_free_port_and_kills_nothing() {
    let mut held = Owned::listener();
    let both = format!("{},:{}", held.target(), unused_port());
    let (out, code) = run_text(&["free", &both, "--dry-run"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("No process listening on port"), "{out}");
    assert!(
        out.contains("Dry run: would kill 1 process to free 1 port"),
        "{out}"
    );
    assert!(held.alive(), "a dry run kills nothing");
}

#[test]
fn free_takes_several_ports_as_separate_arguments() {
    let mut first = Owned::listener();
    let mut second = Owned::listener();
    let (json, code) = run_json(&["free", &first.target(), &second.target(), "--yes", "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["success"], true, "{json}");
    assert_eq!(json["results"].as_array().unwrap().len(), 2, "{json}");
    // `processes` lists the holders in the order the ports were typed
    assert_eq!(json["count"], 2, "{json}");
    for (entry, held) in json["processes"]
        .as_array()
        .unwrap()
        .iter()
        .zip([&first, &second])
    {
        assert_eq!(entry["pid"], held.pid(), "{json}");
        assert_eq!(entry["outcome"], "succeeded", "{json}");
        assert!(
            entry["name"].is_string() && entry.get("error").is_none(),
            "{json}"
        );
    }
    assert!(first.ended_within(Duration::from_secs(5)), "{json}");
    assert!(second.ended_within(Duration::from_secs(5)), "{json}");
}

#[test]
fn free_mixes_separate_ports_and_a_comma_list_and_kills_a_holder_once() {
    let mut held = Owned::listener();
    let mut other = Owned::listener();
    let free = format!(":{}", unused_port());
    // the same port twice has one holder, so one process to kill; the port
    // nobody holds is warned about, not an error
    let list = format!("{},{free}", other.target());
    let (out, code) = run_text(&["free", &held.target(), &held.target(), &list, "--dry-run"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("No process listening on port"), "{out}");
    assert!(out.contains("Dry run: would kill 2 processes"), "{out}");
    assert!(held.alive() && other.alive(), "a dry run kills nothing");
}

#[test]
fn free_without_yes_and_json_asks_for_yes_and_lists_what_it_would_kill() {
    let mut held = Owned::listener();
    let (json, code) = run_json(&["free", &held.target(), "--json"]);
    assert_eq!(code, 4, "{json}");
    assert_eq!(json["success"], false);
    assert_eq!(json["needs_confirmation"], true, "{json}");
    assert!(lists(&json, "processes", held.pid()), "{json}");
    assert!(held.alive(), "nothing is killed without --yes");
}

#[test]
fn free_kills_the_holder_and_says_the_port_is_free() {
    let mut held = Owned::listener();
    let (out, code) = run_text(&["free", &held.target(), "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&format!("Freed port {}", held.port)), "{out}");
    // Windows refuses the polite request for a console program; the force kill
    // that follows is the plan, so it is not reported as a failure
    assert!(!out.contains("Failed to"), "{out}");
    assert!(held.ended_within(Duration::from_secs(5)), "{out}");
}

#[test]
fn free_json_reports_each_port() {
    let mut held = Owned::listener();
    let (out, code) = run_text(&["free", &held.target(), "--yes", "--json"]);
    let json: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["action"], "free");
    assert_eq!(json["success"], true, "{json}");
    assert_eq!(json["results"][0]["port"], held.port);
    assert_eq!(json["results"][0]["freed"], true, "{json}");
    // the holder it stopped, with its outcome, after the keys the receipt had
    assert_eq!(json["count"], 1, "{json}");
    assert_eq!(json["processes"][0]["pid"], held.pid(), "{json}");
    assert_eq!(json["processes"][0]["outcome"], "succeeded", "{json}");
    assert!(json["processes"][0].get("error").is_none(), "{json}");
    assert_eq!(
        contract::key_order(&out),
        [
            "action",
            "success",
            "results",
            "not_found",
            "processes",
            "count"
        ]
    );
    assert!(held.ended_within(Duration::from_secs(5)), "{json}");
}

/// SIGTERM is ignored, so the holder is only moved by the SIGKILL that follows
/// the wait for a graceful exit
#[cfg(unix)]
#[test]
fn free_force_kills_a_holder_that_ignores_sigterm() {
    let mut held = Owned::start(&["--ignore-term"]);
    let (out, code) = run_text(&["free", &held.target(), "--yes", "--wait", "1"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Freed port"), "{out}");
    assert!(held.ended_within(Duration::from_secs(5)), "{out}");
}

/// A copy started by the holder (forked on Unix, a new process on Windows) takes
/// the port the moment the holder is killed, so the port is in use again: `free`
/// reports it instead of claiming it free
#[test]
fn free_reports_a_port_that_stays_in_use() {
    let held = Owned::start(&["--respawn"]);
    let (out, code) = run_text(&["free", &held.target(), "--yes", "--wait", "1"]);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains(&format!("Port {} still in use", held.port)),
        "{out}"
    );
}

#[test]
fn free_json_reports_a_port_that_stays_in_use() {
    let held = Owned::start(&["--respawn"]);
    let (json, code) = run_json(&["free", &held.target(), "--yes", "--wait", "1", "--json"]);
    assert_eq!(code, 1, "{json}");
    assert_eq!(json["success"], false, "{json}");
    assert_eq!(json["results"][0]["port"], held.port);
    assert_eq!(json["results"][0]["freed"], false, "{json}");
    // the holder itself was stopped; it is the copy that took the port back
    assert_eq!(json["count"], 1, "{json}");
    assert_eq!(json["processes"][0]["pid"], held.pid(), "{json}");
    assert_eq!(json["processes"][0]["outcome"], "succeeded", "{json}");
}

#[test]
fn free_with_no_port_in_the_target_is_invalid() {
    let (json, code) = run_json(&["free", ",", "--json"]);
    assert_eq!(code, 4, "{json}");
    assert_eq!(json["reason"], "invalid_input", "{json}");
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("a target can't be empty"), "{error}");
}
