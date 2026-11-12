//! `on` and `ports`, driven through the real binary.
//!
//! Every port and process these tests look for is one the test started: a copy
//! of the listener example (`examples/listener.rs`) under a name and path of its
//! own, in a fresh temp directory, so `--by <name>` and `--in <dir>` match only
//! it. A port nothing holds is one the OS just handed out and took back. Each
//! process is killed and reaped on drop.

mod built;
mod contract;

use assert_cmd::Command;
use proc_cli::core::filters::strip_verbatim_prefix;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

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

/// `target/debug/examples/listener`
fn listener_path() -> PathBuf {
    built::example("listener")
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

/// A process name no process has, for this call alone: the other tests run
/// proc at the same time, so a name two tests share would match the other
/// test's process.
fn nobody() -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    // Ends in `x`, so no name is the start of another (`-1x` in `-10x`)
    format!("nobody-{}-{n}x", std::process::id())
}

/// A port nothing listens on, from below every OS's ephemeral range (Linux
/// starts at 32768, macOS and Windows at 49152), so a listener another test
/// binds to port 0 can never be handed it. A port the OS just handed out and
/// took back could be: on CI a parallel test's listener got one between the
/// lookup and proc's run. A bind alone doesn't show the port is free: on macOS
/// binding 0.0.0.0 succeeds while another process listens on 127.0.0.1 there
/// (std sets SO_REUSEADDR; measured), so nothing may answer on loopback either
fn unused_port() -> u16 {
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
        TcpStream::connect_timeout(
            &SocketAddr::new(host, port),
            std::time::Duration::from_millis(500),
        )
        .is_ok()
    })
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Run the copy, with its cwd in `dir`. On Linux a file another thread has
/// just written can be "busy" (ETXTBSY) for an instant: a fork in a parallel
/// test still holds the write handle until it execs. That clears in
/// milliseconds, so try again.
fn spawn_copy(copy: &std::path::Path, args: &[&str], dir: &std::path::Path) -> Child {
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

/// A listener the test owns: a copy of the listener example, run with its cwd
/// in a fresh temp directory. Killed and reaped on drop.
struct Owned {
    child: Child,
    port: u16,
    /// A name no other process has, short enough for any OS to report whole
    name: String,
    /// Keeps the directory alive; `dir` is its path as processes report it
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    /// The listener's stdout, kept open while it runs: a listener started with
    /// `--also` prints after `PORT`, and a closed pipe would end it
    _stdout: BufReader<std::process::ChildStdout>,
}

impl Owned {
    fn listener() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        Self::named(&format!("pcp{}n{n}x", std::process::id()))
    }

    /// A listener whose process name is exactly `name` (15 characters at most:
    /// Linux cuts a process name there)
    fn named(name: &str) -> Self {
        Self::named_with_args(name, &[])
    }

    /// [`Owned::named`], started with extra arguments (the listener ignores them,
    /// so they only put words in its command line)
    fn named_with_args(name: &str, args: &[&str]) -> Self {
        assert!(name.len() <= 15, "{name} is too long for Linux");
        let (tmp, dir) = fresh_dir();
        let copy = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        std::fs::copy(listener_path(), &copy).expect("copy the listener example");
        let mut child = spawn_copy(&copy, args, &dir);
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
                name: name.to_string(),
                _tmp: tmp,
                dir,
                _stdout: stdout,
            },
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("the listener printed {line:?} instead of `PORT <n>`");
            }
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// The name as proc shows it: Windows keeps the `.exe`
    fn shown(&self) -> String {
        format!("{}{}", self.name, std::env::consts::EXE_SUFFIX)
    }

    fn dir(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }

    /// `:<port>`, as a target
    fn target(&self) -> String {
        format!(":{}", self.port)
    }

    /// The PID as a target
    fn pid_arg(&self) -> String {
        self.pid().to_string()
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `n` connections to the listener at `port`, held until the caller drops them.
/// The listener never accepts; the connections still count, and their client
/// ends belong to this test process.
fn connect(port: u16, n: usize) -> Vec<std::net::TcpStream> {
    (0..n)
        .map(|_| {
            std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect to the listener")
        })
        .collect()
}

/// How many `results` of an `on` JSON answer are for this PID
fn results_for(json: &Value, pid: u32) -> usize {
    json["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["process"]["pid"].as_u64() == Some(u64::from(pid)))
        .count()
}

/// A directory no test process runs in
fn other_dir() -> (tempfile::TempDir, String) {
    let (tmp, dir) = fresh_dir();
    let arg = dir.to_string_lossy().into_owned();
    (tmp, arg)
}

// ---- on: port to process ----

#[test]
fn on_port_names_the_process_holding_it() {
    let p = Owned::listener();
    let (out, _, code) = run_text(&["on", &p.target()]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.starts_with(&format!(
            "✓ Port {} is used by 1 process, reachable from this machine only\n\n",
            p.port
        )),
        "{out}"
    );
    assert!(out.contains(&format!("(PID {})", p.pid())), "{out}");
    for label in [
        "Process:",
        "Command:",
        "Directory:",
        "Path:",
        "Resources:",
        "Started:",
    ] {
        assert!(out.contains(label), "{label} missing: {out}");
    }
    assert!(
        out.contains(&format!(
            "Listening:   :{} TCP on 127.0.0.1 (this machine only)",
            p.port
        )),
        "{out}"
    );
    assert!(!out.contains("Uptime:"), "the card says Started: {out}");
}

/// The card is the one view: command, user, parent and the connections to the
/// port, as a client of it would see them
#[test]
#[cfg_attr(
    any(target_os = "freebsd", target_os = "netbsd"),
    ignore = "the BSDs read no connections"
)]
fn on_a_port_shows_the_card_with_its_connections() {
    let p = Owned::listener();
    let _clients = connect(p.port, 2);
    let (out, _, code) = run_text(&["on", &p.target()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Command:"), "{out}");
    assert!(out.contains("User:"), "{out}");
    assert!(out.contains("Parent:"), "{out}");
    assert!(out.contains("Connections: 2 established"), "{out}");
    // each peer is this test process, which holds the client ends
    let peers = out.lines().filter(|l| l.contains(" → ")).count();
    assert_eq!(peers, 2, "{out}");
    assert!(
        out.contains(&format!("(PID {})", std::process::id())),
        "{out}"
    );
}

/// Five peers by default and the count of the rest; `-v` lists every one
#[test]
#[cfg_attr(
    any(target_os = "freebsd", target_os = "netbsd"),
    ignore = "the BSDs read no connections"
)]
fn on_a_port_lists_five_peers_and_verbose_lists_all() {
    let p = Owned::listener();
    let _clients = connect(p.port, 7);
    let peer_lines = |out: &str| out.lines().filter(|l| l.contains(" → ")).count();

    let (out, _, code) = run_text(&["on", &p.target()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Connections: 7 established"), "{out}");
    assert_eq!(peer_lines(&out), 5, "{out}");
    assert!(out.contains("… and 2 more (-v for all)"), "{out}");

    let (out, _, code) = run_text(&["on", &p.target(), "-v"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(peer_lines(&out), 7, "{out}");
    assert!(!out.contains("more"), "{out}");
}

/// Values that don't fit the terminal are cut with `…`; `-v` prints them whole
#[test]
fn on_port_verbose_prints_the_whole_command_line() {
    let long = "x".repeat(300);
    let p = Owned::named_with_args(&format!("pcp{}lx", std::process::id()), &[&long]);
    let (out, _, code) = run_text(&["on", &p.target()]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains(&long), "cut to the width: {out}");
    assert!(out.contains('…'), "{out}");

    let (out, _, code) = run_text(&["on", &p.target(), "--verbose"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&long), "{out}");
    assert!(out.contains(&p.name), "{out}");
}

#[test]
fn on_port_json_describes_the_listener_and_its_process() {
    let p = Owned::listener();
    let (json, code) = run_json(&["on", &p.target(), "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["action"], "on");
    assert_eq!(json["query_type"], "port_to_process");
    assert_eq!(json["success"], true);
    assert_eq!(json["port"], p.port);
    assert_eq!(json["protocol"], "tcp");
    assert_eq!(json["address"], "127.0.0.1");
    assert_eq!(json["exposure"], "local");
    assert_eq!(json["process"]["pid"], p.pid());
    assert!(json.get("ports").is_none(), "{json}");
}

/// The port's connections are in the JSON in full, next to `address`
#[test]
#[cfg_attr(
    any(target_os = "freebsd", target_os = "netbsd"),
    ignore = "the BSDs read no connections"
)]
fn on_port_json_carries_the_connections() {
    let p = Owned::listener();
    let _clients = connect(p.port, 2);
    let (json, code) = run_json(&["on", &p.target(), "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["exposure"], "local");
    assert_eq!(json["connections"]["total"], 2, "{json}");
    assert_eq!(json["connections"]["by_state"]["established"], 2, "{json}");
    let peers = json["connections"]["peers"].as_array().unwrap();
    assert_eq!(peers.len(), 2, "{json}");
    assert_eq!(peers[0]["holder_pid"], p.pid(), "{json}");
    assert_eq!(peers[0]["peer"]["pid"], std::process::id(), "{json}");
    assert!(json["connections"].get("peers_truncated").is_none());
}

/// The JSON lists 50 peers at most and says it cut the list
#[test]
#[cfg_attr(
    any(target_os = "freebsd", target_os = "netbsd"),
    ignore = "the BSDs read no connections"
)]
fn on_port_json_caps_the_peers_at_fifty() {
    let p = Owned::listener();
    let _clients = connect(p.port, 60);
    let (json, code) = run_json(&["on", &p.target(), "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["connections"]["total"], 60, "{json}");
    assert_eq!(
        json["connections"]["peers"].as_array().unwrap().len(),
        50,
        "{json}"
    );
    assert_eq!(json["connections"]["peers_truncated"], true, "{json}");
}

#[test]
fn on_port_nobody_holds_exits_not_found() {
    let port = format!(":{}", unused_port());
    let (out, err, code) = run_text(&["on", &port]);
    assert_eq!(code, 2, "{out}{err}");
    assert!(err.contains("No process listening on port"), "{err}");

    let (json, code) = run_json(&["on", &port, "--json"]);
    assert_eq!(code, 2, "{json}");
    assert_eq!(json["success"], false);
}

#[test]
fn on_port_keeps_the_process_only_when_it_passes_in_and_by() {
    let p = Owned::listener();
    let (_other, other) = other_dir();

    let (out, _, code) = run_text(&["on", &p.target(), "--in", &p.dir()]);
    assert_eq!(code, 0, "{out}");
    let (out, _, code) = run_text(&["on", &p.target(), "--by", &p.name]);
    assert_eq!(code, 0, "{out}");

    let (out, err, code) = run_text(&["on", &p.target(), "--in", &other]);
    assert_eq!(code, 2, "{out}{err}");
    assert_eq!(
        err.lines()
            .next()
            .unwrap_or_default()
            .trim_start_matches("Error: "),
        format!(
            "No process found matching '{}' (ruled out by --in {other})",
            p.target()
        ),
        "the target sits in the quotes, the guard that ruled it out follows: {err}"
    );
    let (out, err, code) = run_text(&["on", &p.target(), "--by", "no-such-process-name"]);
    assert_eq!(code, 2, "{out}{err}");
    // the guard that failed, not the directory (it named the directory for both)
    assert!(
        err.contains("(ruled out by --by no-such-process-name)"),
        "{err}"
    );
}

/// `list` and `by` take process names, never targets: a colon is part of the
/// name, so `:70000` is not an impossible port there but a name nothing has, and
/// `:<port>` of a live listener does not find the process holding that port.
#[test]
fn list_and_by_read_a_colon_word_as_a_name_not_a_port() {
    let p = Owned::listener();
    for word in [":70000".to_string(), p.target()] {
        let (json, code) = run_json(&["list", &word, "--json"]);
        assert_eq!(code, 0, "list {word}: {json}");
        assert_eq!(json["count"], 0, "list {word}: {json}");

        let (out, err, code) = run_text(&["by", &word]);
        assert_eq!(code, 2, "by {word}: {out}{err}");
        assert!(
            err.contains(&format!("No process found matching '{word}'")),
            "by {word}: {err}"
        );
    }
}

// ---- on: process to ports ----

#[test]
fn on_pid_lists_the_ports_it_listens_on() {
    let p = Owned::listener();
    let (out, _, code) = run_text(&["on", &p.pid_arg()]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.starts_with(&format!("✓ {} (PID {})\n\n", p.shown(), p.pid())),
        "{out}"
    );
    assert!(
        !out.contains("is listening on:"),
        "the headline is the process: {out}"
    );
    assert!(
        out.contains(&format!(
            "Listening:   :{} TCP on 127.0.0.1 (this machine only)",
            p.port
        )),
        "{out}"
    );
    for label in ["Process:", "Directory:", "Path:", "Started:"] {
        assert!(out.contains(label), "{label} missing: {out}");
    }
    assert!(!out.contains("Uptime:"), "the card says Started: {out}");
}

/// `on <pid>` is the card `on :port` prints
#[test]
#[cfg_attr(
    any(target_os = "freebsd", target_os = "netbsd"),
    ignore = "the BSDs read no connections"
)]
fn on_a_pid_shows_the_same_card() {
    let p = Owned::listener();
    let _clients = connect(p.port, 2);
    let (by_pid, _, code) = run_text(&["on", &p.pid_arg()]);
    assert_eq!(code, 0, "{by_pid}");
    for want in ["Command:", "User:", "Parent:", "Connections: 2 established"] {
        assert!(by_pid.contains(want), "{want} missing: {by_pid}");
    }
    // the card, from `Process:` on, is the one the port prints
    let (by_port, _, _) = run_text(&["on", &p.target()]);
    let card = |out: &str| {
        // `Started:` and CPU move between two runs
        let at = out.find("  Process:").unwrap();
        out[at..]
            .lines()
            .filter(|l| !l.contains("Started:") && !l.contains("Resources:"))
            .map(|l| format!("{l}\n"))
            .collect::<String>()
    };
    assert_eq!(card(&by_pid), card(&by_port));
}

#[test]
fn on_pid_verbose_prints_the_whole_command_line() {
    let long = "y".repeat(300);
    let p = Owned::named_with_args(&format!("pcp{}vx", std::process::id()), &[&long]);
    let (out, _, code) = run_text(&["on", &p.pid_arg()]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains(&long), "cut to the width: {out}");
    let (out, _, code) = run_text(&["on", &p.pid_arg(), "-v"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&long), "{out}");
    assert!(out.contains("Path:"), "{out}");
}

#[test]
fn on_pid_json_lists_its_ports() {
    let p = Owned::listener();
    let (json, code) = run_json(&["on", &p.pid_arg(), "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["query_type"], "process_to_ports");
    assert_eq!(json["process"]["pid"], p.pid());
    let ports = json["ports"].as_array().unwrap();
    assert!(ports.iter().any(|x| x["port"] == p.port), "{json}");
    assert!(ports.iter().all(|x| x["exposure"] == "local"), "{json}");
    assert!(json.get("port").is_none(), "{json}");
}

/// A port of `on <pid>` and of a name carries the counts, never the peers
#[test]
#[cfg_attr(
    any(target_os = "freebsd", target_os = "netbsd"),
    ignore = "the BSDs read no connections"
)]
fn on_pid_and_name_json_carry_connection_counts_only() {
    let p = Owned::listener();
    let _clients = connect(p.port, 2);
    let (json, code) = run_json(&["on", &p.pid_arg(), "--json"]);
    assert_eq!(code, 0, "{json}");
    let port = &json["ports"][0];
    assert_eq!(port["port"], p.port, "{json}");
    assert_eq!(port["connections"]["total"], 2, "{json}");
    assert_eq!(port["connections"]["by_state"]["established"], 2, "{json}");
    assert!(port["connections"].get("peers").is_none(), "{json}");

    let (json, code) = run_json(&["on", &p.name, "--json"]);
    assert_eq!(code, 0, "{json}");
    let port = &json["results"][0]["ports"][0];
    assert_eq!(port["connections"]["total"], 2, "{json}");
    assert!(port["connections"].get("peers").is_none(), "{json}");

    // several targets: the same port object
    let (json, code) = run_json(&["on", &p.pid_arg(), &p.target(), "--json"]);
    assert_eq!(code, 0, "{json}");
    for result in json["results"].as_array().unwrap() {
        assert_eq!(result["ports"][0]["connections"]["total"], 2, "{json}");
        assert!(result["ports"][0]["connections"].get("peers").is_none());
    }
}

#[test]
fn on_a_process_listening_on_nothing_says_none() {
    let p = Owned::named_with_args(&format!("pcp{}nx", std::process::id()), &["--no-port"]);
    let (out, _, code) = run_text(&["on", &p.pid_arg()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&format!("(PID {})", p.pid())), "{out}");
    assert!(out.contains("Listening:   none"), "{out}");
    assert!(!out.contains("is listening on:"), "{out}");
    assert!(!out.contains("No listening ports"), "{out}");
    assert!(
        !out.contains("Connections:"),
        "nothing to connect to: {out}"
    );

    let (json, code) = run_json(&["on", &p.pid_arg(), "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["ports"].as_array().unwrap().len(), 0, "{json}");
}

#[test]
fn on_pid_outside_in_or_by_exits_not_found() {
    let p = Owned::listener();
    let (_other, other) = other_dir();
    let (out, err, code) = run_text(&["on", &p.pid_arg(), "--in", &other]);
    assert_eq!(code, 2, "{out}{err}");
    assert_eq!(
        err.lines()
            .next()
            .unwrap_or_default()
            .trim_start_matches("Error: "),
        format!(
            "No process found matching '{}' (ruled out by --in {other})",
            p.pid_arg()
        ),
        "the target sits in the quotes, the guard that ruled it out follows: {err}"
    );

    let (out, err, code) = run_text(&["on", &p.pid_arg(), "--by", "no-such-process-name"]);
    assert_eq!(code, 2, "{out}{err}");
    assert!(
        err.contains("(ruled out by --by no-such-process-name)"),
        "{err}"
    );

    let (out, _, code) = run_text(&["on", &p.pid_arg(), "--in", &p.dir(), "--by", &p.name]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn on_a_pid_nothing_runs_exits_not_found() {
    // A PID past anything a system hands out
    let (out, err, code) = run_text(&["on", "4294967"]);
    assert_eq!(code, 2, "{out}{err}");
}

// ---- on: process name ----

#[test]
fn on_name_lists_each_matching_process() {
    let p = Owned::listener();
    let (out, _, code) = run_text(&["on", &p.name]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.starts_with(&format!("✓ 1 process named '{}'\n\n", p.name)),
        "{out}"
    );
    assert!(out.contains(&format!("(PID {})", p.pid())), "{out}");
    assert!(
        out.contains(&format!("Listening:   :{} TCP", p.port)),
        "{out}"
    );

    let (json, code) = run_json(&["on", &p.name, "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["count"], 1, "{json}");
    assert_eq!(json["results"][0]["process"]["pid"], p.pid());
    assert_eq!(json["results"][0]["ports"][0]["port"], p.port);
}

#[test]
fn on_name_applies_in_and_by() {
    let p = Owned::listener();
    let (_other, other) = other_dir();

    let (json, code) = run_json(&["on", &p.name, "--in", &p.dir(), "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["count"], 1, "{json}");

    let (out, err, code) = run_text(&["on", &p.name, "--in", &other]);
    assert_eq!(code, 2, "{out}{err}");
    assert_eq!(
        err.lines()
            .next()
            .unwrap_or_default()
            .trim_start_matches("Error: "),
        format!(
            "No process found matching '{}' (ruled out by --in {other})",
            p.name
        ),
        "the target sits in the quotes, the guard that ruled it out follows: {err}"
    );

    let (out, err, code) = run_text(&["on", &p.name, "--by", "no-such-process-name"]);
    assert_eq!(code, 2, "{out}{err}");
    assert!(
        err.contains("(ruled out by --by no-such-process-name)"),
        "{err}"
    );
}

#[test]
fn on_name_is_the_whole_name_and_cmd_widens_it_to_the_command_line() {
    let word = nobody();
    let p = Owned::named_with_args(&format!("pcp{}on", std::process::id()), &[&word]);

    // the word is only in the command line: not a name, and the error says so
    let (out, err, code) = run_text(&["on", &word]);
    assert_eq!(code, 2, "{out}{err}");
    assert!(err.contains("No process found matching"), "{err}");
    assert!(
        err.contains(&format!(
            "1 process has '{word}' in its command line (add --cmd to match those)"
        )),
        "{err}"
    );

    let (json, code) = run_json(&["on", &word, "--cmd", "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["count"], 1, "{json}");
    assert_eq!(json["results"][0]["process"]["pid"], p.pid());

    // the headline says what matched: the command line, not a name
    let (out, _, code) = run_text(&["on", &word, "--cmd"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.starts_with(&format!("✓ 1 process matching '{word}'\n\n")),
        "{out}"
    );

    // part of the name is not the name either
    let part = &p.name[..p.name.len() - 1];
    let (out, err, code) = run_text(&["on", part]);
    assert_eq!(code, 2, "{out}{err}");
    let (json, code) = run_json(&["on", part, "--cmd", "--json"]);
    assert_eq!(code, 0, "{json}");
}

#[test]
fn on_a_name_nothing_runs_exits_not_found() {
    let (out, err, code) = run_text(&["on", &nobody()]);
    assert_eq!(code, 2, "{out}{err}");
    assert!(err.contains("No process found matching"), "{err}");
}

// ---- on: several targets ----

#[test]
fn on_several_targets_shows_each_and_warns_about_the_missing_one() {
    let a = Owned::listener();
    let b = Owned::listener();
    let missing = unused_port();
    let targets = format!("{},{},:{missing}", a.target(), b.pid_arg());
    let (out, _, code) = run_text(&["on", &targets]);
    assert_eq!(code, 0, "one match is enough: {out}");
    assert!(
        out.contains(&format!("✓ Port {} is used by 1 process", a.port)),
        "{out}"
    );
    assert!(
        out.contains(&format!("✓ {} (PID {})", b.shown(), b.pid())),
        "{out}"
    );
    assert!(out.contains(&format!("Port {missing}:")), "{out}");
    assert!(out.contains("No process listening on port"), "{out}");
}

#[test]
fn on_several_targets_warns_about_a_missing_pid_and_name() {
    let a = Owned::listener();
    let nobody = nobody();
    let targets = format!("{},4294967,{nobody}", a.name);
    let (out, _, code) = run_text(&["on", &targets]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("PID 4294967:"), "{out}");
    assert!(out.contains(&format!("'{nobody}':")), "{out}");
}

#[test]
fn on_several_targets_all_missing_exits_not_found() {
    let nobody = nobody();
    let targets = format!(":{},4294967,{nobody}", unused_port());
    let (out, err, code) = run_text(&["on", &targets]);
    assert_eq!(code, 2, "{out}{err}");
    // Each target printed its own warning, so the final error adds no message
    assert!(out.contains("No process listening on port"), "{out}");
    assert!(out.contains("PID 4294967:"), "{out}");
    assert!(out.contains(&format!("'{nobody}':")), "{out}");
    // the comma list is the one thing stderr says
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(err.starts_with("note: comma-separated targets are deprecated"));
}

#[test]
fn on_several_targets_json_has_one_result_per_process() {
    let a = Owned::listener();
    let b = Owned::listener();
    let missing = format!(":{}", unused_port());
    let targets = format!("{},{},{},{missing}", a.target(), b.pid_arg(), a.name);
    let (json, code) = run_json(&["on", &targets, "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["action"], "on");
    assert_eq!(json["success"], true);
    assert_eq!(json["count"], 3, "{json}");
    assert_eq!(json["not_found"], serde_json::json!([missing]), "{json}");
    let results = json["results"].as_array().unwrap();
    assert_eq!(results[0]["target"], a.target());
    assert_eq!(results[0]["process"]["pid"], a.pid());
    assert_eq!(results[0]["ports"][0]["port"], a.port);
    assert_eq!(results[1]["target"], b.pid_arg());
    assert_eq!(results[1]["process"]["pid"], b.pid());
    assert_eq!(results[2]["target"], a.name);
    assert_eq!(results_for(&json, a.pid()), 2, "{json}");
}

#[test]
fn on_several_targets_json_with_nothing_found_exits_not_found() {
    let targets = format!(":{},4294967", unused_port());
    let (json, code) = run_json(&["on", &targets, "--json"]);
    assert_eq!(code, 2, "{json}");
    assert_eq!(json["success"], false);
}

#[test]
fn on_several_targets_json_counts_a_filtered_out_process_as_not_found() {
    // `a` runs in the directory, `b` somewhere else
    let a = Owned::listener();
    let b = Owned::listener();
    let targets = format!("{},{}", a.target(), b.pid_arg());
    let (json, code) = run_json(&["on", &targets, "--in", &a.dir(), "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["count"], 1, "{json}");
    assert_eq!(json["results"][0]["process"]["pid"], a.pid());
    assert_eq!(
        json["not_found"],
        serde_json::json!([b.pid_arg()]),
        "{json}"
    );

    // Everything filtered out is an error, not an empty answer
    let (json, code) = run_json(&["on", &b.pid_arg(), "--in", &a.dir(), "--json"]);
    assert_eq!(code, 2, "{json}");
}

#[test]
fn on_takes_several_targets_as_separate_arguments_and_commas_still_work() {
    let a = Owned::listener();
    let b = Owned::listener();
    let missing = format!(":{}", unused_port());
    // (target, PID) of every result: one per matching process, in the order typed
    let found = |json: &Value| -> Vec<(String, u64)> {
        json["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["target"].as_str().unwrap().to_string(),
                    r["process"]["pid"].as_u64().unwrap(),
                )
            })
            .collect()
    };
    let want = vec![
        (a.target(), u64::from(a.pid())),
        (b.pid_arg(), u64::from(b.pid())),
        (a.name.clone(), u64::from(a.pid())),
    ];

    let (separate, code) =
        run_json(&["on", &a.target(), &b.pid_arg(), &a.name, &missing, "--json"]);
    assert_eq!(code, 0, "{separate}");
    assert_eq!(found(&separate), want, "{separate}");
    assert_eq!(separate["count"], 3, "{separate}");
    assert_eq!(separate["not_found"], serde_json::json!([missing]));

    // the old form, and the two mixed, give the same answer
    let comma = format!("{},{},{},{missing}", a.target(), b.pid_arg(), a.name);
    let (joined, _) = run_json(&["on", &comma, "--json"]);
    assert_eq!(found(&joined), want, "{joined}");
    assert_eq!(joined["not_found"], separate["not_found"]);
    let tail = format!("{},{missing}", b.pid_arg());
    let (mixed, _) = run_json(&["on", &a.target(), &tail, &a.name, "--json"]);
    assert_eq!(found(&mixed), want, "{mixed}");
    assert_eq!(mixed["not_found"], separate["not_found"]);

    // human output: each target is shown, a missing one is warned about
    let (out, _, code) = run_text(&["on", &a.target(), &b.pid_arg(), &missing]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(&format!("✓ Port {} is used by 1 process", a.port)),
        "{out}"
    );
    assert!(
        out.contains(&format!("✓ {} (PID {})", b.shown(), b.pid())),
        "{out}"
    );
    assert!(out.contains("No process listening on port"), "{out}");
}

#[test]
fn on_with_nothing_found_echoes_the_targets_as_typed() {
    // two PIDs nothing runs: the error names both, as typed
    let (json, code) = run_json(&["on", "4294967", "4294968", "--json"]);
    assert_eq!(code, 2, "{json}");
    let error = json["error"].as_str().unwrap();
    assert!(
        error.contains("No process found matching '4294967 4294968'"),
        "{error}"
    );
    // a target that splits into nothing is refused while parsing
    let (json, code) = run_json(&["on", ",", ",,", "--json"]);
    assert_eq!(code, 4, "{json}");
    assert_eq!(json["reason"], "invalid_input", "{json}");
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("a target can't be empty"), "{error}");
    let (out, err, code) = run_text(&["on", ",", ",,"]);
    assert_eq!(code, 4, "{out}{err}");
}

// ---- ports ----

/// The `ports` rows (JSON) for the listeners whose process name is `name`
/// (`--cmd` in `extra` makes it any part of the name)
fn ports_by(name: &str, extra: &[&str]) -> Value {
    let mut args = vec!["ports", "--by", name, "--json"];
    args.extend_from_slice(extra);
    let (json, code) = run_json(&args);
    assert_eq!(code, 0, "{json}");
    json
}

#[test]
fn ports_lists_a_listener_as_a_table() {
    let p = Owned::listener();
    let (out, _, code) = run_text(&["ports", "--by", &p.name]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Found 1 listening port\n"), "{out}");
    let row = out
        .lines()
        .find(|l| l.contains(&p.name))
        .unwrap_or_else(|| panic!("no row for {}: {out}", p.name));
    assert!(row.contains(&p.port.to_string()), "{row}");
    assert!(row.contains("TCP"), "{row}");
    assert!(row.contains(&p.pid().to_string()), "{row}");
    assert!(row.contains("127.0.0.1"), "{row}");
    assert!(!out.contains('↳'), "no path line any more: {out}");
}

/// The table's cells for the one row of `ports --by <name>`, and its header
fn ports_table_of(p: &Owned, extra: &[&str]) -> (Vec<String>, Vec<String>, String) {
    let mut args = vec!["ports", "--by", &p.name];
    args.extend_from_slice(extra);
    let (out, _, code) = run_text(&args);
    assert_eq!(code, 0, "{out}");
    let header = out
        .lines()
        .find(|l| l.contains("EXPOSURE"))
        .unwrap_or_else(|| panic!("no header: {out}"));
    let row = out
        .lines()
        .find(|l| l.contains(&p.port.to_string()) && l.contains("TCP"))
        .unwrap_or_else(|| panic!("no row for {}: {out}", p.name));
    let words = |line: &str| line.split_whitespace().map(String::from).collect();
    (words(header), words(row), out)
}

#[test]
fn ports_shows_name_uptime_command_and_exposure() {
    let p = Owned::named_with_args(&format!("pcp{}ux", std::process::id()), &["--marker"]);
    let (header, row, out) = ports_table_of(&p, &[]);
    assert_eq!(
        header,
        ["PORT", "PROTO", "ADDRESS", "EXPOSURE", "PID", "NAME", "UPTIME", "COMMAND"],
        "{out}"
    );
    assert_eq!(
        row[..6],
        [
            p.port.to_string(),
            "TCP".to_string(),
            "127.0.0.1".to_string(),
            "local".to_string(),
            p.pid().to_string(),
            p.shown()
        ],
        "{out}"
    );
    // UPTIME is a duration, such as `0s` or `1m 3s`; COMMAND starts with the
    // executable's directory (the table cuts the rest to the terminal)
    assert!(row[6].starts_with(|c: char| c.is_ascii_digit()), "{row:?}");
    let dir_start: String = p.dir().chars().take(12).collect();
    assert!(
        row.iter().skip(7).any(|w| w.contains(&dir_start)),
        "{row:?}"
    );
}

#[test]
fn ports_verbose_adds_user_memory_and_path_and_cuts_nothing() {
    let p = Owned::named_with_args(&format!("pcp{}vb", std::process::id()), &["--marker"]);
    let (header, row, out) = ports_table_of(&p, &["--verbose"]);
    assert_eq!(
        header,
        [
            "PORT", "PROTO", "ADDRESS", "EXPOSURE", "PID", "NAME", "USER", "MEM", "UPTIME", "PATH",
            "COMMAND"
        ],
        "{out}"
    );
    assert!(!out.contains('…'), "-v cuts nothing: {out}");
    assert!(!out.contains('↳'), "{out}");
    // the whole command line is the last cell(s): the executable, then the argument
    assert_eq!(row.last().map(String::as_str), Some("--marker"), "{row:?}");
    let json = ports_by(&p.name, &["--verbose"]);
    let exe = json["ports"][0]["exe_path"].as_str().unwrap();
    assert!(row.iter().any(|w| w == exe), "{exe} is not in {row:?}");
    let user = json["ports"][0]["user"].as_str();
    assert_eq!(
        row.get(6).map(String::as_str),
        user.or(Some("-")),
        "{row:?}"
    );
}

#[test]
fn ports_json_carries_the_holders_facts() {
    let p = Owned::named_with_args(&format!("pcp{}js", std::process::id()), &["--marker"]);
    let (text, _, code) = run_text(&["ports", "--by", &p.name, "--json"]);
    assert_eq!(code, 0, "{text}");
    let json: Value = serde_json::from_str(&text).unwrap();
    let row = &json["ports"][0];
    let (on, _) = run_json(&["on", &p.pid_arg(), "--json"]);
    let process = &on["process"];

    // what it has always had, then the additions, after it and in this order
    let at = |key: &str| text.find(&format!("\"{key}\":")).unwrap_or(0);
    let keys = [
        "port",
        "protocol",
        "pid",
        "process_name",
        "address",
        "exposure",
        "command",
        "uptime_secs",
        "memory_mb",
    ];
    for pair in keys.windows(2) {
        assert!(
            at(pair[0]) < at(pair[1]),
            "{} before {}: {text}",
            pair[0],
            pair[1]
        );
    }
    assert_eq!(row["exposure"], "local");
    assert_eq!(row["command"], process["command"], "{row}");
    assert!(
        row["command"].as_str().unwrap().ends_with("--marker"),
        "{row}"
    );
    assert_eq!(row["user"], process["user"], "absent where on has none");
    assert!(row["uptime_secs"].as_u64().unwrap() < 600, "{row}");
    assert!(row["memory_mb"].as_f64().unwrap() >= 0.0, "{row}");
    // the connection counts and the path are for -v
    assert!(row.get("connections").is_none(), "{row}");
    assert!(row.get("exe_path").is_none(), "{row}");
}

#[test]
#[cfg_attr(
    any(target_os = "freebsd", target_os = "netbsd"),
    ignore = "the BSDs read no connections"
)]
fn ports_verbose_json_counts_the_connections() {
    let p = Owned::listener();
    let _clients = connect(p.port, 2);
    let json = ports_by(&p.name, &["--verbose"]);
    let row = &json["ports"][0];
    assert_eq!(row["connections"]["total"], 2, "{json}");
    assert_eq!(row["connections"]["by_state"]["established"], 2, "{json}");
    assert!(row["connections"].get("peers").is_none(), "{json}");
    assert!(
        row["exe_path"].as_str().unwrap().contains(&p.name),
        "{json}"
    );
}

#[test]
fn ports_pluralises_the_count() {
    // two listeners with one name, in directories of their own
    let name = format!("pcp{}pl", std::process::id());
    let a = Owned::named(&name);
    let b = Owned::named(&name);
    let (out, _, code) = run_text(&["ports", "--by", &name]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Found 2 listening ports\n"), "{out}");
    drop((a, b));
}

#[test]
fn ports_with_no_match_says_so() {
    let (out, _, code) = run_text(&["ports", "--by", "no-such-process-name-anywhere"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("No listening ports found"), "{out}");

    let (json, code) = run_json(&["ports", "--by", "no-such-process-name-anywhere", "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["action"], "ports");
    assert_eq!(json["count"], 0);
    assert_eq!(json["ports"].as_array().unwrap().len(), 0);
}

#[test]
fn ports_json_describes_each_listener() {
    let p = Owned::listener();
    let json = ports_by(&p.name, &[]);
    assert_eq!(json["success"], true);
    assert_eq!(json["count"], 1, "{json}");
    let row = &json["ports"][0];
    assert_eq!(row["port"], p.port);
    assert_eq!(row["protocol"], "tcp");
    assert_eq!(row["pid"], p.pid());
    assert_eq!(row["address"], "127.0.0.1");
    assert!(row.get("exe_path").is_none(), "{row}");
}

/// A process started through a symlink named `zzname-<pid of the test>`, so the
/// name the socket tables report (the name it was started as) and the name the
/// process list records can differ
#[cfg(unix)]
#[test]
fn a_port_row_and_its_process_have_one_name() {
    let name = format!("zzname-{}", std::process::id());
    let (_tmp, dir) = fresh_dir();
    let link = dir.join(&name);
    std::os::unix::fs::symlink(listener_path(), &link).unwrap();
    let mut child = spawn_copy(&link, &[], &dir);
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let port: u16 = line.trim().strip_prefix("PORT ").unwrap().parse().unwrap();
    let pid = child.id();

    let (ports, code) = run_json(&["ports", "--json"]);
    assert_eq!(code, 0, "{ports}");
    let row = ports["ports"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["pid"].as_u64() == Some(u64::from(pid)))
        .unwrap_or_else(|| panic!("no row for PID {pid} in {ports}"))
        .clone();
    let row_name = row["process_name"].as_str().unwrap().to_string();

    let (by_port, _) = run_json(&["on", &format!(":{port}"), "--json"]);
    let (by_pid, _) = run_json(&["on", &pid.to_string(), "--json"]);
    let port_name = by_port["process"]["name"].as_str().unwrap().to_string();
    let pid_name = by_pid["process"]["name"].as_str().unwrap().to_string();
    let (found, by_code) = run_json(&["by", &row_name, "--json"]);

    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(row_name, port_name, "ports row vs on :port");
    assert_eq!(row_name, pid_name, "ports row vs on <pid>");
    assert_eq!(by_code, 0, "by {row_name}: {found}");
    assert!(
        found["processes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["pid"].as_u64() == Some(u64::from(pid))),
        "by {row_name} did not find PID {pid}: {found}"
    );
}

#[test]
fn ports_by_matches_the_whole_name_in_any_case() {
    let p = Owned::listener();
    let shouted = p.name.to_uppercase();
    let json = ports_by(&shouted, &[]);
    assert_eq!(json["count"], 1, "{json}");
    assert_eq!(json["ports"][0]["pid"], p.pid());
}

#[test]
fn ports_by_does_not_match_part_of_a_name_unless_cmd_is_given() {
    let p = Owned::listener();
    // the name without its last character
    let part = &p.name[..p.name.len() - 1];
    assert_eq!(ports_by(part, &[])["count"], 0);
    let json = ports_by(part, &["--cmd"]);
    assert!(
        json["ports"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["pid"] == p.pid()),
        "{json}"
    );
}

#[test]
fn ports_by_ignores_a_word_that_is_only_in_the_command_line_unless_cmd_is_given() {
    let word = nobody();
    let p = Owned::named_with_args(&format!("pcp{}cl", std::process::id()), &[&word]);
    assert_eq!(ports_by(&word, &[])["count"], 0);
    let json = ports_by(&word, &["--cmd"]);
    assert_eq!(json["count"], 1, "{json}");
    assert_eq!(json["ports"][0]["pid"], p.pid());
}

#[test]
fn ports_in_keeps_only_processes_started_in_that_directory() {
    let p = Owned::listener();
    let (_other, other) = other_dir();
    let json = ports_by(&p.name, &["--in", &p.dir()]);
    assert_eq!(json["count"], 1, "{json}");
    let json = ports_by(&p.name, &["--in", &other]);
    assert_eq!(json["count"], 0, "{json}");
}

#[test]
fn ports_exposed_and_local_split_on_the_bind_address() {
    // The listener binds 127.0.0.1 only
    let p = Owned::listener();
    assert_eq!(ports_by(&p.name, &["--local"])["count"], 1);
    assert_eq!(ports_by(&p.name, &["--exposed"])["count"], 0);
}

#[test]
fn ports_range_keeps_ports_inside_it() {
    let p = Owned::listener();
    let inside = format!("{}-{}", p.port, p.port);
    assert_eq!(ports_by(&p.name, &["--range", &inside])["count"], 1);
    let below = format!("1-{}", p.port - 1);
    assert_eq!(ports_by(&p.name, &["--range", &below])["count"], 0);
    if p.port < u16::MAX {
        let above = format!("{}-65535", p.port + 1);
        assert_eq!(ports_by(&p.name, &["--range", &above])["count"], 0);
    }
}

#[test]
fn ports_sorts_by_port_pid_or_name() {
    let pid = std::process::id();
    // The first one started has the later name, so name order differs from start order
    let first = Owned::named(&format!("pcp{pid}so_z"));
    let second = Owned::named(&format!("pcp{pid}so_a"));
    // both names start with it, so this needs --cmd
    let filter = format!("pcp{pid}so_");

    let order = |sort: &str, key: &str| -> Vec<Value> {
        let json = ports_by(&filter, &["--cmd", "--sort", sort]);
        assert_eq!(json["count"], 2, "{json}");
        json["ports"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p[key].clone())
            .collect()
    };

    let mut by_port = [first.port, second.port];
    by_port.sort_unstable();
    assert_eq!(order("port", "port"), by_port.map(Value::from).to_vec());

    let mut by_pid = [first.pid(), second.pid()];
    by_pid.sort_unstable();
    assert_eq!(order("pid", "pid"), by_pid.map(Value::from).to_vec());

    // "so_a" before "so_z", whatever the PIDs and ports are
    assert_eq!(
        order("name", "pid"),
        vec![Value::from(second.pid()), Value::from(first.pid())]
    );
}

#[test]
fn ports_limit_cuts_the_list() {
    let pid = std::process::id();
    // two listeners with one name, in directories of their own
    let filter = format!("pcp{pid}lm");
    let _a = Owned::named(&filter);
    let _b = Owned::named(&filter);
    assert_eq!(ports_by(&filter, &[])["count"], 2);
    let json = ports_by(&filter, &["--limit", "1"]);
    assert_eq!(json["count"], 1, "{json}");
    assert_eq!(json["ports"].as_array().unwrap().len(), 1);
}

#[test]
fn ports_names_a_process_that_holds_several_ports() {
    // one PID on two rows of the socket table: the name lookup asked for it
    // twice, and sysinfo dropped a PID refreshed twice, so on Windows, where
    // netstat gives no name of its own, both rows read "unknown"
    let pid = std::process::id();
    let owned = Owned::named_with_args(&format!("pcp{pid}tw"), &["--also", "1"]);
    let json = ports_by(&owned.name, &[]);
    let rows = json["ports"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{json}");
    for row in rows {
        assert_eq!(row["pid"], owned.pid(), "{json}");
        assert_eq!(row["process_name"], owned.shown().as_str(), "{json}");
    }
}
