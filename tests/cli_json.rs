//! JSON output contract tests: every command prints exactly one JSON document
//! (`watch` streams one per refresh),
//! `action` is the canonical command name, and `success: false` means a non-zero exit.

mod contract;

use assert_cmd::Command;
use serde_json::Value;

/// A PID that will not exist on any test machine
const MISSING_PID: &str = "99999999";

fn proc_cmd() -> Command {
    Command::cargo_bin("proc").unwrap()
}

/// Run proc and parse stdout as exactly one JSON document.
fn run_json(args: &[&str]) -> (Value, i32) {
    let output = proc_cmd().args(args).output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout is not a single JSON document ({e}):\n{stdout}"));
    let code = output.status.code().unwrap();
    contract::check(&json, code, args);
    (json, code)
}

/// The check every `run_json` makes refuses each way a document can break
/// the contract, and takes the ones that keep it
#[test]
fn the_contract_check_refuses_each_break() {
    use serde_json::json;
    let kept = [
        (json!({"action": "list", "success": true, "count": 0}), 0),
        (
            json!({"action": "kill", "success": false, "error": "x", "exit_code": 2, "reason": "not_found"}),
            2,
        ),
        (
            json!({"action": "kill", "success": false, "error": "x", "exit_code": 2, "reason": "ruled_out", "ruled_out_by": ["--by"]}),
            2,
        ),
        (json!({"action": "kill", "success": false, "failed": []}), 1),
        (
            json!({"action": "kill", "success": false, "error": "x", "exit_code": 4, "reason": "invalid_input"}),
            4,
        ),
        // a receipt: `processes` with an outcome each, beside the older lists
        (
            json!({"action": "kill", "success": false, "succeeded_count": 1, "failed_count": 1,
                "succeeded": [{"pid": 1}], "failed": [{"process": {"pid": 2}, "error": "no"}],
                "processes": [{"pid": 2, "outcome": "failed", "error": "no"},
                              {"pid": 1, "outcome": "succeeded"}], "count": 2}),
            1,
        ),
        // the deprecated names break one rule each, and are let through
        (
            json!({"action": "wait", "success": true, "elapsed_seconds": 1, "initial_count": 0,
                "exited": [{"exited_after_seconds": 1}], "still_running": [], "count": 1,
                "elapsed_human": "1s"}),
            0,
        ),
    ];
    for (doc, code) in kept {
        assert_eq!(contract::violation(&doc, code), None, "{doc}");
    }
    let broken = [
        (json!({"action": "list", "success": true}), 5),
        (json!({"success": true}), 0),
        (json!({"action": "list", "success": false}), 0),
        (json!({"action": "list", "success": true}), 1),
        (json!({"action": "kill", "success": false, "error": "x"}), 2),
        (
            json!({"action": "kill", "success": false, "error": "x", "exit_code": 1}),
            2,
        ),
        (
            json!({"action": "kill", "success": false, "error": "x", "exit_code": 4}),
            4,
        ),
        (
            json!({"action": "kill", "success": false, "error": "x", "exit_code": 2, "reason": "invalid_input"}),
            2,
        ),
        // an error without a reason, a reason without an error
        (
            json!({"action": "kill", "success": false, "error": "x", "exit_code": 2}),
            2,
        ),
        (
            json!({"action": "list", "success": true, "reason": "not_found"}),
            0,
        ),
        // ruled_out and ruled_out_by come together
        (
            json!({"action": "kill", "success": false, "error": "x", "exit_code": 2, "reason": "ruled_out"}),
            2,
        ),
        (
            json!({"action": "kill", "success": false, "error": "x", "exit_code": 2, "reason": "not_found", "ruled_out_by": ["--by"]}),
            2,
        ),
    ];
    for (doc, code) in broken {
        assert!(
            contract::violation(&doc, code).is_some(),
            "{doc} with exit {code}"
        );
    }
}

