//! One set of JSON names: every command's `--json`, read together and checked
//! against the naming rules (tests/contract/mod.rs, `names_violation`,
//! `mixed_kinds`; the rules are in skills/proc-cli/reference.md, "Names
//! deprecated in 2.3.0"). It is a guard against new drift: a key that holds a
//! duration and does not end in `_secs`, a `_count` beside no list, a receipt
//! whose `processes` disagree with `succeeded` and `failed`, or a key that is a
//! list in one command and a number in another. The names 2.3.0 deprecated are
//! the only exceptions, listed in the contract module with the rule each breaks.
//!
//! The shapes are the ones a test can make with processes it starts: the
//! listener example (`examples/listener.rs`), run as it is. Every process the
//! commands act on (`kill`, `stop`, `freeze`, `free`) is one of those, named
//! by PID or by the port it holds; the rest only read. Each is killed and
//! reaped on drop.

mod contract;

use assert_cmd::Command;
use contract::Kinds;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Stdio};

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

/// A listener the test started: killed and reaped on drop
struct Listener {
    child: Child,
    port: u16,
}

impl Listener {
    fn start() -> Self {
        let mut child = std::process::Command::new(listener_path())
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
        let Some(port) = port else {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the listener printed {line:?} instead of `PORT <n>`");
        };
        Listener { child, port }
    }

    fn pid(&self) -> String {
        self.child.id().to_string()
    }

    fn port(&self) -> String {
        format!(":{}", self.port)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Every document read so far, as the rules see them
struct Reader {
    kinds: Kinds,
    /// the `action` and arguments of each document, for a failure message
    read: Vec<String>,
}

impl Reader {
    /// Run proc, parse its one JSON document, check the contract and the names
    /// rules on it (`contract::check` does both) and note its keys' kinds.
    /// Returns the exit code.
    fn json(&mut self, args: &[&str]) -> (Value, i32) {
        let output = proc_cmd().args(args).output().unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        let json: Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|e| panic!("proc {args:?}: not one JSON document ({e}):\n{stdout}"));
        let code = output.status.code().unwrap();
        contract::check(&json, code, args);
        contract::note_kinds(&json, &mut self.kinds);
        self.read.push(args.join(" "));
        (json, code)
    }

    /// [`Reader::json`] where the exit code must be `want`
    fn json_exiting(&mut self, args: &[&str], want: i32) -> Value {
        let (json, code) = self.json(args);
        assert_eq!(code, want, "proc {args:?}: {json}");
        json
    }
}

/// The first line `proc watch <pid> --json` prints, a frame; the child is
/// killed and reaped before this returns
fn watch_frame(pid: &str) -> Value {
    struct Reaped(Child);
    impl Drop for Reaped {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut watch = Reaped(
        std::process::Command::new(proc_cmd().get_program())
            .args(["watch", pid, "--json", "--interval", "1"])
            .stdout(Stdio::piped())
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn proc watch --json"),
    );
    let mut line = String::new();
    BufReader::new(watch.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    serde_json::from_str(&line).unwrap_or_else(|e| panic!("a watch frame ({e}): {line:?}"))
}

#[test]
fn every_command_keeps_the_naming_rules() {
    let mut reader = Reader {
        kinds: Kinds::new(),
        read: Vec::new(),
    };
    let held = Listener::start();
    let (pid, port) = (held.pid(), held.port());
    let exe = listener_path();
    let exe = exe.to_string_lossy().into_owned();

    // ---- discovery: lists of processes and ports
    reader.json_exiting(&["list", "--json"], 0);
    reader.json_exiting(&["info", &pid, "--json"], 0);
    reader.json_exiting(&["info", &pid, "99999999", "--json"], 0);
    reader.json_exiting(&["info", "99999999", "--json"], 2);
    reader.json_exiting(&["on", &port, "--json"], 0);
    reader.json_exiting(&["on", &pid, "--json"], 0);
    reader.json_exiting(&["on", &pid, &port, "99999999", "--json"], 0);
    reader.json_exiting(&["ports", "--json"], 0);
    reader.json_exiting(&["ports", "-v", "--json"], 0);
    reader.json_exiting(&["tree", &pid, "--json"], 0);
    reader.json_exiting(&["tree", &pid, "--ancestors", "--json"], 0);
    reader.json_exiting(&["why", &pid, &port, "--json"], 0);
    reader.json_exiting(&["for", &exe, "--json"], 0);
    reader.json_exiting(&["by", "99999999-no-such-name", "--json"], 2);
    reader.json_exiting(&["in", ".", "--json"], 0);
    let frame = watch_frame(&pid);
    contract::check(&frame, 0, &["watch", &pid, "--json"]);
    contract::note_kinds(&frame, &mut reader.kinds);

    // ---- stuck and orphans: a list or an empty result, whichever the machine has
    for args in [["stuck", "--json"], ["orphans", "--json"]] {
        let (_, code) = reader.json(&args);
        assert!(code == 0 || code == 4, "{args:?} exited {code}");
    }

    // ---- previews and the refusal to act without --yes
    for action in ["kill", "stop", "freeze", "thaw", "free"] {
        let target = if action == "free" { &port } else { &pid };
        let supported = cfg!(unix) || !matches!(action, "freeze" | "thaw");
        let want = if supported { 0 } else { 4 };
        reader.json_exiting(&[action, target, "--dry-run", "--json"], want);
        reader.json_exiting(&[action, target, "--json"], 4);
    }
    reader.json_exiting(&["unstick", &pid, "--dry-run", "--json"], 0);
    reader.json_exiting(&["kill", "99999999", "--json"], 2);
    reader.json_exiting(&["kill", &pid, "--bogus-flag", "--json"], 4);

    // ---- wait: a timeout, and an exit
    let (waited, _) = reader.json(&["wait", &pid, "--timeout", "1", "--json"]);
    assert_eq!(waited["timed_out"], true, "{waited}");
    let leaving = Listener::start();
    let leaving_pid = leaving.pid();
    let ending = {
        // the listener leaves while `wait` is waiting for it
        let mut child = leaving;
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(700));
            let _ = child.child.kill();
            let _ = child.child.wait();
        })
    };
    let waited = reader.json_exiting(&["wait", &leaving_pid, "--timeout", "30", "--json"], 0);
    assert_eq!(waited["exited"][0]["pid"].to_string(), leaving_pid);
    ending.join().unwrap();

