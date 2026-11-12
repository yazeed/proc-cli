//! A name target never finds the shell that ran proc.
//!
//! `sh -c "proc by <name>"` has the name in its own command line, and a shell
//! started under that name has it as its name too, so a lookup that did not
//! skip proc's ancestors would list the shell, and `kill` would take it
//! down. Unix only: the test needs a shell it can start under a name of its
//! own.
#![cfg(unix)]

mod built;

use assert_cmd::cargo::CommandCargoExt;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A name no other process has, for this call alone (the tests run at the same
/// time). Ends in `x`, so no name is the start of another.
fn unique_name() -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("pca{}n{n}x", std::process::id())
}

/// What proc did, run from inside a named shell
struct Inside {
    /// The shell's PID: an ancestor of the proc it ran
    shell: u32,
    /// What proc printed on stdout, after the shell's PID line
    out: String,
    /// What proc printed on stderr
    err: String,
    code: i32,
}

/// Run `proc <args>` from inside a shell that is called `name` (its first
/// argument, so it matches the name exactly) and has `name` in its command
/// line, working in `dir`. The shell prints its PID first, which is split off.
fn from_inside_a_shell(name: &str, dir: &std::path::Path, args: &[&str]) -> Inside {
    let proc_path = Command::cargo_bin("proc").unwrap();
    let script = format!(
        "echo $$; '{}' {}; code=$?; exit $code",
        proc_path.get_program().to_string_lossy(),
        args.join(" ")
    );
    let output = Command::new("sh")
        .arg0(name)
        .arg("-c")
        .arg(script)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let (shell, out) = stdout.split_once('\n').expect("the shell's PID line");
    Inside {
        shell: shell.trim().parse().expect("a PID"),
        out: out.to_string(),
        err: String::from_utf8(output.stderr).unwrap(),
        code: output.status.code().unwrap(),
    }
}

/// `target/debug/examples/listener`
fn listener_path() -> std::path::PathBuf {
    built::example("listener")
}

/// A copy of the listener example running as `name` in `dir`: a process that
/// has the shell's name but doesn't run proc. Killed and reaped on drop
struct Namesake(std::process::Child);

impl Namesake {
    fn start(name: &str, dir: &std::path::Path) -> Self {
        use std::io::BufRead;
        let copy = dir.join(name);
        std::fs::copy(listener_path(), &copy).unwrap();
        // a file just written can be "busy" (ETXTBSY) for an instant: a fork
        // in a parallel test still holds the write handle until it execs.
        // That clears in milliseconds, so try again, as cmd_ports does
        let mut attempts = 0;
        let mut child = loop {
            let spawned = Command::new(&copy)
                .stdout(std::process::Stdio::piped())
                .stdin(std::process::Stdio::null())
                .spawn();
            match spawned {
                Ok(child) => break child,
                Err(e) if attempts < 100 && e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                    attempts += 1;
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(e) => panic!("spawn the listener copy: {e}"),
            }
        };
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert!(line.starts_with("PORT "), "{line:?}");
        Namesake(child)
    }
}

impl Drop for Namesake {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not one JSON document ({e}): {text}"))
}

#[test]
fn a_command_that_acts_leaves_the_shell_out_and_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let name = unique_name();
    for args in [
        vec!["kill", &name, "--dry-run", "--json"],
        vec!["stop", &name, "--dry-run", "--json"],
        vec!["freeze", &name, "--dry-run", "--json"],
        vec!["thaw", &name, "--dry-run", "--json"],
        vec!["unstick", &name, "--dry-run", "--json"],
        vec!["kill", &name, "--cmd", "--dry-run", "--json"],
    ] {
        let ran = from_inside_a_shell(&name, dir.path(), &args);
        let doc = json(&ran.out);
        assert_eq!(ran.code, 2, "{args:?}: {doc}");
        assert_eq!(doc["reason"], "ruled_out", "{args:?}: {doc}");
        // the processes it left out are data too, as `ruled_out_by` is
        assert_eq!(doc["left_out"][0]["pid"], ran.shell, "{args:?}: {doc}");
        // the shell is named as the OS names it (`bash` for macOS's sh, `sh`
        // or `dash` elsewhere), whatever argv[0] it was started under
        let error = doc["error"].as_str().unwrap();
        assert!(
            error.ends_with(&format!(
                " (PID {}) matches '{name}', but proc runs under it, so it's left out",
                ran.shell
            )),
            "{args:?}: {error}"
        );
    }
    // human: the same message, and no process listed
    let ran = from_inside_a_shell(&name, dir.path(), &["kill", &name, "--dry-run"]);
    assert_eq!(ran.code, 2, "{}{}", ran.out, ran.err);
    assert!(
        ran.err
            .contains(&format!(" (PID {}) matches '{name}'", ran.shell)),
        "{}{}",
        ran.out,
        ran.err
    );
}