/// The names rules refuse each way a key can drift, and take the names that
/// keep them
#[test]
fn the_names_check_refuses_each_drift() {
    use serde_json::json;
    let broken = [
        // a deprecated name is accepted only in the command that carries it
        json!({"action": "list", "success": true, "found_count": 1}),
        json!({"action": "info", "success": true, "initial_count": 1}),
        json!({"action": "info", "success": true, "elapsed_seconds": 1}),
        // rule 3: a duration without _secs
        json!({"action": "x", "success": true, "waited_seconds": 1}),
        json!({"action": "x", "success": true, "uptime": 1}),
        // rule 2: a count beside no list, a count that is not a number, a
        // count that is not its list's length
        json!({"action": "x", "success": true, "matched_count": 1}),
        json!({"action": "x", "success": true, "failed_count": "1"}),
        json!({"action": "x", "success": true, "count": 2, "processes": [{"pid": 1}]}),
        json!({"action": "x", "success": true, "count": 1}),
        // rule 5: an outcome without its error, an error without its outcome,
        // a kept list that differs from `processes`
        json!({"action": "x", "success": true, "processes": [{"pid": 1, "outcome": "failed"}], "count": 1}),
        json!({"action": "x", "success": true, "processes": [{"pid": 1, "outcome": "succeeded", "error": "no"}], "count": 1}),
        json!({"action": "x", "success": true, "succeeded": [{"pid": 2}],
            "processes": [{"pid": 1, "outcome": "succeeded"}], "count": 1}),
        json!({"action": "x", "success": true, "failed": [],
            "processes": [{"pid": 1, "outcome": "failed", "error": "no"}], "count": 1}),
    ];
    for doc in broken {
        assert!(contract::names_violation(&doc).is_some(), "{doc}");
    }
    // deep inside a list is as wrong as at the top
    let deep = json!({"processes": [{"pid": 1, "age_seconds": 3}], "count": 1});
    assert!(contract::names_violation(&deep).is_some());

    // rule 6, across documents
    let mut kinds = contract::Kinds::new();
    contract::note_kinds(
        &json!({"action": "kill", "failed": [], "ok": []}),
        &mut kinds,
    );
    contract::note_kinds(
        &json!({"action": "unstick", "failed": 0, "ok": 0}),
        &mut kinds,
    );
    let wrong = contract::mixed_kinds(&kinds).expect("`ok` is a list and a number");
    assert!(wrong.contains("`ok`"), "{wrong}");
    // any two kinds, not only a list and a number
    let mut strings = contract::Kinds::new();
    contract::note_kinds(&json!({"action": "a", "name": "x"}), &mut strings);
    contract::note_kinds(
        &json!({"action": "b", "name": {"first": "x"}}),
        &mut strings,
    );
    assert!(contract::mixed_kinds(&strings).unwrap().contains("`name`"));
    // the one allowed: unstick's deprecated `failed`
    let mut allowed = contract::Kinds::new();
    contract::note_kinds(&json!({"action": "kill", "failed": []}), &mut allowed);
    contract::note_kinds(&json!({"action": "unstick", "failed": 0}), &mut allowed);
    assert_eq!(contract::mixed_kinds(&allowed), None);
}

#[test]
fn usage_error_exits_invalid_input_with_json() {
    let (json, code) = run_json(&["kill", MISSING_PID, "--bogus-flag", "--json"]);
    assert_eq!(code, 4);
    assert_eq!(json["action"], "kill");
    assert_eq!(json["success"], false);
    assert_eq!(json["exit_code"], 4);
    assert!(json["error"].as_str().unwrap().contains("--bogus-flag"));
    assert_eq!(json["reason"], "invalid_input", "{json}");
}

#[test]
fn a_wrong_flag_says_invalid_input() {
    // What an agent tester hit: `--name` is not a flag of `kill`
    let (json, code) = run_json(&["kill", "--name", "wispworker", "-j"]);
    assert_eq!(code, 4, "{json}");
    assert_eq!(json["reason"], "invalid_input", "{json}");
    assert!(json.get("needs_confirmation").is_none(), "{json}");
}

#[test]
fn a_bad_value_says_invalid_input_whether_clap_or_proc_finds_it() {
    // clap rejects the value while parsing
    let (json, code) = run_json(&["list", "--sort", "bogus", "--json"]);
    assert_eq!(code, 4, "{json}");
    assert_eq!(json["reason"], "invalid_input", "{json}");
    // proc rejects it after parsing (a missing directory: invalid on every OS,
    // unlike a bad --signal name, which Windows rejects as not supported)
    let missing = std::env::temp_dir().join("proc-test-no-such-directory");
    let (json, code) = run_json(&["in", missing.to_str().unwrap(), "--json"]);
    assert_eq!(code, 4, "{json}");
    assert_eq!(json["reason"], "invalid_input", "{json}");
}

#[test]
fn every_error_says_why_and_a_result_does_not() {
    let (json, code) = run_json(&["kill", MISSING_PID, "--json"]);
    assert_eq!(code, 2, "{json}");
    assert_eq!(json["reason"], "not_found", "{json}");

    // a wait that times out prints its own result, exit 1, which is not an
    // error document; the process it waits for is this test, still running
    let pid = std::process::id().to_string();
    let (json, code) = run_json(&["wait", &pid, "--timeout", "1", "--json"]);
    assert_eq!(code, 1, "{json}");
    assert!(json.get("reason").is_none(), "{json}");
}

