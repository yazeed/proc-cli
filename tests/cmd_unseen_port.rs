//! A port held by another user's process, which proc, when it isn't root,
//! can't trace to a PID. Every command that takes `:port` says the port is in
//! use and that proc can't see its holder (exit 3, `permission_denied`), never
//! that nobody holds it. Linux and macOS, whose socket tables (/proc/net,
//! netstat) list every listening socket, whoever owns it. The holder runs as
//! root through `sudo -n`; where that can't be done (no password-less sudo,
//! or the tests run as root, who sees every process) the tests skip, which CI
//! turns into a failure.
#![cfg(any(target_os = "linux", target_os = "macos"))]

mod contract;
mod skip;

use assert_cmd::Command;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::process::{Child, Stdio};

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

/// A listener on 127.0.0.1 run as root, and its port. It exits when its
/// stdin closes, on drop: a test that isn't root can't signal it.
struct RootListener {
    child: Child,
    port: u16,
}

const LISTENER: &str = "import socket, sys
s = socket.socket()
s.bind(('127.0.0.1', 0))
s.listen()
print(s.getsockname()[1], flush=True)
sys.stdin.read()";

impl RootListener {
    fn start() -> Option<Self> {
        if unsafe { libc::geteuid() } == 0 {
            skip::skipped("the tests run as root, and root sees every process's sockets");
            return None;
        }
        let sudo = std::process::Command::new("sudo")
            .args(["-n", "true"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !sudo.is_ok_and(|status| status.success()) {
            skip::skipped("no password-less sudo to run a listener as root");
            return None;
        }
        let mut child = std::process::Command::new("sudo")
            .args(["-n", "python3", "-c", LISTENER])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("sudo python3 starts");
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let port = line
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("the listener printed no port: {line:?}"));
        Some(RootListener { child, port })
    }
}

impl Drop for RootListener {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.wait();
    }
}

#[test]
fn a_port_root_holds_is_in_use_not_nobodys() {
    let Some(root) = RootListener::start() else {
        return;
    };
    let port = format!(":{}", root.port);
    // a port nobody holds
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let nobody = format!(":{}", free.local_addr().unwrap().port());
    drop(free);
    let says_so = |json: &Value| {
        json["error"]
            .as_str()
            .is_some_and(|e| e.contains("is in use, but proc can't see which process holds it"))
    };

    // the lookups and every command that acts on a port: the commands that
    // change processes are previews, and could not touch root's process anyway
    for args in [
        vec!["on", port.as_str()],
        // with a target that matched nothing: in use is the truer answer
        vec!["on", port.as_str(), nobody.as_str()],
        vec!["info", port.as_str()],
        vec!["why", port.as_str()],
        vec!["kill", port.as_str(), "--dry-run"],
        vec!["stop", port.as_str(), "--dry-run"],
        vec!["freeze", port.as_str(), "--dry-run"],
        vec!["thaw", port.as_str(), "--dry-run"],
        vec!["free", port.as_str(), "--dry-run"],
        vec!["wait", port.as_str(), "--timeout", "1"],
    ] {
        let mut argv = args.clone();
        argv.push("--json");
        let (json, code) = run_json(&argv);
        assert_eq!(code, 3, "{argv:?}: {json}");
        assert_eq!(json["reason"], "permission_denied", "{argv:?}: {json}");
        assert!(says_so(&json), "{argv:?}: {json}");
    }

    // a port nobody holds is still not found
    let (json, code) = run_json(&["on", &nobody, "--json"]);
    assert_eq!(code, 2, "{json}");
    assert_eq!(json["reason"], "not_found", "{json}");

    // the warning says it in words, and the human error too
    let output = proc_cmd()
        .args(["kill", &port, "--dry-run"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(3), "{stderr}");
    assert!(stderr.contains("is in use, but proc can't see"), "{stderr}");
    assert!(!stderr.contains("Not found"), "{stderr}");
}
