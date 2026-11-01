//! Failures that used to pass silently now say what went wrong.
//!
//! `tree -a` with no target, `tree <unknown name>`, an empty target, an `--in`
//! directory that does not exist, a relative path when the current directory
//! is gone, and `--in` against a directory whose name holds a control
//! character each looked like an answer (exit 0, an empty or a full list).
//! Every case here only reads, runs `--dry-run`, or acts on a process the test
//! started.

mod contract;

use clap::CommandFactory;
use proc_cli::cli::Cli;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn proc_cmd() -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::cargo_bin("proc").unwrap();
    cmd.env("NO_COLOR", "1");
    // a case that wrongly succeeds must not run forever (`watch`)
    cmd.timeout(Duration::from_secs(20));
    cmd
}

/// Runs proc with `args` and returns its JSON document and exit code
fn run_json(args: &[&str]) -> (Value, i32) {
    let out = proc_cmd().args(args).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let json = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("proc {args:?} printed no JSON ({e}): {text}"));
    let code = out.status.code().unwrap_or(-1);
    contract::check(&json, code, args);
    (json, code)
}

/// `target/debug/examples/listener[.exe]`, found relative to this test binary
fn listener_path() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("examples");
    path.push(format!("listener{}", std::env::consts::EXE_SUFFIX));
    assert!(path.exists(), "build the example first: {}", path.display());
    path
}

/// A listener the test owns, running in `dir`; killed and reaped on drop
struct Owned(Child);