#[test]
fn usage_error_without_json_goes_to_stderr() {
    let output = proc_cmd()
        .args(["kill", MISSING_PID, "--bogus-flag"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument"));
}

#[test]
fn alias_reports_canonical_action() {
    let (json, code) = run_json(&["k", MISSING_PID, "--yes", "--json"]);
    assert_eq!(code, 2);
    assert_eq!(json["action"], "kill");
}

#[test]
fn inspect_is_an_alias_of_info() {
    let (json, code) = run_json(&["inspect", MISSING_PID, "--json"]);
    assert_eq!(code, 2);
    assert_eq!(json["action"], "info");
}

#[test]
fn short_flag_cluster_enables_json_errors() {
    let (json, code) = run_json(&["kill", MISSING_PID, "-yj"]);
    assert_eq!(code, 2);
    assert_eq!(json["success"], false);
}

#[test]
fn why_not_found_prints_json_error() {
    let (json, code) = run_json(&["why", MISSING_PID, "--json"]);
    assert_eq!(code, 2);
    assert_eq!(json["action"], "why");
    assert_eq!(json["success"], false);
}

#[test]
fn on_multi_target_not_found_prints_json_error() {
    let (json, code) = run_json(&["on", &format!("{MISSING_PID},{MISSING_PID}1"), "--json"]);
    assert_eq!(code, 2);
    assert_eq!(json["action"], "on");
    assert_eq!(json["success"], false);
}

#[test]
fn in_missing_directory_is_invalid_input() {
    let (json, code) = run_json(&["in", "/nonexistent/proc-test-dir", "--json"]);
    assert_eq!(code, 4);
    assert_eq!(json["action"], "in");
}

#[test]
fn unknown_status_filter_is_invalid_input() {
    // A typo must not silently match every process
    for cmd in ["list", "tree"] {
        let (json, code) = run_json(&[cmd, "--status", "stoped", "--json"]);
        assert_eq!(code, 4, "{cmd}: {json}");
        assert_eq!(json["action"], cmd);
        assert!(json["error"].as_str().unwrap().contains("stoped"));
    }
    let (_, code) = run_json(&["list", "--status", "STOP", "--json"]);
    assert_eq!(code, 0, "aliases and any case still work");
}

#[test]
fn no_process_has_been_up_longer_than_the_machine() {
    // A start time that can't be read must not turn into "up since 1970"
    let boot_uptime = sysinfo::System::uptime();
    let (json, code) = run_json(&["list", "--json"]);
    assert_eq!(code, 0);
    for p in json["processes"].as_array().unwrap() {
        if let Some(uptime) = p["uptime_secs"].as_u64() {
            assert!(
                uptime <= boot_uptime + 5,
                "{} (PID {}) reports {uptime}s, but the system has been up {boot_uptime}s",
                p["name"],
                p["pid"]
            );
        }
    }
}

#[test]
fn watch_rejects_intervals_it_cannot_use() {
    // nan, inf and negatives used to panic; 0 refreshed in a busy loop
    for bad in ["nan", "inf", "-1", "0"] {
        let arg = format!("--interval={bad}");
        let (json, code) = run_json(&["watch", &arg, "--json"]);
        assert_eq!(code, 4, "--interval {bad}: {json}");
        assert_eq!(json["success"], false);
    }
}

#[test]
fn a_colon_target_must_be_a_real_port() {
    // ":70000" used to fall through to a name search and exit 2 (not found)
    let cases: &[&[&str]] = &[
        &["on", ":70000", "--json"],
        &["why", ":abc", "--json"],
        &["info", ":0", "--json"],
        &["tree", ":99999", "--json"],
        &["watch", ":3000x", "--json"],
        &["kill", ":3000,:70000", "--dry-run", "--json"],
        &["free", ":65536", "--dry-run", "--json"],
    ];
    for args in cases {
        let (json, code) = run_json(args);
        assert_eq!(code, 4, "{args:?}: {json}");
        assert!(
            json["error"].as_str().unwrap().contains("not a port"),
            "{args:?}: {json}"
        );
    }
}

#[test]
fn ports_rejects_a_range_it_cannot_use() {
    // These used to be ignored, listing every port as if no range was given
    for bad in ["abc", "70000-80000", "9000-8000", "3000"] {
        let (json, code) = run_json(&["ports", "--range", bad, "--json"]);
        assert_eq!(code, 4, "--range {bad}: {json}");
        assert_eq!(json["success"], false);
    }
    let (json, code) = run_json(&["ports", "--range", "1-65535", "--json"]);
    assert_eq!(code, 0, "{json}");
}

#[test]
fn info_not_found_exits_not_found() {
    let (json, code) = run_json(&["info", MISSING_PID, "--json"]);
    assert_eq!(code, 2);
    assert_eq!(json["action"], "info");
    assert_eq!(json["success"], false);
    assert_eq!(json["reason"], "not_found", "{json}");
    assert_eq!(json["found_count"], 0, "{json}");
    assert_eq!(json["count"], 0, "{json}");
    assert_eq!(json["not_found"][0], MISSING_PID, "{json}");
}

#[test]
fn ipv6_loopback_listener_keeps_its_address() {
    // lsof and netstat bracket IPv6 addresses ([::1]:PORT). The address came
    // back as "]" on Windows and "[::1]" on macOS, so `proc ports --local` and
    // `--exposed` disagreed between systems
    let Ok(listener) = std::net::TcpListener::bind("[::1]:0") else {
        return; // no IPv6 on this machine
    };
    let port = listener.local_addr().unwrap().port();

    let (json, code) = run_json(&["on", &format!(":{port}"), "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["address"], "::1", "{json}");

    let (json, code) = run_json(&["ports", "--local", "--json"]);
    assert_eq!(code, 0, "{json}");
    let listed = json["ports"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["port"] == port && p["address"] == "::1");
    assert!(listed, "[::1]:{port} missing from ports --local: {json}");

    let (json, code) = run_json(&["ports", "--exposed", "--json"]);
    assert_eq!(code, 0, "{json}");
    let listed = json["ports"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["port"] == port);
    assert!(!listed, "[::1]:{port} is loopback, not exposed: {json}");
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::process::{Child, Command as StdCommand};

    /// A `sleep` child that is always killed when the test ends.
    struct Sleeper(Child);

    impl Sleeper {
        fn spawn() -> Self {
            Sleeper(StdCommand::new("sleep").arg("300").spawn().unwrap())
        }

        fn pid(&self) -> String {
            self.0.id().to_string()
        }
    }

    impl Drop for Sleeper {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    impl Sleeper {
        /// A child that exits after `secs`. The test doesn't reap it until drop,
        /// so once it exits it is a zombie: gone, but its PID is still taken.
        fn exiting_after(secs: &str) -> Self {
            Sleeper(StdCommand::new("sleep").arg(secs).spawn().unwrap())
        }
    }

    #[test]
    fn dry_run_reports_own_action() {
        let sleeper = Sleeper::spawn();
        for action in ["kill", "stop", "freeze", "thaw"] {
            let (json, code) = run_json(&[action, &sleeper.pid(), "--dry-run", "--json"]);
            assert_eq!(code, 0, "{action}");
            assert_eq!(json["action"], action);
            assert_eq!(json["dry_run"], true, "{action}");
            assert_eq!(json["count"], 1, "{action}");
        }
    }

    #[test]
    fn user_is_reported_by_name() {
        let sleeper = Sleeper::spawn();
        let id = StdCommand::new("id").arg("-un").output().unwrap();
        let expected = String::from_utf8(id.stdout).unwrap();

        let (json, code) = run_json(&["info", &sleeper.pid(), "--json"]);
        assert_eq!(code, 0);
        assert_eq!(json["processes"][0]["user"], expected.trim());
    }

    #[test]
    fn uptime_is_reported_in_seconds() {
        let sleeper = Sleeper::spawn();

        let (json, code) = run_json(&["info", &sleeper.pid(), "--json"]);
        assert_eq!(code, 0);
        let process = &json["processes"][0];
        let uptime = process["uptime_secs"].as_u64().unwrap();
        let start = process["start_time"].as_u64().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // The sleeper was just spawned, and uptime agrees with start_time
        assert!(uptime < 60, "uptime_secs = {uptime}");
        assert!(now.saturating_sub(start).abs_diff(uptime) <= 2);
    }

    /// A shell with one child, killed when the test ends (the child, a
    /// `sleep 30`, ends by itself)
    struct ShellWithChild(Child);

    impl Drop for ShellWithChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn a_process_names_its_parent_children_cpu_time_and_open_files() {
        let shell = ShellWithChild(
            StdCommand::new("sh")
                .args(["-c", "sleep 30 & wait"])
                .spawn()
                .unwrap(),
        );
        let pid = shell.0.id().to_string();
        // the shell starts its sleep after the exec: wait for it to show
        let mut process = Value::Null;
        for _ in 0..500 {
            let (json, code) = run_json(&["info", &pid, "--json"]);
            assert_eq!(code, 0, "{json}");
            process = json["processes"][0].clone();
            if process["child_count"] == 1 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(process["child_count"], 1, "{process}");
        assert!(process["parent_name"].is_string(), "{process}");
        assert!(process["cpu_time_secs"].is_u64(), "{process}");
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        assert!(process["open_files"].as_u64() >= Some(3), "{process}");

        // the child names the shell as its parent
        let (json, _) = run_json(&["by", "sleep", "--json"]);
        let sleeper = json["processes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["parent_pid"].as_u64() == Some(u64::from(shell.0.id())))
            .unwrap_or_else(|| panic!("no sleep under the shell: {json}"));
        assert!(sleeper["parent_name"].is_string(), "{sleeper}");
        assert_eq!(sleeper["child_count"], 0, "{sleeper}");
    }

    #[test]
    fn a_fact_the_os_does_not_give_is_an_absent_key_never_null() {
        let (json, code) = run_json(&["list", "--json"]);
        assert_eq!(code, 0);
        let processes = json["processes"].as_array().unwrap();
        for p in processes {
            for key in ["parent_name", "cpu_time_secs", "open_files", "child_count"] {
                assert!(
                    p.get(key).is_none_or(|value| !value.is_null()),
                    "{key} is null for {p}"
                );
            }
        }
        // the table is loaded: every row knows its children
        assert!(processes.iter().all(|p| p["child_count"].is_u64()));
    }

    #[test]
    fn every_process_view_reports_uptime() {
        let sleeper = Sleeper::spawn();
        let pid = sleeper.pid();
        let has_uptime = |v: &Value, what: &str| {
            assert!(v["uptime_secs"].as_u64().is_some(), "{what}: {v}");
        };

        let (tree, _) = run_json(&["tree", &pid, "--json"]);
        has_uptime(&tree["tree"][0], "tree node");

        // Ancestors owned by other users (PID 1 on macOS) have no readable
        // start time, so check the sleeper's own entry in each chain
        let own_entry = |chain: &Value| -> Value {
            chain
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["pid"].as_u64() == Some(u64::from(sleeper.0.id())))
                .cloned()
                .unwrap_or_else(|| panic!("sleeper not in chain: {chain}"))
        };

        let (ancestors, _) = run_json(&["tree", &pid, "--ancestors", "--json"]);
        has_uptime(
            &own_entry(&ancestors["ancestry"][0]["chain"]),
            "tree --ancestors chain entry",
        );

        let (why, _) = run_json(&["why", &pid, "--json"]);
        has_uptime(&why["results"][0]["process"], "why process");
        has_uptime(
            &own_entry(&why["results"][0]["ancestry"]),
            "why ancestry entry",
        );

        let (unstick, _) = run_json(&["unstick", &pid, "--dry-run", "--json"]);
        has_uptime(&unstick["processes"][0], "unstick process");

        let (on, _) = run_json(&["on", &pid, "--json"]);
        has_uptime(&on["process"], "on <pid> process");
    }

    #[test]
    fn every_process_object_has_the_full_fields() {
        let sleeper = Sleeper::spawn();
        let pid = sleeper.pid();
        let full = |v: &Value, what: &str| {
            for key in ["parent_pid", "user", "exe_path", "command"] {
                assert!(v.get(key).is_some(), "{what} lacks {key}: {v}");
            }
        };

        let (tree, _) = run_json(&["tree", &pid, "--json"]);
        full(&tree["tree"][0], "tree node");

        let (ancestors, _) = run_json(&["tree", &pid, "--ancestors", "--json"]);
        let chain = ancestors["ancestry"][0]["chain"].as_array().unwrap();
        let own = chain
            .iter()
            .find(|e| e["pid"].as_u64() == Some(u64::from(sleeper.0.id())))
            .unwrap_or_else(|| panic!("sleeper not in chain: {chain:?}"));
        full(own, "tree --ancestors chain entry");

        let (why, _) = run_json(&["why", &pid, "--json"]);
        full(&why["results"][0]["process"], "why process");
        let ancestry = why["results"][0]["ancestry"].as_array().unwrap();
        let own = ancestry
            .iter()
            .find(|e| e["pid"].as_u64() == Some(u64::from(sleeper.0.id())))
            .unwrap_or_else(|| panic!("sleeper not in ancestry: {ancestry:?}"));
        full(own, "why ancestry entry");

        let (unstick, _) = run_json(&["unstick", &pid, "--dry-run", "--json"]);
        full(&unstick["processes"][0], "unstick process");

        let (wait, _) = run_json(&["wait", &pid, "--timeout", "1", "--json"]);
        full(&wait["still_running"][0], "wait still-running process");

        // a process that exits: the reading taken before it went
        let exiting = Sleeper::exiting_after("1");
        let (wait, _) = run_json(&["wait", &exiting.pid(), "--json"]);
        full(&wait["exited"][0], "wait exited process");
    }

    #[test]
    fn on_multi_target_prints_one_document() {
        let first = Sleeper::spawn();
        let second = Sleeper::spawn();
        let targets = format!("{},{},{}", first.pid(), second.pid(), MISSING_PID);
        let (json, code) = run_json(&["on", &targets, "--json"]);
        assert_eq!(code, 0);
        assert_eq!(json["action"], "on");
        assert_eq!(json["count"], 2);
        assert_eq!(json["results"][0]["target"], first.pid());
        assert_eq!(json["not_found"][0], MISSING_PID);
    }

    #[test]
    fn why_multi_target_prints_one_document() {
        let first = Sleeper::spawn();
        let second = Sleeper::spawn();
        let targets = format!("{},{}", first.pid(), second.pid());
        let (json, code) = run_json(&["why", &targets, "--json"]);
        assert_eq!(code, 0);
        assert_eq!(json["count"], 2);
    }

    #[test]
    fn thaw_reports_thaw_action() {
        let sleeper = Sleeper::spawn();
        let (frozen, _) = run_json(&["freeze", &sleeper.pid(), "--yes", "--json"]);
        assert_eq!(frozen["action"], "freeze");
        let (json, code) = run_json(&["thaw", &sleeper.pid(), "--yes", "--json"]);
        assert_eq!(code, 0);
        assert_eq!(json["action"], "thaw");
        assert_eq!(json["succeeded_count"], 1);
    }

    #[test]
    fn wait_timeout_exits_non_zero() {
        let sleeper = Sleeper::spawn();
        let (json, code) = run_json(&[
            "wait",
            &sleeper.pid(),
            "--timeout",
            "1",
            "--interval",
            "1",
            "--json",
        ]);
        assert_eq!(code, 1);
        assert_eq!(json["success"], false);
        assert_eq!(json["timed_out"], true);
        assert_eq!(json["still_running"][0]["pid"], sleeper.0.id());
    }

    #[test]
    fn escape_sequences_in_process_data_are_never_printed_raw() {
        // A process can run from a directory whose name holds terminal escape
        // sequences; printing it as-is would let it recolour, retitle or rewrite
        // the terminal of whoever runs proc
        let parent = tempfile::tempdir().unwrap();
        let evil = parent.path().join("evil\x1b]0;pwned\x07\x1b[31mred");
        std::fs::create_dir(&evil).unwrap();
        let sleeper = Sleeper(
            StdCommand::new("sleep")
                .arg("300")
                .current_dir(&evil)
                .spawn()
                .unwrap(),
        );

        for command in ["info", "on", "why", "tree"] {
            let output = proc_cmd()
                .args([command, &sleeper.pid()])
                .env("NO_COLOR", "1")
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                !stdout.contains('\x1b') && !stdout.contains('\x07'),
                "{command} printed a raw escape sequence:\n{stdout:?}"
            );
        }
        let (json, code) = run_json(&["info", &sleeper.pid(), "--json"]);
        assert_eq!(code, 0);
        let cwd = json["processes"][0]["cwd"].as_str().unwrap();
        assert!(
            cwd.ends_with("evil\\x1b]0;pwned\\x07\\x1b[31mred"),
            "{cwd:?}"
        );
    }

    #[test]
    fn cpu_is_measured_where_it_is_shown() {
        // CPU used to be read a few milliseconds after the first refresh, so a
        // process spinning at 100% showed about 0.4%
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().canonicalize().unwrap();
        let spinning = Sleeper(
            StdCommand::new("sh")
                .args(["-c", "while :; do :; done"])
                .current_dir(&dir_path)
                .spawn()
                .unwrap(),
        );
        std::thread::sleep(std::time::Duration::from_millis(500));
        let pid = spinning.pid();
        let me = u64::from(spinning.0.id());
        let cpu_of = |list: &Value| -> Option<f64> {
            list.as_array()?
                .iter()
                .find(|p| p["pid"].as_u64() == Some(me))?["cpu_percent"]
                .as_f64()
        };

        // unmeasured reads ~0.4; a full core measured can read under 50 on a
        // loaded CI runner (49% seen), so 20 tells the two apart without flaking
        let (info, _) = run_json(&["info", &pid, "--json"]);
        assert!(cpu_of(&info["processes"]).unwrap() > 20.0, "info: {info}");

        let (on, _) = run_json(&["on", &pid, "--json"]);
        let on_cpu = on["process"]["cpu_percent"].as_f64().unwrap();
        assert!(on_cpu > 20.0, "on: {on}");

        let (busy, _) = run_json(&["list", "--min-cpu", "20", "--json"]);
        assert!(
            cpu_of(&busy["processes"]).is_some(),
            "--min-cpu 20 missed it: {busy}"
        );

        // sorted and cut by the measured figure. Only this directory is listed:
        // "in the top 5 of every process on the machine" depends on what else
        // the machine runs (measured, it was 5th of 5 in one of 40 runs)
        let (top, _) = run_json(&[
            "list",
            "--in",
            &dir_path.to_string_lossy(),
            "--sort",
            "cpu",
            "--limit",
            "1",
            "--json",
        ]);
        let top_cpu = cpu_of(&top["processes"]);
        assert!(top_cpu.is_some_and(|cpu| cpu > 20.0), "top by CPU: {top}");
    }

    #[test]
    fn changing_a_process_needs_yes_without_a_terminal_or_with_json() {
        // --json used to act without asking, and without a terminal the prompt
        // failed with "Dialog error: not a terminal"; both now exit 4 untouched
        let sleeper = Sleeper::spawn();
        let pid = sleeper.pid();
        for command in ["kill", "stop", "freeze", "thaw", "unstick"] {
            let (json, code) = run_json(&[command, &pid, "--json"]);
            assert_eq!(code, 4, "{command} --json without --yes: {json}");
            assert!(json["error"].as_str().unwrap().contains("--yes"), "{json}");
            assert_eq!(json["needs_confirmation"], true, "{command}: {json}");
            assert_eq!(json["reason"], "needs_confirmation", "{command}: {json}");
            let listed = json["processes"].as_array().unwrap();
            assert_eq!(listed.len(), 1, "{command}: {json}");
            assert_eq!(
                listed[0]["pid"].as_u64().map(|p| p.to_string()),
                Some(pid.clone()),
                "{command}: {json}"
            );

            // no --json, and stdin is not a terminal (assert_cmd closes it)
            let output = proc_cmd().args([command, &pid]).output().unwrap();
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(
                output.status.code(),
                Some(4),
                "{command} without a terminal: {stderr}"
            );
            assert!(stderr.contains("--yes"), "{command}: {stderr}");
        }
        let mut sleeper = sleeper;
        assert!(
            sleeper.0.try_wait().unwrap().is_none(),
            "the process was changed without --yes"
        );
        // and --dry-run still previews without --yes
        let (json, code) = run_json(&["kill", &pid, "--dry-run", "--json"]);
        assert_eq!(code, 0, "{json}");

        // other errors keep their plain shape, with their own reason
        let (json, code) = run_json(&["kill", "99999999", "--json"]);
        assert_ne!(code, 0, "{json}");
        assert!(json.get("needs_confirmation").is_none(), "{json}");
        assert!(json.get("processes").is_none(), "{json}");
        assert_eq!(json["reason"], "not_found", "{json}");
    }

    #[test]
    fn zombies_are_listed() {
        let sleeper = Sleeper::exiting_after("0");
        std::thread::sleep(std::time::Duration::from_millis(500));
        let (json, code) = run_json(&["list", "--status", "zombie", "--json"]);
        assert_eq!(code, 0);
        let found = json["processes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["pid"].as_u64() == Some(u64::from(sleeper.0.id())));
        assert!(found, "zombie {} not listed: {json}", sleeper.pid());
    }

    #[test]
    fn tree_and_why_status_is_lowercase() {
        let sleeper = Sleeper::spawn();
        let (tree, _) = run_json(&["tree", &sleeper.pid(), "--json"]);
        let status = tree["tree"][0]["status"].as_str().unwrap();
        assert_eq!(status, status.to_lowercase());

        let (why, _) = run_json(&["why", &sleeper.pid(), "--json"]);
        let status = why["results"][0]["process"]["status"].as_str().unwrap();
        assert_eq!(status, status.to_lowercase());
    }

    #[test]
    fn stop_rejects_an_unknown_signal_even_on_dry_run() {
        let mut sleeper = Sleeper::spawn();
        for extra in [&["--dry-run"][..], &["--yes"][..]] {
            let mut args = vec!["stop", "--signal", "9", "--json"];
            let pid = sleeper.pid();
            args.push(&pid);
            args.extend_from_slice(extra);
            let (json, code) = run_json(&args);
            assert_eq!(code, 4, "{extra:?}: {json}");
            assert_eq!(json["action"], "stop");
            assert!(json["error"].as_str().unwrap().contains("Unknown signal"));
        }
        assert!(
            sleeper.0.try_wait().unwrap().is_none(),
            "sleeper was signalled"
        );
    }

    #[test]
    #[cfg_attr(
        any(target_os = "freebsd", target_os = "netbsd"),
        ignore = "FreeBSD and NetBSD forget the path of a deleted working directory"
    )]
    fn in_finds_processes_in_a_deleted_directory() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().canonicalize().unwrap();
        let dir_str = dir_path.to_str().unwrap().to_string();
        let sleeper = Sleeper(
            StdCommand::new("sleep")
                .arg("300")
                .current_dir(&dir_path)
                .spawn()
                .unwrap(),
        );
        dir.close().unwrap();
        assert!(!dir_path.exists());

        for args in [
            vec!["in", dir_str.as_str(), "--json"],
            vec!["list", "--in", dir_str.as_str(), "--json"],
        ] {
            let (json, code) = run_json(&args);
            assert_eq!(code, 0, "{args:?}: {json}");
            let found = json["processes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["pid"].as_u64() == Some(u64::from(sleeper.0.id())))
                .unwrap_or_else(|| panic!("{args:?} missed the sleeper: {json}"));
            // Linux reports "<dir> (deleted)"; proc shows the plain path everywhere
            assert_eq!(found["cwd"], dir_str.as_str(), "{args:?}");
        }
    }

    #[test]
    fn orphans_kill_prints_one_document() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().canonicalize().unwrap();
        let dir_str = dir_path.to_str().unwrap();

        // The shell exits immediately, so the backgrounded sleep is reparented to PID 1
        StdCommand::new("sh")
            .args(["-c", "sleep 300 &"])
            .current_dir(&dir_path)
            .status()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));

        let (found, _) = run_json(&["orphans", "--in", dir_str, "--json"]);
        if found["count"] == 0 {
            // Some init systems (e.g. a subreaper) adopt the child instead of PID 1
            return;
        }

        let (json, code) = run_json(&["orphans", "--in", dir_str, "--kill", "--yes", "--json"]);
        assert_eq!(code, 0);
        assert_eq!(json["action"], "orphans");
        assert_eq!(json["succeeded_count"], 1);
        assert_eq!(json["failed_count"], 0);
        assert_eq!(json["count"], 1);
        assert_eq!(json["processes"][0]["outcome"], "succeeded");
        assert_eq!(json["processes"][0]["pid"], json["succeeded"][0]["pid"]);
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    #[test]
    fn ports_are_found_without_ss() {
        // Read from /proc/net/tcp{,6}; this test also runs where iproute2 isn't installed
        let v4 = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let v4_port = v4.local_addr().unwrap().port();
        let v6 = std::net::TcpListener::bind("[::1]:0").ok(); // IPv6 may be disabled
        let me = u64::from(std::process::id());

        let (json, code) = run_json(&["ports", "--local", "--json"]);
        assert_eq!(code, 0, "{json}");
        let ports = json["ports"].as_array().unwrap();
        let find = |port: u16| {
            ports
                .iter()
                .find(|p| p["port"].as_u64() == Some(u64::from(port)))
                .cloned()
        };

        let entry = find(v4_port).unwrap_or_else(|| panic!("127.0.0.1:{v4_port} missing: {json}"));
        assert_eq!(entry["pid"].as_u64(), Some(me));
        assert_eq!(entry["address"], "127.0.0.1");

        if let Some(v6) = v6 {
            let v6_port = v6.local_addr().unwrap().port();
            let entry = find(v6_port).unwrap_or_else(|| panic!("[::1]:{v6_port} missing: {json}"));
            assert_eq!(entry["pid"].as_u64(), Some(me));
            assert_eq!(entry["address"], "::1");
        }
    }

    #[test]
    fn list_shows_processes_not_their_threads() {
        // Keep an extra thread alive so this test process has more than one task
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || rx.recv().ok());

        let me = std::process::id();
        let thread_ids: Vec<u64> = std::fs::read_dir("/proc/self/task")
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
            .filter(|tid| *tid != u64::from(me))
            .collect();
        assert!(!thread_ids.is_empty(), "test needs a second thread");

        let (json, code) = run_json(&["list", "--json"]);
        assert_eq!(code, 0);
        let pids: Vec<u64> = json["processes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p["pid"].as_u64())
            .collect();
        assert!(
            pids.contains(&u64::from(me)),
            "the test process itself is listed"
        );
        for tid in &thread_ids {
            assert!(!pids.contains(tid), "thread {tid} was listed as a process");
        }

        tx.send(()).unwrap();
        worker.join().unwrap();
    }
}

#[cfg(windows)]
mod windows {
    use super::*;

    #[test]
    fn stop_signal_is_rejected_on_windows() {
        // Windows has no signals; --signal must not quietly become a normal stop
        let pid = std::process::id().to_string();
        let (json, code) = run_json(&["stop", &pid, "--signal", "HUP", "--dry-run", "--json"]);
        assert_eq!(code, 4, "{json}");
        assert_eq!(json["action"], "stop");
        assert!(json["error"].as_str().unwrap().contains("Windows"));
        assert_eq!(json["reason"], "not_supported", "{json}");
    }

    #[test]
    fn freeze_and_orphans_say_not_supported_on_windows() {
        let pid = std::process::id().to_string();
        for args in [&["freeze", &pid, "--dry-run"][..], &["orphans"][..]] {
            let mut full = args.to_vec();
            full.push("--json");
            let (json, code) = run_json(&full);
            assert_eq!(code, 4, "{args:?}: {json}");
            assert_eq!(json["reason"], "not_supported", "{args:?}: {json}");
        }
    }
}