#[test]
fn wait_never_waits_on_the_shell_proc_runs_in() {
    let dir = tempfile::tempdir().unwrap();
    let name = unique_name();
    let started = std::time::Instant::now();
    let ran = from_inside_a_shell(
        &name,
        dir.path(),
        &["wait", &name, "--timeout", "5", "--json"],
    );
    // `--timeout 5` bounds a broken wait; a right one returns at once
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "{}",
        ran.out
    );
    let doc = json(&ran.out);
    assert_eq!(ran.code, 2, "{doc}");
    assert_eq!(doc["reason"], "ruled_out", "{doc}");
}

#[test]
fn beside_another_match_the_shell_is_left_out_and_named() {
    let dir = tempfile::tempdir().unwrap();
    let name = unique_name();
    let namesake = Namesake::start(&name, dir.path());
    let ran = from_inside_a_shell(&name, dir.path(), &["kill", &name, "--dry-run", "--json"]);
    let doc = json(&ran.out);
    assert_eq!(ran.code, 0, "{doc}");
    let pids: Vec<u64> = doc["processes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["pid"].as_u64().unwrap())
        .collect();
    assert_eq!(pids, vec![u64::from(namesake.0.id())], "{doc}");
    let left_out = doc["left_out"]
        .as_array()
        .unwrap_or_else(|| panic!("{doc}"));
    assert_eq!(left_out.len(), 1, "{doc}");
    assert_eq!(left_out[0]["pid"], ran.shell, "{doc}");
    let shell_name = left_out[0]["name"].as_str().unwrap();
    let ran = from_inside_a_shell(&name, dir.path(), &["kill", &name, "--dry-run"]);
    assert_eq!(ran.code, 0, "{}{}", ran.out, ran.err);
    assert!(
        ran.out.contains(&format!(
            "⚠ Left out: {shell_name} (PID {}), which runs this proc",
            ran.shell
        )),
        "{}",
        ran.out
    );
}

/// The object in `doc`, at any depth, that describes process `pid`
fn object_for(doc: &serde_json::Value, pid: u32) -> Option<&serde_json::Value> {
    match doc {
        serde_json::Value::Object(map) => {
            if map.get("pid").and_then(serde_json::Value::as_u64) == Some(u64::from(pid))
                && map.contains_key("name")
            {
                return Some(doc);
            }
            map.values().find_map(|value| object_for(value, pid))
        }
        serde_json::Value::Array(items) => items.iter().find_map(|value| object_for(value, pid)),
        _ => None,
    }
}

#[test]
fn a_command_that_reads_lists_the_shell_and_marks_it() {
    let dir = tempfile::tempdir().unwrap();
    let name = unique_name();
    for args in [
        vec!["by", &name, "--json"],
        vec!["list", &name, "--json"],
        vec!["in", ".", "--by", &name, "--json"],
        vec!["info", &name, "--json"],
        vec!["why", &name, "--json"],
        vec!["on", &name, "--json"],
        vec!["tree", &name, "--json"],
    ] {
        let ran = from_inside_a_shell(&name, dir.path(), &args);
        let doc = json(&ran.out);
        assert_eq!(ran.code, 0, "{args:?}: {doc}");
        let shell = object_for(&doc, ran.shell).unwrap_or_else(|| panic!("{args:?}: {doc}"));
        assert_eq!(shell["runs_proc"], true, "{args:?}: {doc}");
        assert!(doc.get("left_out").is_none(), "{args:?}: {doc}");
    }
    // human: the list, then the note
    let ran = from_inside_a_shell(&name, dir.path(), &["by", &name]);
    assert_eq!(ran.code, 0, "{}{}", ran.out, ran.err);
    let last = ran.out.trim_end().lines().last().unwrap();
    assert!(
        last.starts_with("ℹ Includes ")
            && last.ends_with(&format!(" (PID {}), which runs this proc", ran.shell)),
        "{}",
        ran.out
    );
}

#[test]
fn a_command_line_search_leaves_the_shell_out_and_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let name = unique_name();
    for args in [
        vec!["by", &name, "--cmd", "--json"],
        vec!["in", ".", "--by", &name, "--cmd", "--json"],
        vec!["info", &name, "--cmd", "--json"],
        vec!["why", &name, "--cmd", "--json"],
        vec!["on", &name, "--cmd", "--json"],
        vec!["tree", &name, "--cmd", "--json"],
    ] {
        let ran = from_inside_a_shell(&name, dir.path(), &args);
        let doc = json(&ran.out);
        assert_eq!(ran.code, 2, "{args:?}: {doc}");
        assert_eq!(doc["reason"], "ruled_out", "{args:?}: {doc}");
        // the shell is in `left_out` (where a document has it), and in no result
        let mut results = doc.clone();
        if let Some(map) = results.as_object_mut() {
            map.remove("left_out");
        }
        assert!(object_for(&results, ran.shell).is_none(), "{args:?}: {doc}");
    }
    // `list` lists what it finds, and says what it left out
    let ran = from_inside_a_shell(&name, dir.path(), &["list", &name, "--cmd", "--json"]);
    let doc = json(&ran.out);
    assert_eq!(ran.code, 0, "{doc}");
    assert_eq!(doc["left_out"][0]["pid"], ran.shell, "{doc}");
    let ran = from_inside_a_shell(&name, dir.path(), &["list", &name, "--cmd"]);
    assert!(
        ran.out
            .contains(&format!(" (PID {}), which runs this proc", ran.shell))
            && ran.out.contains("⚠ Left out: "),
        "{}",
        ran.out
    );
}