impl Owned {
    fn start_in(dir: &Path) -> Self {
        let mut child = Command::new(listener_path())
            .current_dir(dir)
            .stdout(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .expect("spawn the listener example");
        // the listener prints `PORT <n>` once it is up
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert!(line.starts_with("PORT "), "listener printed {line:?}");
        Owned(child)
    }

    fn pid(&self) -> String {
        self.0.id().to_string()
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A fresh directory, in the form processes report their cwd in
fn fresh_dir() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    let dir = tmp.path().canonicalize().unwrap();
    #[cfg(not(unix))]
    let dir = tmp.path().to_path_buf();
    (tmp, dir)
}

fn listed_pids(json: &Value) -> Vec<u64> {
    let mut pids = Vec::new();
    collect_pids(json, &mut pids);
    pids
}

fn collect_pids(json: &Value, pids: &mut Vec<u64>) {
    match json {
        Value::Object(map) => {
            if let Some(pid) = map.get("pid").and_then(Value::as_u64) {
                pids.push(pid);
            }
            map.values().for_each(|v| collect_pids(v, pids));
        }
        Value::Array(items) => items.iter().for_each(|v| collect_pids(v, pids)),
        _ => {}
    }
}

#[test]
fn tree_ancestors_without_a_target_is_invalid_input() {
    let (json, code) = run_json(&["tree", "-a", "--json"]);
    assert_eq!(code, 4, "{json}");
    assert_eq!(json["reason"], "invalid_input", "{json}");
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("--ancestors needs a target"),
        "{json}"
    );

    let out = proc_cmd().args(["tree", "--ancestors"]).output().unwrap();
    assert_eq!(out.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--ancestors needs a target"));
}

#[test]
fn tree_of_a_name_that_matches_nothing_is_not_found() {
    let (json, code) = run_json(&["tree", "no-such-process-name-7c1e", "--json"]);
    assert_eq!(code, 2, "{json}");
    assert_eq!(json["success"], false, "{json}");

    let (json, code) = run_json(&["tree", "no-such-process-name-7c1e", "-a", "--json"]);
    assert_eq!(code, 2, "{json}");
}

#[test]
fn an_empty_target_is_invalid_input() {
    for target in ["", ",", " , "] {
        for command in ["watch", "kill", "on", "info"] {
            let (json, code) = run_json(&[command, target, "--json"]);
            assert_eq!(code, 4, "{command} {target:?}: {json}");
            assert_eq!(
                json["reason"], "invalid_input",
                "{command} {target:?}: {json}"
            );
            assert!(
                json["error"].as_str().unwrap().contains("can't be empty"),
                "{command} {target:?}: {json}"
            );
        }
    }
}

/// The arguments each command with `--in` runs with, before `--in`. `{pid}` is
/// a process the test owns; changing commands only preview. The exit code a
/// missing `--in` directory gives: 4 on every one. (`on` said its targets were
/// not in the directory, exit 2, the same as for a directory that exists, so
/// a typo looked like an answer.)
fn in_dir_rows(pid: &str, exe: &str) -> Vec<(&'static str, Vec<String>, i32)> {
    let s = |args: &[&str]| {
        args.iter()
            .map(|a| a.replace("{pid}", pid).replace("{exe}", exe))
            .collect::<Vec<_>>()
    };
    vec![
        ("on", s(&["on", "{pid}"]), 4),
        ("for", s(&["for", "{exe}"]), 4),
        ("by", s(&["by", "listener"]), 4),
        ("list", s(&["list"]), 4),
        ("info", s(&["info", "{pid}"]), 4),
        ("ports", s(&["ports"]), 4),
        ("kill", s(&["kill", "{pid}", "--dry-run"]), 4),
        ("stop", s(&["stop", "{pid}", "--dry-run"]), 4),
        ("tree", s(&["tree"]), 4),
        ("watch", s(&["watch"]), 4),
        ("stuck", s(&["stuck"]), 4),
        ("unstick", s(&["unstick", "--dry-run"]), 4),
        ("freeze", s(&["freeze", "{pid}", "--dry-run"]), 4),
        ("thaw", s(&["thaw", "{pid}", "--dry-run"]), 4),
        ("orphans", s(&["orphans"]), 4),
        ("why", s(&["why", "{pid}"]), 4),
        ("free", s(&["free", ":1", "--dry-run"]), 4),
        ("wait", s(&["wait", "{pid}"]), 4),
    ]
}

#[test]
fn every_command_with_in_has_a_row() {
    let rows = in_dir_rows("1", "x");
    let missing: Vec<String> = Cli::command()
        .get_subcommands()
        .filter(|sub| sub.get_arguments().any(|a| a.get_long() == Some("in")))
        .map(|sub| sub.get_name().to_string())
        .filter(|name| !rows.iter().any(|(row, _, _)| row == name))
        .collect();
    assert!(missing.is_empty(), "add these to in_dir_rows: {missing:?}");
}

#[test]
fn a_missing_in_directory_is_an_error_for_every_command() {
    let (_tmp, dir) = fresh_dir();
    let owned = Owned::start_in(&dir);
    let pid = owned.pid();
    let exe = listener_path().to_string_lossy().into_owned();
    let missing = dir.join("no-such-directory");
    let missing = missing.to_str().unwrap();
    let has_in: Vec<String> = Cli::command()
        .get_subcommands()
        .filter(|sub| sub.get_arguments().any(|a| a.get_long() == Some("in")))
        .map(|sub| sub.get_name().to_string())
        .collect();

    for (command, mut args, want) in in_dir_rows(&pid, &exe) {
        if !has_in.iter().any(|c| c == command) {
            continue;
        }
        // freeze, thaw and orphans are not supported on Windows, which they say first
        if cfg!(windows) && matches!(command, "freeze" | "thaw" | "orphans") {
            continue;
        }
        args.extend([
            "--in".to_string(),
            missing.to_string(),
            "--json".to_string(),
        ]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let (json, code) = run_json(&args);
        assert_eq!(code, want, "{args:?}: {json}");
        if want == 4 {
            assert_eq!(json["reason"], "invalid_input", "{args:?}: {json}");
            assert!(
                json["error"]
                    .as_str()
                    .unwrap()
                    .contains("Directory not found"),
                "{args:?}: {json}"
            );
        }
    }
    // nothing above changed the process it previewed
    drop(owned);
}

/// A deleted directory that a process still runs in is kept, so its leftovers
/// can be found
#[cfg(unix)]
#[test]
#[cfg_attr(
    any(target_os = "freebsd", target_os = "netbsd"),
    ignore = "FreeBSD and NetBSD forget the path of a deleted working directory"
)]
fn a_deleted_directory_with_a_process_in_it_still_finds_the_process() {
    let (tmp, dir) = fresh_dir();
    let project = dir.join("project");
    std::fs::create_dir(&project).unwrap();
    let owned = Owned::start_in(&project);
    std::fs::remove_dir(&project).unwrap();
    let dir_arg = project.to_str().unwrap();

    let (json, code) = run_json(&["list", "--in", dir_arg, "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(listed_pids(&json), vec![u64::from(owned.0.id())], "{json}");

    let (json, code) = run_json(&["kill", &owned.pid(), "--in", dir_arg, "--dry-run", "--json"]);
    assert_eq!(code, 0, "{json}");
    drop(owned);
    drop(tmp);
}

/// The same, typed through a symlink (`/tmp` is `/private/tmp` on macOS): the
/// part of the path that still exists is resolved, the rest kept as typed
#[cfg(unix)]
#[test]
#[cfg_attr(
    any(target_os = "freebsd", target_os = "netbsd"),
    ignore = "FreeBSD and NetBSD forget the path of a deleted working directory"
)]
fn a_deleted_directory_typed_through_a_symlink_still_finds_the_process() {
    let (tmp, dir) = fresh_dir();
    let real = dir.join("real");
    std::fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, dir.join("link")).unwrap();
    let project = real.join("project");
    std::fs::create_dir(&project).unwrap();
    let owned = Owned::start_in(&project);
    std::fs::remove_dir(&project).unwrap();
    let typed = dir.join("link").join("project");
    let typed = typed.to_str().unwrap();