    // ---- unstick: a receipt, with an idle process it leaves alone
    let idle = Listener::start();
    let unstuck = reader.json_exiting(&["unstick", &idle.pid(), "--yes", "--json"], 0);
    assert_eq!(unstuck["not_stuck_count"], 1, "{unstuck}");

    // ---- the receipts of the commands that change processes
    #[cfg(unix)]
    {
        let sleeper = Listener::start();
        let frozen = reader.json_exiting(&["freeze", &sleeper.pid(), "--yes", "--json"], 0);
        assert_eq!(frozen["processes"][0]["outcome"], "succeeded", "{frozen}");
        reader.json_exiting(&["thaw", &sleeper.pid(), "--yes", "--json"], 0);
    }
    let stopped = Listener::start();
    let receipt = reader.json_exiting(&["stop", &stopped.pid(), "--yes", "--json"], 0);
    assert_eq!(receipt["count"], 1, "{receipt}");
    let killed = Listener::start();
    let receipt = reader.json_exiting(&["kill", &killed.pid(), "--yes", "--json"], 0);
    assert_eq!(receipt["processes"][0]["outcome"], "succeeded", "{receipt}");
    let holder = Listener::start();
    let receipt = reader.json_exiting(&["free", &holder.port(), "--yes", "--json"], 0);
    assert_eq!(receipt["processes"][0]["outcome"], "succeeded", "{receipt}");
    assert_eq!(receipt["count"], 1, "{receipt}");

    // ---- one word, one meaning, over everything read
    if let Some(wrong) = contract::mixed_kinds(&reader.kinds) {
        panic!("{wrong}\nafter reading:\n  {}", reader.read.join("\n  "));
    }
}