#[test]
fn every_listing_marks_the_processes_proc_runs_under() {
    let dir = tempfile::tempdir().unwrap();
    let name = unique_name();
    // a full list: the shell is there, marked, and nothing chose it by name, so
    // there is no note; a process proc doesn't run under has no `runs_proc`
    let ran = from_inside_a_shell(&name, dir.path(), &["list", "--json"]);
    let doc = json(&ran.out);
    let shell = object_for(&doc, ran.shell).unwrap_or_else(|| panic!("{doc}"));
    assert_eq!(shell["runs_proc"], true, "{doc}");
    let unmarked = doc["processes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p.get("runs_proc").is_none())
        .count();
    assert!(unmarked > 0, "{doc}");
    let ran = from_inside_a_shell(&name, dir.path(), &["list"]);
    assert!(!ran.out.contains("ℹ Includes"), "{}", ran.out);

    // `for .`: the shell works in this directory
    let ran = from_inside_a_shell(&name, dir.path(), &["for", ".", "--json"]);
    let doc = json(&ran.out);
    let shell = object_for(&doc, ran.shell).unwrap_or_else(|| panic!("{doc}"));
    assert_eq!(shell["runs_proc"], true, "{doc}");
}

#[test]
fn watch_lists_the_shell_its_name_matches_and_marks_it() {
    let dir = tempfile::tempdir().unwrap();
    let name = unique_name();
    // a snapshot (no terminal): the shell, then the note
    let ran = from_inside_a_shell(&name, dir.path(), &["watch", &name]);
    assert_eq!(ran.code, 0, "{}{}", ran.out, ran.err);
    assert!(ran.out.contains(&ran.shell.to_string()), "{}", ran.out);
    let last = ran.out.trim_end().lines().last().unwrap();
    assert!(last.starts_with("ℹ Includes "), "{}", ran.out);
    // one JSON frame
    let ran = from_inside_a_shell(
        &name,
        dir.path(),
        &["watch", &name, "--json", "|", "head", "-n1"],
    );
    let frame = json(ran.out.lines().next().unwrap());
    let shell = object_for(&frame, ran.shell).unwrap_or_else(|| panic!("{frame}"));
    assert_eq!(shell["runs_proc"], true, "{frame}");
    // with --cmd it is left out, and said, in the snapshot and in every frame
    let ran = from_inside_a_shell(&name, dir.path(), &["watch", &name, "--cmd"]);
    assert!(
        ran.out.contains("⚠ Left out: ")
            && ran
                .out
                .contains(&format!(" (PID {}), which runs this proc", ran.shell)),
        "{}",
        ran.out
    );
    let ran = from_inside_a_shell(
        &name,
        dir.path(),
        &["watch", &name, "--cmd", "--json", "|", "head", "-n1"],
    );
    let frame = json(ran.out.lines().next().unwrap());
    assert_eq!(frame["left_out"][0]["pid"], ran.shell, "{frame}");
}

#[test]
fn watch_cmd_beside_another_match_leaves_the_shell_out_of_every_frame() {
    let dir = tempfile::tempdir().unwrap();
    let name = unique_name();
    let other = Namesake::start(&name, dir.path());
    let ran = from_inside_a_shell(
        &name,
        dir.path(),
        &["watch", &name, "--cmd", "--json", "|", "head", "-n1"],
    );
    let frame = json(ran.out.lines().next().unwrap());
    assert_eq!(frame["left_out"][0]["pid"], ran.shell, "{frame}");
    let listed: Vec<u64> = frame["processes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["pid"].as_u64().unwrap())
        .collect();
    assert!(listed.contains(&u64::from(other.0.id())), "{frame}");
    assert!(!listed.contains(&u64::from(ran.shell)), "{frame}");
}

#[test]
fn a_command_line_search_never_counts_procs_own_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let name = unique_name();
    // the shell's command line holds the pattern only because it ran proc
    let pattern = format!("zq{name}");
    let ran = from_inside_a_shell(&name, dir.path(), &["by", &pattern, "--cmd", "--json"]);
    let doc = json(&ran.out);
    assert_eq!(ran.code, 2, "{doc}");
    assert_eq!(doc["reason"], "not_found", "{doc}");
    assert!(doc.get("left_out").is_none(), "{doc}");
    let ran = from_inside_a_shell(&name, dir.path(), &["by", &pattern, "--cmd"]);
    assert!(!ran.out.contains("Left out"), "{}", ran.out);
}

#[test]
fn several_targets_still_say_what_they_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let name = unique_name();
    // PID 1 is found; the name matched only the shell, left out with --cmd
    for args in [
        vec!["why", &name, "1", "--cmd", "--json"],
        vec!["on", &name, "1", "--cmd", "--json"],
        vec!["info", &name, "1", "--cmd", "--json"],
    ] {
        let ran = from_inside_a_shell(&name, dir.path(), &args);
        let doc = json(&ran.out);
        assert_eq!(ran.code, 0, "{args:?}: {doc}");
        assert_eq!(doc["left_out"][0]["pid"], ran.shell, "{args:?}: {doc}");
    }
    // `kill <name> <nothing>`: the error quotes only the name that matched the shell
    let ghost = format!("{name}ghost");
    let ran = from_inside_a_shell(
        &name,
        dir.path(),
        &["kill", &name, &ghost, "--dry-run", "--json"],
    );
    let doc = json(&ran.out);
    let error = doc["error"].as_str().unwrap();
    assert!(error.contains(&format!("matches '{name}',")), "{error}");
}