    for args in [
        &["list", "--in", typed, "--json"][..],
        &["in", typed, "--json"],
        &["kill", &owned.pid(), "--in", typed, "--dry-run", "--json"],
    ] {
        let (json, code) = run_json(args);
        assert_eq!(code, 0, "{args:?}: {json}");
        assert_eq!(
            listed_pids(&json),
            vec![u64::from(owned.0.id())],
            "{args:?}: {json}"
        );
    }
    drop(owned);
    drop(tmp);
}

/// An empty argument names nothing; each of these read it as something (the
/// current directory, a name no process has)
#[test]
fn an_empty_argument_is_invalid_input() {
    for args in [
        &["by", ""][..],
        &["list", ""],
        &["in", ""],
        &["for", ""],
        &["list", "--in", ""],
        &["list", "--in="],
        &["list", "-i="],
        &["kill", "1", "--by", ""],
    ] {
        let mut args = args.to_vec();
        args.push("--json");
        let (json, code) = run_json(&args);
        assert_eq!(code, 4, "{args:?}: {json}");
        let error = json["error"].as_str().unwrap();
        assert!(error.contains("can't be empty"), "{args:?}: {json}");
    }
    let (json, _) = run_json(&["list", "--in=", "--json"]);
    assert!(
        json["error"].as_str().unwrap().starts_with("--in "),
        "{json}"
    );
}

/// `tree --ancestors` takes `--in` and `--by` as the plain tree does: a
/// missing directory is exit 4, a target they rule out exit 2
#[test]
fn tree_ancestors_keeps_to_in_and_by() {
    let (_tmp, dir) = fresh_dir();
    let owned = Owned::start_in(&dir);
    let pid = owned.pid();
    let dir_arg = dir.to_str().unwrap();
    let gone = dir.join("nothing-here");

    let (json, code) = run_json(&["tree", &pid, "-a", "--in", gone.to_str().unwrap(), "--json"]);
    assert_eq!(code, 4, "{json}");
    let (json, code) = run_json(&["tree", &pid, "-a", "--by", "no-such-name", "--json"]);
    assert_eq!(code, 2, "{json}");
    let (json, code) = run_json(&[
        "tree", &pid, "-a", "--in", dir_arg, "--by", "listener", "--json",
    ]);
    assert_eq!(code, 0, "{json}");
    assert!(
        listed_pids(&json).contains(&u64::from(owned.0.id())),
        "{json}"
    );
}

/// A relative path when the current directory is gone has nothing to be
/// resolved against: say that, instead of an unrelated "not found"
#[cfg(unix)]
#[test]
fn a_relative_path_in_a_deleted_current_directory_says_so() {
    let (_tmp, dir) = fresh_dir();
    let gone = dir.join("gone");
    std::fs::create_dir(&gone).unwrap();
    let proc_path = assert_cmd::cargo::cargo_bin("proc");
    for args in [
        &["for", "./x", "--json"][..],
        &["list", "--in", "sub", "--json"],
        &["in", "sub", "--json"],
    ] {
        let script = format!(
            "cd '{}' && rmdir '{}' && exec '{}' {}",
            gone.display(),
            gone.display(),
            proc_path.display(),
            args.join(" ")
        );
        std::fs::create_dir_all(&gone).unwrap();
        let out = Command::new("sh").args(["-c", &script]).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        let json: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(out.status.code(), Some(4), "{args:?}: {json}");
        assert_eq!(json["reason"], "invalid_input", "{args:?}: {json}");
        assert!(
            json["error"]
                .as_str()
                .unwrap()
                .contains("current directory no longer exists"),
            "{args:?}: {json}"
        );
    }
}

