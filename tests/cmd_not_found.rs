//! The targets that match nothing, in the JSON of every command that takes
//! several targets.
//!
//! `not_found` is always there, an array that is empty when every target was
//! found: in a dry run, in the result of the action, and in the error that asks
//! for `--yes`. Every process these tests act on is a listener the test
//! started; the missing target is a PID above every OS maximum.

mod contract;

use assert_cmd::Command;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Stdio};

/// A PID no OS hands out
const MISSING: &str = "99999999";

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

/// A process the test owns. Killed and reaped on drop.
struct Owned {
    child: Child,
    port: u16,
}

impl Owned {
    fn start() -> Self {
        let mut child = std::process::Command::new(listener_path())
            .stdout(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .expect("spawn the listener example");
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
        Owned { child, port }
    }

    fn pid(&self) -> String {
        self.child.id().to_string()
    }

    fn port_target(&self) -> String {
        format!(":{}", self.port)
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Run proc and return its stdout and exit code.
fn run_text(args: &[&str]) -> (String, i32) {
    let output = proc_cmd().args(args).output().unwrap();
    (
        String::from_utf8(output.stdout).unwrap(),
        output.status.code().unwrap(),
    )
}

/// A port nothing listens on: a privileged one, which a test running at the
/// same time can't be handed as an ephemeral port (one the OS just took back
/// from another test can still be held for a moment by a child it forked).
/// Only dry runs and refusals name it: nothing is killed on a port the test
/// does not hold.
fn unused_port_target() -> String {
    ":1".to_string()
}

#[test]
fn a_dry_run_lists_the_targets_that_were_not_found() {
    let own = Owned::start();
    let pid = own.pid();
    let mut commands = vec!["kill", "stop"];
    if cfg!(unix) {
        commands.extend(["freeze", "thaw", "unstick"]);
    }
    for command in commands {
        // separate arguments, and the comma form
        for args in [
            vec![command, &pid, MISSING, "--dry-run", "--json"],
            vec![command, &format!("{pid},{MISSING}"), "--dry-run", "--json"],
        ] {
            let (json, code) = run_json(&args);
            assert_eq!(code, 0, "{args:?}: {json}");
            assert_eq!(json["success"], true, "{args:?}: {json}");
            assert_eq!(json["not_found"], json!([MISSING]), "{args:?}: {json}");
        }
        // nothing missing: the field is there, empty
        let (json, code) = run_json(&[command, &pid, "--dry-run", "--json"]);
        assert_eq!(code, 0, "{command}: {json}");
        assert_eq!(json["not_found"], json!([]), "{command}: {json}");
    }
}

#[test]
fn the_result_of_kill_and_stop_lists_the_targets_that_were_not_found() {
    for command in ["kill", "stop"] {
        let own = Owned::start();
        let (json, code) = run_json(&[command, &own.pid(), MISSING, "--yes", "--json"]);
        assert_eq!(code, 0, "{command}: {json}");
        assert_eq!(json["success"], true, "{command}: {json}");
        assert_eq!(json["succeeded_count"], 1, "{command}: {json}");
        assert_eq!(json["not_found"], json!([MISSING]), "{command}: {json}");
    }
    let own = Owned::start();
    let (json, _) = run_json(&["kill", &own.pid(), "--yes", "--json"]);
    assert_eq!(json["not_found"], json!([]), "{json}");
}

#[cfg(unix)]
#[test]
fn the_result_of_freeze_and_thaw_lists_the_targets_that_were_not_found() {
    let own = Owned::start();
    let pid = own.pid();
    for command in ["freeze", "thaw"] {
        let (json, code) = run_json(&[command, &pid, MISSING, "--yes", "--json"]);
        assert_eq!(code, 0, "{command}: {json}");
        assert_eq!(json["succeeded_count"], 1, "{command}: {json}");
        assert_eq!(json["not_found"], json!([MISSING]), "{command}: {json}");
    }
}

#[cfg(unix)]
#[test]
fn the_result_of_unstick_lists_the_targets_that_were_not_found() {
    let own = Owned::start();
    let pid = own.pid();
    let (json, code) = run_json(&["unstick", &pid, MISSING, "--yes", "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["found"], 1, "{json}");
    assert_eq!(json["not_found"], json!([MISSING]), "{json}");

    // no target at all: nothing can be missing, and the field is still there
    let (json, _) = run_json(&["unstick", "--dry-run", "--json", "--timeout", "999999"]);
    assert_eq!(json["not_found"], json!([]), "{json}");
}

#[test]
fn asking_for_yes_lists_the_targets_that_were_not_found() {
    let own = Owned::start();
    let pid = own.pid();
    let mut commands = vec!["kill", "stop"];
    if cfg!(unix) {
        commands.extend(["freeze", "thaw", "unstick"]);
    }
    for command in commands {
        let (json, code) = run_json(&[command, &pid, MISSING, "--json"]);
        assert_eq!(code, 4, "{command}: {json}");
        assert_eq!(json["success"], false, "{command}: {json}");
        assert_eq!(json["needs_confirmation"], true, "{command}: {json}");
        assert_eq!(json["not_found"], json!([MISSING]), "{command}: {json}");

        let (json, code) = run_json(&[command, &pid, "--json"]);
        assert_eq!(code, 4, "{command}: {json}");
        assert_eq!(json["not_found"], json!([]), "{command}: {json}");
    }
}

#[test]
fn free_lists_the_ports_that_were_not_found() {
    let own = Owned::start();
    let held = own.port_target();
    let free = unused_port_target();

    let (json, code) = run_json(&["free", &held, &free, "--dry-run", "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["not_found"], json!([free]), "{json}");

    let (json, code) = run_json(&["free", &held, "--dry-run", "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["not_found"], json!([]), "{json}");

    let (json, code) = run_json(&["free", &held, &free, "--json"]);
    assert_eq!(code, 4, "{json}");
    assert_eq!(json["needs_confirmation"], true, "{json}");
    assert_eq!(json["not_found"], json!([free]), "{json}");

    // the result, with --yes: it acts on the port the test holds; `:1` is
    // privileged, so nothing is expected on it. Checked first: if something
    // does hold it on this machine, skip rather than act on it.
    let (probe, code) = run_json(&["on", &free, "--json"]);
    if code != 2 {
        proc_cli::err_println!("skipped: something holds {free}: {probe}");
        return;
    }
    let (json, code) = run_json(&["free", &held, &free, "--yes", "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["success"], true, "{json}");
    assert_eq!(json["not_found"], json!([free]), "{json}");
}

#[test]
fn free_with_every_port_already_free_says_which() {
    let free = unused_port_target();
    let (json, code) = run_json(&["free", &free, "--dry-run", "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["count"], 0, "{json}");
    assert_eq!(json["not_found"], json!([free]), "{json}");
}

#[test]
fn wait_lists_the_targets_that_were_not_found() {
    let own = Owned::start();
    let pid = own.pid();
    // the process never exits, so the wait times out; the field is on that too
    let (json, code) = run_json(&["wait", &pid, MISSING, "--timeout", "1", "--json"]);
    assert_eq!(code, 1, "{json}");
    assert_eq!(json["timed_out"], true, "{json}");
    assert_eq!(json["not_found"], json!([MISSING]), "{json}");

    let (json, _) = run_json(&["wait", &pid, "--timeout", "1", "--json"]);
    assert_eq!(json["not_found"], json!([]), "{json}");
}

#[test]
fn wait_that_finishes_lists_the_targets_that_were_not_found() {
    let mut own = Owned::start();
    let pid = own.pid();
    // end the process ourselves a moment after wait starts
    let waiter = std::thread::spawn({
        let pid = pid.clone();
        move || run_json(&["wait", &pid, MISSING, "--timeout", "60", "--json"])
    });
    std::thread::sleep(std::time::Duration::from_millis(1500));
    own.child.kill().unwrap();
    let (json, code) = waiter.join().unwrap();
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["success"], true, "{json}");
    assert_eq!(json["not_found"], json!([MISSING]), "{json}");
}

#[test]
fn info_and_on_list_the_targets_that_were_not_found() {
    let own = Owned::start();
    let pid = own.pid();
    for command in ["info", "on"] {
        let (json, code) = run_json(&[command, &pid, MISSING, "--json"]);
        assert_eq!(code, 0, "{command}: {json}");
        assert_eq!(json["not_found"], json!([MISSING]), "{command}: {json}");
    }
    let (json, _) = run_json(&["info", &pid, "--json"]);
    assert_eq!(json["not_found"], json!([]), "{json}");
}

#[test]
fn why_lists_the_targets_that_were_not_found() {
    let own = Owned::start();
    let pid = own.pid();
    let (json, code) = run_json(&["why", &pid, MISSING, "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["count"], 1, "{json}");
    assert_eq!(json["not_found"], json!([MISSING]), "{json}");

    let (json, _) = run_json(&["why", &pid, "--json"]);
    assert_eq!(json["not_found"], json!([]), "{json}");
}

#[test]
fn a_name_that_matches_nothing_is_listed_as_typed() {
    let own = Owned::start();
    let pid = own.pid();
    let name = "pnf-no-such-process-name";
    let (json, code) = run_json(&["kill", &pid, name, "--dry-run", "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["not_found"], json!([name]), "{json}");
}

#[test]
fn the_error_when_every_target_is_missing_lists_them() {
    // It named them only inside `error`, joined into one string, so a program
    // had to parse the text to learn which targets were missing
    let port = unused_port_target();
    let mut commands: Vec<Vec<&str>> = vec![
        vec!["kill", "--dry-run"],
        vec!["stop", "--dry-run"],
        vec!["wait", "--timeout", "1"],
        vec!["why"],
        vec!["on"],
        vec!["watch"],
    ];
    if cfg!(unix) {
        commands.extend([
            vec!["freeze", "--dry-run"],
            vec!["thaw", "--dry-run"],
            vec!["unstick", "--dry-run"],
        ]);
    }
    for command in commands {
        let mut args = command.clone();
        args.extend([MISSING, "99999998", "--json"]);
        let (json, code) = run_json(&args);
        assert_eq!(code, 2, "{args:?}: {json}");
        assert_eq!(json["reason"], "not_found", "{args:?}: {json}");
        assert_eq!(
            json["not_found"],
            json!([MISSING, "99999998"]),
            "{args:?}: {json}"
        );
    }
    // a port and a name, as typed, and a comma list split as it is everywhere
    let (json, code) = run_json(&[
        "kill",
        &format!("{MISSING},{port}"),
        "pnf-no-such-process-name",
        "--dry-run",
        "--json",
    ]);
    assert_eq!(code, 2, "{json}");
    assert_eq!(
        json["not_found"],
        json!([MISSING, port, "pnf-no-such-process-name"]),
        "{json}"
    );
}

#[test]
fn a_target_its_guards_rule_out_was_found_so_the_error_has_no_not_found() {
    let own = Owned::start();
    let pid = own.pid();
    let (json, code) = run_json(&[
        "kill",
        &pid,
        "--by",
        "pnf-no-such-process-name",
        "--dry-run",
        "--json",
    ]);
    assert_eq!(code, 2, "{json}");
    assert_eq!(json["reason"], "ruled_out", "{json}");
    assert!(json.get("not_found").is_none(), "{json}");
}

#[test]
fn people_are_still_warned_in_the_human_output() {
    let own = Owned::start();
    let pid = own.pid();
    let mut commands = vec!["kill", "stop", "unstick"];
    if cfg!(unix) {
        commands.extend(["freeze", "thaw"]);
    }
    for command in commands {
        let (out, code) = run_text(&[command, &pid, MISSING, "--dry-run"]);
        assert_eq!(code, 0, "{command}: {out}");
        assert!(
            out.contains(&format!("Not found: {MISSING}")),
            "{command}: {out}"
        );
        assert!(!out.contains("not_found"), "{command}: {out}");
    }
}