#[test]
fn people_are_told_what_was_kept_and_what_was_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let name = unique_name();
    // a process with the shell's name that doesn't run proc: every command
    // below finds it, so each prints its whole answer around the line
    let _namesake = Namesake::start(&name, dir.path());
    let kept = |out: &str, shell: u32| {
        out.lines().any(|line| {
            line.starts_with("ℹ Includes ")
                && line.ends_with(&format!(" (PID {shell}), which runs this proc"))
        })
    };
    let left = |out: &str, shell: u32| {
        out.lines().any(|line| {
            line.starts_with("⚠ Left out: ")
                && line.ends_with(&format!(" (PID {shell}), which runs this proc"))
        })
    };
    for args in [
        vec!["by", &name],
        vec!["info", &name],
        vec!["on", &name],
        vec!["why", &name],
        vec!["tree", &name],
        vec!["tree", &name, "--ancestors"],
        vec!["list", &name],
        vec!["in", ".", "--by", &name],
    ] {
        let ran = from_inside_a_shell(&name, dir.path(), &args);
        assert_eq!(ran.code, 0, "{args:?}: {}{}", ran.out, ran.err);
        assert!(kept(&ran.out, ran.shell), "{args:?}: {}", ran.out);
    }
    for args in [
        vec!["by", &name, "--cmd"],
        vec!["info", &name, "--cmd"],
        vec!["on", &name, "--cmd"],
        vec!["why", &name, "--cmd"],
        vec!["tree", &name, "--cmd"],
        vec!["tree", &name, "--ancestors", "--cmd"],
        vec!["in", ".", "--by", &name, "--cmd"],
    ] {
        let ran = from_inside_a_shell(&name, dir.path(), &args);
        assert_eq!(ran.code, 0, "{args:?}: {}{}", ran.out, ran.err);
        assert!(left(&ran.out, ran.shell), "{args:?}: {}", ran.out);
    }
    // in JSON, beside the process it found
    let ran = from_inside_a_shell(&name, dir.path(), &["info", &name, "--cmd", "--json"]);
    let doc = json(&ran.out);
    assert_eq!(doc["left_out"][0]["pid"], ran.shell, "{doc}");
    assert_eq!(doc["count"], 1, "{doc}");
}