/// `--in` compares the real directory, not the escaped text shown for it: a
/// name holding a newline and one holding a backslash and an `n` both read
/// `a\nb`, and each must find only its own process
#[cfg(unix)]
#[test]
fn in_tells_a_control_character_from_its_escaped_text() {
    let (_tmp, dir) = fresh_dir();
    let newline = dir.join("a\nb");
    let literal = dir.join("a\\nb");
    std::fs::create_dir(&newline).unwrap();
    std::fs::create_dir(&literal).unwrap();
    let in_newline = Owned::start_in(&newline);
    let in_literal = Owned::start_in(&literal);

    for (path, owner) in [(&newline, &in_newline), (&literal, &in_literal)] {
        let (json, code) = run_json(&["list", "--in", path.to_str().unwrap(), "--json"]);
        assert_eq!(code, 0, "{json}");
        assert_eq!(listed_pids(&json), vec![u64::from(owner.0.id())], "{json}");
    }
}

/// `tree --in` with no target shows the processes in the directory, each with
/// its subtree; it used to ignore `--in` and print every process
#[test]
fn tree_in_without_a_target_shows_only_that_directory() {
    let (_tmp, dir) = fresh_dir();
    let owned = Owned::start_in(&dir);
    let (json, code) = run_json(&["tree", "--in", dir.to_str().unwrap(), "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(listed_pids(&json), vec![u64::from(owned.0.id())], "{json}");
}

/// A process and its parent that both match are shown once: the child inside
/// the parent's tree, not again as a root of its own
#[cfg(unix)]
#[test]
fn tree_in_shows_a_matching_child_once() {
    let (_tmp, dir) = fresh_dir();
    let script = format!("'{}' > /dev/null; true", listener_path().display());
    let mut shell = Command::new("sh")
        .args(["-c", &script])
        .current_dir(&dir)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let shell_pid = u64::from(shell.id());
    // wait until the shell has started the listener
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let json = loop {
        let (json, _) = run_json(&["tree", "--in", dir.to_str().unwrap(), "--json"]);
        if listed_pids(&json).len() >= 2 || std::time::Instant::now() > deadline {
            break json;
        }
    };
    let pids = listed_pids(&json);
    let child = proc_cli::core::Process::find_all()
        .unwrap()
        .into_iter()
        .find(|p| p.parent_pid == Some(shell.id()))
        .map(|p| u64::from(p.pid));
    if let Some(child) = child {
        let _ = Command::new("kill").arg(child.to_string()).status();
    }
    let _ = shell.kill();
    let _ = shell.wait();
    assert_eq!(pids.len(), 2, "{json}");
    assert_eq!(pids[0], shell_pid, "the shell is the one root: {json}");
    assert_eq!(json["tree"].as_array().unwrap().len(), 1, "{json}");
}

/// `ports --in` a deleted directory that a process still runs in is that
/// directory with no ports, not a missing one, when the process listens on
/// nothing
#[cfg(unix)]
#[test]
#[cfg_attr(
    any(target_os = "freebsd", target_os = "netbsd"),
    ignore = "FreeBSD and NetBSD forget the path of a deleted working directory"
)]
fn ports_in_a_deleted_directory_whose_process_listens_on_nothing_is_empty() {
    let (_tmp, dir) = fresh_dir();
    let project = dir.join("project");
    std::fs::create_dir(&project).unwrap();
    let mut sleeper = Command::new("sleep")
        .arg("30")
        .current_dir(&project)
        .spawn()
        .unwrap();
    std::fs::remove_dir(&project).unwrap();
    let (json, code) = run_json(&["ports", "--in", project.to_str().unwrap(), "--json"]);
    let _ = sleeper.kill();
    let _ = sleeper.wait();
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["count"], 0, "{json}");
}

/// `--in` a file that exists says it is not a directory, not that it is missing
#[test]
fn in_a_file_is_not_a_directory() {
    let (_tmp, dir) = fresh_dir();
    let file = dir.join("file.txt");
    std::fs::write(&file, "x").unwrap();
    let (json, code) = run_json(&["list", "--in", file.to_str().unwrap(), "--json"]);
    assert_eq!(code, 4, "{json}");
    assert!(
        json["error"].as_str().unwrap().contains("Not a directory"),
        "{json}"
    );
}

/// `--by` and `--in` next to a PID are guards. When they rule the process out,
/// the error says which guard did: the same exit 2 as a PID that is gone used
/// to read the same way, so an agent took its own mistyped guard for a
/// recycled PID and left the process running
#[test]
fn a_guard_that_rules_out_the_target_names_itself() {
    let (_tmp, dir) = fresh_dir();
    let owned = Owned::start_in(&dir);
    let pid = owned.pid();
    let other = dir.join("elsewhere");
    std::fs::create_dir(&other).unwrap();
    let other = other.to_str().unwrap();

    let mut commands = vec![
        vec!["kill", &pid, "--dry-run"],
        vec!["stop", &pid, "--dry-run"],
        vec!["wait", &pid, "--timeout", "1"],
        vec!["info", &pid],
        vec!["unstick", &pid, "--dry-run"],
    ];
    // not supported on Windows, which they say first
    if cfg!(unix) {
        commands.push(vec!["freeze", &pid, "--dry-run"]);
        commands.push(vec!["thaw", &pid, "--dry-run"]);
    }
    for command in commands {
        for (guard, named) in [
            (
                vec!["--by", "no-such-name"],
                "ruled out by --by no-such-name",
            ),
            (vec!["--in", other], "ruled out by --in"),
        ] {
            let mut args = command.clone();
            args.extend(guard);
            args.push("--json");
            let (json, code) = run_json(&args);
            assert_eq!(code, 2, "{args:?}: {json}");
            let error = json["error"].as_str().unwrap();
            assert!(error.contains(named), "{args:?}: {json}");
        }
    }

    // a PID that is gone still reads as plain not found
    let (json, code) = run_json(&["stop", "99999999", "--by", "no-such-name", "--json"]);
    assert_eq!(code, 2, "{json}");
    assert!(
        !json["error"].as_str().unwrap().contains("ruled out"),
        "{json}"
    );

    // and nothing above touched the process
    let (json, code) = run_json(&["info", &pid, "--json"]);
    assert_eq!(code, 0, "{json}");
}

/// `--start-time` acts on a PID only while it belongs to the process that
/// started then: the OS gives a PID to a new process once the old one exits,
/// and the new one can have the same name and directory as the old
#[test]
fn start_time_guards_one_pid_exactly() {
    let (_tmp, dir) = fresh_dir();
    let owned = Owned::start_in(&dir);
    let pid = owned.pid();
    let (json, _) = run_json(&["info", &pid, "--json"]);
    let started = json["processes"][0]["start_time"].as_u64().unwrap();
    let at = |offset: i64| (started as i64 + offset).to_string();

    // the start time it has, or a second off it: the preview lists it
    for seen in [at(0), at(1), at(-1)] {
        let (json, code) = run_json(&["kill", &pid, "--start-time", &seen, "--dry-run", "--json"]);
        assert_eq!(code, 0, "{seen}: {json}");
        assert_eq!(listed_pids(&json), vec![u64::from(owned.0.id())], "{json}");
    }

    // another start time: ruled out, and only that guard is named
    let wrong = at(-60);
    let (json, code) = run_json(&[
        "kill",
        &pid,
        "--by",
        "listener",
        "--start-time",
        &wrong,
        "--dry-run",
        "--json",
    ]);
    assert_eq!(code, 2, "{json}");
    let error = json["error"].as_str().unwrap();
    assert!(
        error.contains(&format!("(ruled out by --start-time {wrong})")),
        "{json}"
    );

    // it describes one process: one PID target, nothing else
    let mine = std::process::id().to_string();
    for args in [
        vec![
            "kill",
            &pid,
            &mine,
            "--start-time",
            "1",
            "--dry-run",
            "--json",
        ],
        vec![
            "kill",
            "listener",
            "--start-time",
            "1",
            "--dry-run",
            "--json",
        ],
        vec!["kill", ":1", "--start-time", "1", "--dry-run", "--json"],
        vec!["unstick", "--start-time", "1", "--dry-run", "--json"],
    ] {
        let (json, code) = run_json(&args);
        assert_eq!(code, 4, "{args:?}: {json}");
        assert_eq!(json["reason"], "invalid_input", "{args:?}: {json}");
    }

    // nothing above touched the process
    let (json, code) = run_json(&["info", &pid, "--start-time", &at(0), "--json"]);
    assert_eq!(code, 0, "{json}");
}
