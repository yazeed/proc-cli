//! `list`, `in`, `tree` and the shared `--in` filter, driven through the real binary.
//!
//! Every process these tests look for is one the test started: a copy of the
//! listener example (`examples/listener.rs`) under a name and path of its own,
//! in a fresh temp directory, so `--by <name>` and `--in <dir>` match only it.
//! Each process is killed and reaped on drop.

mod built;
mod contract;

use assert_cmd::Command;
use proc_cli::core::filters::strip_verbatim_prefix;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
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

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A process name no process has, for this call alone: the other tests run
/// proc at the same time, so a name two tests share would match the other
/// test's process (with `--cmd`, any process whose command line has it).
fn nobody() -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    // Ends in `x`, so no name is the start of another (`-1x` in `-10x`)
    format!("nobody-{}-{n}x", std::process::id())
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
    /// A name no other process has, short enough for any OS to report whole
    name: String,
    port: u16,
    /// Keeps the directory alive
    _tmp: tempfile::TempDir,
    /// The directory as processes report it: symlinks resolved, no Windows
    /// `\\?\` prefix
    dir: PathBuf,
    /// The directory exactly as `tempfile` named it: on macOS `/var/folders/..`
    /// (reported by processes as `/private/var/..`), on Windows possibly an
    /// 8.3 short name. This is what a user would type.
    raw_dir: PathBuf,
}

impl Owned {
    fn listener() -> Self {
        Self::listener_with_args(&[])
    }

    /// A listener started with extra arguments (it ignores them, so they only
    /// put words in its command line)
    fn listener_with_args(args: &[&str]) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let raw_dir = tmp.path().to_path_buf();
        let dir = raw_dir.canonicalize().unwrap();
        let dir = PathBuf::from(strip_verbatim_prefix(&dir.to_string_lossy()));
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let name = format!("pcl{}n{n}x", std::process::id());
        let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        std::fs::copy(listener_path(), raw_dir.join(&file)).expect("copy the listener example");
        // Started by its canonical path, so the executable path processes
        // report is that path on every OS; the cwd is the path as the user
        // would type it
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
            name,
            port,
            _tmp: tmp,
            dir,
            raw_dir,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn dir(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }

    /// The file name of the executable copy
    fn file_name(&self) -> String {
        format!("{}{}", self.name, std::env::consts::EXE_SUFFIX)
    }

    /// What proc itself reports for this process (cwd, exe_path), for a
    /// failure message
    fn what_proc_saw(&self) -> String {
        let out = proc_cmd()
            .args(["info", &self.pid().to_string(), "--json"])
            .output()
            .unwrap();
        format!(
            "proc info {} --json (raw dir {}, canonical dir {}):\n{}{}",
            self.pid(),
            self.raw_dir.display(),
            self.dir.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    }

    /// The path of the executable copy
    fn exe(&self) -> PathBuf {
        self.dir
            .join(format!("{}{}", self.name, std::env::consts::EXE_SUFFIX))
    }

    fn raw_dir(&self) -> String {
        self.raw_dir.to_string_lossy().into_owned()
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Whether the `processes` array of a JSON answer holds this PID
fn lists(json: &Value, pid: u32) -> bool {
    json["processes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["pid"].as_u64() == Some(u64::from(pid)))
}

// --- `--in` takes the path the way a user types it ---------------------------

#[test]
fn list_in_finds_a_process_by_the_uncanonicalized_temp_path() {
    // On macOS the temp dir is `/var/folders/..` but processes report
    // `/private/var/folders/..`; on Windows it can be `C:\Users\RUNNER~1\..`
    let owned = Owned::listener();
    let (json, code) = run_json(&["list", "--in", &owned.raw_dir(), "--json"]);
    assert_eq!(code, 0);
    assert!(
        lists(&json, owned.pid()),
        "{json}\n{}",
        owned.what_proc_saw()
    );
}

#[test]
fn in_finds_a_process_by_the_uncanonicalized_temp_path() {
    let owned = Owned::listener();
    let (json, code) = run_json(&["in", &owned.raw_dir(), "--json"]);
    assert_eq!(code, 0);
    assert!(
        lists(&json, owned.pid()),
        "{json}\n{}",
        owned.what_proc_saw()
    );
}

#[test]
fn by_in_finds_a_process_by_the_uncanonicalized_temp_path() {
    let owned = Owned::listener();
    let (json, code) = run_json(&["by", &owned.name, "--in", &owned.raw_dir(), "--json"]);
    assert_eq!(code, 0);
    assert!(
        lists(&json, owned.pid()),
        "{json}\n{}",
        owned.what_proc_saw()
    );
}

#[test]
fn tree_in_finds_a_process_by_the_uncanonicalized_temp_path() {
    let owned = Owned::listener();
    let (json, code) = run_json(&["tree", &owned.name, "--in", &owned.raw_dir(), "--json"]);
    assert_eq!(code, 0);
    let roots = json["tree"].as_array().unwrap();
    assert!(
        roots
            .iter()
            .any(|n| n["pid"].as_u64() == Some(u64::from(owned.pid()))),
        "{json}\n{}",
        owned.what_proc_saw()
    );
}

#[cfg(unix)]
#[test]
fn list_in_follows_a_symlink_to_the_directory() {
    let owned = Owned::listener();
    let links = tempfile::tempdir().unwrap();
    let link = links.path().join("via-link");
    std::os::unix::fs::symlink(&owned.raw_dir, &link).unwrap();
    let (json, code) = run_json(&["list", "--in", link.to_str().unwrap(), "--json"]);
    assert_eq!(code, 0);
    assert!(
        lists(&json, owned.pid()),
        "{json}\n{}",
        owned.what_proc_saw()
    );
}

#[cfg(unix)]
#[test]
fn in_follows_a_symlink_to_the_directory() {
    let owned = Owned::listener();
    let links = tempfile::tempdir().unwrap();
    let link = links.path().join("via-link");
    std::os::unix::fs::symlink(&owned.raw_dir, &link).unwrap();
    let (json, code) = run_json(&["in", link.to_str().unwrap(), "--json"]);
    assert_eq!(code, 0);
    assert!(
        lists(&json, owned.pid()),
        "{json}\n{}",
        owned.what_proc_saw()
    );
}

// --- `tree NAME` does not match the shell that launched it --------------------

#[cfg(unix)]
#[test]
fn tree_by_name_with_cmd_skips_the_shell_whose_command_line_holds_the_name() {
    let owned = Owned::listener();
    let proc_exe = assert_cmd::cargo::cargo_bin("proc");
    // The `exit` keeps sh alive as proc's parent (a bare `sh -c cmd` may exec
    // cmd in place); sh's own command line holds the name, which only --cmd
    // looks at
    let output = std::process::Command::new("sh")
        .args(["-c", r#""$0" "$@"; exit $?"#])
        .arg(&proc_exe)
        .args(["tree", &owned.name, "--cmd", "--json", "--depth", "0"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let pids: Vec<u64> = json["tree"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["pid"].as_u64().unwrap())
        .collect();
    assert_eq!(pids, vec![u64::from(owned.pid())], "{json}");
}

// --- list --------------------------------------------------------------------

/// The PIDs in the `processes` array of an answer
fn pids(json: &Value) -> Vec<u64> {
    json["processes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["pid"].as_u64().unwrap())
        .collect()
}

#[test]
fn list_in_prints_a_table_and_says_which_directory_it_looked_in() {
    let owned = Owned::listener();
    let (out, _, code) = run_text(&["list", "--in", &owned.raw_dir(), "--wide"]);
    assert_eq!(code, 0);
    assert!(
        out.contains(&format!("Found 1 process in {}", owned.dir())),
        "{out}\n{}",
        owned.what_proc_saw()
    );
    assert!(out.contains(&owned.name), "{out}");
    assert!(out.contains(&owned.pid().to_string()), "{out}");
}

#[test]
fn list_a_comma_separated_name_list_finds_each_name_once() {
    let first = Owned::listener();
    let second = Owned::listener();
    // The same name twice, and a name nothing has: each process shows once
    let names = format!("{0},{1},{0},{2}", first.name, second.name, nobody());
    let (json, code) = run_json(&["list", &names, "--json", "--sort", "pid"]);
    assert_eq!(code, 0);
    let mut expected = vec![u64::from(first.pid()), u64::from(second.pid())];
    expected.sort_unstable();
    assert_eq!(pids(&json), expected, "{json}");
}

#[test]
fn list_name_is_the_whole_name_and_cmd_widens_it_to_the_command_line() {
    let word = nobody();
    let owned = Owned::listener_with_args(&[&word]);
    let part = &owned.name[..owned.name.len() - 1];

    // the whole name, in any case
    let (json, _) = run_json(&["list", &owned.name.to_uppercase(), "--json"]);
    assert_eq!(pids(&json), [u64::from(owned.pid())], "{json}");

    // a part of the name, or a word of the command line, lists nothing
    for text in [part, word.as_str()] {
        let (json, code) = run_json(&["list", text, "--json"]);
        assert_eq!(code, 0, "{json}");
        assert!(pids(&json).is_empty(), "{text}: {json}");
    }

    // with --cmd both list it
    for text in [part, word.as_str()] {
        let (json, _) = run_json(&["list", text, "--cmd", "--json"]);
        assert!(
            pids(&json).contains(&u64::from(owned.pid())),
            "{text}: {json}"
        );
    }
}

#[test]
fn list_names_can_be_separate_arguments() {
    let first = Owned::listener();
    let second = Owned::listener();
    // the same name twice, a name nothing has, and a comma list among them:
    // each process shows once
    let list = format!("{},{}", second.name, first.name);
    let nobody = nobody();
    let (json, code) = run_json(&[
        "list",
        &first.name,
        &second.name,
        &nobody,
        &list,
        "--json",
        "--sort",
        "pid",
    ]);
    assert_eq!(code, 0);
    let mut expected = vec![u64::from(first.pid()), u64::from(second.pid())];
    expected.sort_unstable();
    assert_eq!(pids(&json), expected, "{json}");

    // one name of the two, as before
    let (json, _) = run_json(&["list", &first.name, &nobody, "--json"]);
    assert_eq!(pids(&json), [u64::from(first.pid())], "{json}");
}

#[test]
fn list_path_keeps_the_processes_running_that_executable() {
    let owned = Owned::listener();
    let (json, code) = run_json(&["list", "--path", &owned.exe().to_string_lossy(), "--json"]);
    assert_eq!(code, 0);
    assert_eq!(
        pids(&json),
        vec![u64::from(owned.pid())],
        "{json}\n{}",
        owned.what_proc_saw()
    );
}

#[test]
fn list_path_is_taken_from_the_current_directory_when_relative() {
    let owned = Owned::listener();
    let output = proc_cmd()
        .current_dir(&owned.dir)
        .args(["list", "--path", &owned.file_name(), "--json"])
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        pids(&json),
        vec![u64::from(owned.pid())],
        "{json}\n{}",
        owned.what_proc_saw()
    );
}

#[test]
fn list_path_finds_a_process_by_the_uncanonicalized_temp_path() {
    // Same as `--in`: the executable path processes report has symlinks
    // resolved, the path the user types need not
    let owned = Owned::listener();
    let typed = owned.raw_dir.join(owned.file_name());
    let (json, code) = run_json(&["list", "--path", &typed.to_string_lossy(), "--json"]);
    assert_eq!(code, 0);
    assert_eq!(
        pids(&json),
        vec![u64::from(owned.pid())],
        "{json}\n{}",
        owned.what_proc_saw()
    );
}

#[test]
fn list_in_dot_is_the_current_directory() {
    let owned = Owned::listener();
    let output = proc_cmd()
        .current_dir(&owned.dir)
        .args(["list", "--in", "--json"])
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        lists(&json, owned.pid()),
        "{json}\n{}",
        owned.what_proc_saw()
    );
}

#[test]
fn list_a_filter_nothing_passes_says_so() {
    let owned = Owned::listener();
    let (out, _, code) = run_text(&["list", "--in", &owned.dir(), "--min-cpu", "100000"]);
    assert_eq!(code, 0);
    assert!(
        out.contains(&format!("No processes found in {}", owned.dir())),
        "{out}"
    );
}

// --- in ----------------------------------------------------------------------

#[test]
fn in_prints_a_table_and_says_which_directory_it_looked_in() {
    let owned = Owned::listener();
    let (out, _, code) = run_text(&["in", &owned.raw_dir(), "--wide"]);
    assert_eq!(code, 0);
    assert!(
        out.contains(&format!("Found 1 process in {}", owned.dir())),
        "{out}\n{}",
        owned.what_proc_saw()
    );
    assert!(out.contains(&owned.name), "{out}");
}

#[test]
fn in_dot_is_the_current_directory() {
    let owned = Owned::listener();
    let output = proc_cmd()
        .current_dir(&owned.dir)
        .args(["in", ".", "--json"])
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        pids(&json),
        vec![u64::from(owned.pid())],
        "{json}\n{}",
        owned.what_proc_saw()
    );
}

#[test]
fn in_by_keeps_only_that_name() {
    let owned = Owned::listener();
    let (json, _) = run_json(&["in", &owned.dir(), "--by", &owned.name, "--json"]);
    assert_eq!(
        pids(&json),
        vec![u64::from(owned.pid())],
        "{json}\n{}",
        owned.what_proc_saw()
    );
    // A name nothing has is not found, like `by`
    let (json, code) = run_json(&["in", &owned.dir(), "--by", &nobody(), "--json"]);
    assert_eq!(code, 2);
    assert_eq!(json["success"], false, "{json}");
}

#[test]
fn in_filters_drop_what_does_not_match() {
    let owned = Owned::listener();
    let dir = owned.dir();
    for args in [
        vec!["--min-cpu", "100000"],
        vec!["--min-mem", "100000000"],
        vec!["--status", "zombie"],
        vec!["--min-uptime", "100000"],
        vec!["--parent", "1"],
    ] {
        let mut all = vec!["in", dir.as_str(), "--json"];
        all.extend(args.iter());
        let (json, code) = run_json(&all);
        assert_eq!(code, 0);
        assert!(pids(&json).is_empty(), "{args:?} kept it: {json}");
    }
    // And the same filters keep it when it does match
    let (json, _) = run_json(&[
        "in",
        &dir,
        "--min-uptime",
        "0",
        "--min-mem",
        "0",
        "--parent",
        &std::process::id().to_string(),
        "--limit",
        "5",
        "--json",
    ]);
    assert_eq!(
        pids(&json),
        vec![u64::from(owned.pid())],
        "{json}\n{}",
        owned.what_proc_saw()
    );
}

#[test]
fn in_a_missing_directory_is_invalid_input() {
    let (_, stderr, code) = run_text(&["in", "/no/such/dir/for/proc-tests"]);
    assert_eq!(code, 4);
    assert!(stderr.contains("Directory not found"), "{stderr}");
}

#[test]
fn in_tilde_is_the_home_directory() {
    let owned = Owned::listener();
    let output = proc_cmd()
        .env("HOME", &owned.dir)
        .args(["in", "~", "--json"])
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        lists(&json, owned.pid()),
        "{json}\n{}",
        owned.what_proc_saw()
    );
}

#[test]
fn in_tilde_slash_is_taken_from_the_home_directory() {
    let owned = Owned::listener();
    let home = owned.dir.parent().unwrap();
    let rest = owned.dir.file_name().unwrap().to_string_lossy();
    let output = proc_cmd()
        .env("HOME", home)
        .args(["in", &format!("~/{rest}"), "--json"])
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        lists(&json, owned.pid()),
        "{json}\n{}",
        owned.what_proc_saw()
    );
}

#[test]
fn in_tilde_with_no_home_is_a_path_called_tilde() {
    for path in ["~", "~/projects"] {
        let output = proc_cmd()
            .env_remove("HOME")
            .args(["in", path])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(4), "{path}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("Directory not found"), "{stderr}");
    }
}

// --- tree --------------------------------------------------------------------
//
// The listener is a child of this test process, so `tree <this pid>` has a
// branch to show whatever else the machine is doing.

fn me() -> String {
    std::process::id().to_string()
}

/// The node with this PID anywhere under `nodes`
fn find_node(nodes: &[Value], pid: u32) -> Option<&Value> {
    for node in nodes {
        if node["pid"].as_u64() == Some(u64::from(pid)) {
            return Some(node);
        }
        if let Some(found) = find_node(node["children"].as_array().unwrap(), pid) {
            return Some(found);
        }
    }
    None
}

#[test]
fn tree_pid_json_shows_the_process_with_its_children() {
    let owned = Owned::listener();
    let (json, code) = run_json(&["tree", &me(), "--json"]);
    assert_eq!(code, 0);
    assert_eq!(json["action"], "tree");
    assert_eq!(json["success"], true);
    let roots = json["tree"].as_array().unwrap();
    assert_eq!(roots.len(), 1, "{json}");
    assert_eq!(
        roots[0]["pid"].as_u64(),
        Some(u64::from(std::process::id()))
    );
    let child = find_node(roots[0]["children"].as_array().unwrap(), owned.pid())
        .unwrap_or_else(|| panic!("the listener is not under this process: {json}"));
    // the process name is the executable's file name: `.exe` on Windows
    assert_eq!(child["name"], owned.file_name().as_str());
    assert!(child["children"].as_array().unwrap().is_empty());
    assert!(child["memory_mb"].as_f64().unwrap() > 0.0);
    assert!(child["status"].is_string());
}

#[test]
fn tree_pid_human_draws_the_branch() {
    let owned = Owned::listener();
    let (out, _, code) = run_text(&["tree", &me()]);
    assert_eq!(code, 0);
    assert!(
        out.starts_with(&format!("✓ Process tree for '{}':", me())),
        "{out}"
    );
    let line = out
        .lines()
        .find(|l| l.contains(&format!("[{}]", owned.pid())))
        .unwrap_or_else(|| panic!("no line for the listener:\n{out}"));
    assert!(line.contains(&owned.name), "{line}");
    assert!(line.contains("└── ") || line.contains("├── "), "{line}");
}

#[test]
fn tree_compact_prints_only_pids() {
    let owned = Owned::listener();
    let (out, _, code) = run_text(&["tree", &me(), "--compact"]);
    assert_eq!(code, 0);
    let pid = owned.pid().to_string();
    let line = out
        .lines()
        .find(|l| l.trim_start_matches(['│', ' ']).ends_with(&pid))
        .unwrap_or_else(|| panic!("no pid-only line for the listener:\n{out}"));
    assert!(!line.contains(&owned.name), "{line}");
    assert!(!out.contains('['), "{out}");
}

#[test]
fn tree_depth_zero_leaves_out_the_children() {
    let owned = Owned::listener();
    let (out, _, _) = run_text(&["tree", &me(), "--depth", "0"]);
    assert!(out.contains(&format!("[{}]", std::process::id())), "{out}");
    assert!(!out.contains(&owned.name), "{out}");

    let (json, _) = run_json(&["tree", &me(), "--depth", "0", "--json"]);
    let root = &json["tree"][0];
    assert!(root["children"].as_array().unwrap().is_empty(), "{json}");
}

#[test]
fn tree_depth_one_shows_children_but_not_grandchildren() {
    let _owned = Owned::listener();
    let (json, _) = run_json(&["tree", &me(), "--depth", "1", "--json"]);
    let children = json["tree"][0]["children"].as_array().unwrap();
    assert!(!children.is_empty(), "{json}");
    for child in children {
        assert!(child["children"].as_array().unwrap().is_empty(), "{json}");
    }
}

#[test]
fn tree_a_missing_pid_is_not_found() {
    let (out, stderr, code) = run_text(&["tree", "4294967290"]);
    assert_eq!(code, 2);
    assert!(
        format!("{out}{stderr}").contains("No process found matching '4294967290'"),
        "{out}{stderr}"
    );
}

#[test]
fn tree_a_port_shows_the_process_listening_on_it() {
    let owned = Owned::listener();
    let (json, code) = run_json(&["tree", &format!(":{}", owned.port), "--json"]);
    assert_eq!(code, 0);
    let roots = json["tree"].as_array().unwrap();
    assert_eq!(roots.len(), 1, "{json}");
    assert_eq!(roots[0]["pid"].as_u64(), Some(u64::from(owned.pid())));
}

#[test]
fn tree_a_name_shows_every_process_with_that_name() {
    let owned = Owned::listener();
    let (json, code) = run_json(&["tree", &owned.name, "--json"]);
    assert_eq!(code, 0);
    let roots = json["tree"].as_array().unwrap();
    assert_eq!(roots.len(), 1, "{json}");
    assert_eq!(roots[0]["pid"].as_u64(), Some(u64::from(owned.pid())));

    let (out, _, _) = run_text(&["tree", &owned.name]);
    assert!(
        out.contains(&format!("Process tree for '{}':", owned.name)),
        "{out}"
    );
    assert!(out.contains(&format!("[{}]", owned.pid())), "{out}");
}

#[test]
fn tree_by_and_in_narrow_a_target() {
    let owned = Owned::listener();
    let pid = owned.pid().to_string();
    let (json, _) = run_json(&[
        "tree",
        &pid,
        "--by",
        &owned.name,
        "--in",
        &owned.dir(),
        "--json",
    ]);
    assert_eq!(json["tree"].as_array().unwrap().len(), 1, "{json}");

    // Wrong name, or a directory it is not in: the guard rules the PID out,
    // exit 2 as in `info` and `on`, not an empty tree that looks like success
    let (json, code) = run_json(&["tree", &pid, "--by", &nobody(), "--json"]);
    assert_eq!(code, 2, "{json}");
    let elsewhere = tempfile::tempdir().unwrap();
    let (json, code) = run_json(&[
        "tree",
        &pid,
        "--in",
        elsewhere.path().to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(code, 2, "{json}");
    assert_eq!(json["reason"], "ruled_out", "{json}");
    assert_eq!(json["ruled_out_by"], serde_json::json!(["--in"]), "{json}");
}

#[test]
fn tree_a_target_every_filter_drops_says_so() {
    let owned = Owned::listener();
    let pid = owned.pid().to_string();
    for args in [
        vec!["--min-cpu", "100000"],
        vec!["--min-mem", "100000000"],
        vec!["--status", "zombie"],
        vec!["--min-uptime", "100000"],
    ] {
        let mut all = vec!["tree", pid.as_str()];
        all.extend(args.iter());
        let (out, _, code) = run_text(&all);
        assert_eq!(code, 0);
        assert_eq!(
            out.trim(),
            format!("⚠ No processes for '{pid}' match the specified filters"),
            "{args:?}"
        );
    }
    let (json, _) = run_json(&["tree", &pid, "--min-cpu", "100000", "--json"]);
    assert!(json["tree"].as_array().unwrap().is_empty(), "{json}");
}

#[test]
fn tree_a_target_the_filters_keep_is_shown() {
    let owned = Owned::listener();
    let pid = owned.pid().to_string();
    let (out, _, _) = run_text(&["tree", &pid, "--min-uptime", "0", "--min-mem", "0"]);
    assert!(out.contains(&format!("[{pid}]")), "{out}");
}

/// Whether any level of the tree has two nodes side by side, which is when a
/// branch (`├── `) is drawn. A machine with one process chain (a build
/// sandbox, a container) has none.
fn has_siblings(nodes: &[serde_json::Value]) -> bool {
    nodes.len() > 1
        || nodes
            .iter()
            .any(|n| has_siblings(n["children"].as_array().unwrap()))
}

#[test]
fn tree_with_no_target_draws_the_whole_machine() {
    let _owned = Owned::listener();
    let (json, code) = run_json(&["tree", "--depth", "2", "--json"]);
    assert_eq!(code, 0);
    let roots = json["tree"].as_array().unwrap();
    assert!(!roots.is_empty(), "{json}");
    assert!(roots
        .iter()
        .all(|n| n["pid"].is_u64() && n["children"].is_array()));

    let (out, _, code) = run_text(&["tree", "--depth", "2"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("✓ Process tree:"), "{out}");
    assert!(out.contains("└── "), "{out}");
    if has_siblings(roots) {
        assert!(out.contains("├── "), "{out}");
    }
}

#[test]
fn tree_filters_without_a_target_list_the_processes_that_pass() {
    let owned = Owned::listener();
    let (out, _, code) = run_text(&[
        "tree",
        "--min-uptime",
        "0",
        "--min-mem",
        "0",
        "--depth",
        "0",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("matching filters:"), "{out}");
    assert!(out.contains(&format!("[{}]", owned.pid())), "{out}");

    let (json, _) = run_json(&["tree", "--min-uptime", "0", "--depth", "0", "--json"]);
    assert!(
        json["tree"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["pid"].as_u64() == Some(u64::from(owned.pid()))),
        "{json}"
    );
}

#[test]
fn tree_filters_without_a_target_that_nothing_passes_say_so() {
    let (out, _, code) = run_text(&["tree", "--min-cpu", "100000"]);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "⚠ No processes match the specified filters");
}

#[test]
fn tree_ancestors_shows_the_path_up_to_the_root() {
    let owned = Owned::listener();
    let (out, _, code) = run_text(&["tree", &owned.pid().to_string(), "--ancestors"]);
    assert_eq!(code, 0);
    assert!(
        out.starts_with(&format!("✓ Ancestry for '{}':", owned.pid())),
        "{out}"
    );
    let lines: Vec<&str> = out.lines().filter(|l| l.contains('[')).collect();
    let last = lines.last().unwrap();
    assert!(
        last.contains(&owned.name) && last.contains("← target"),
        "{last}"
    );
    // The line before is this test process, one level up
    let parent = lines[lines.len() - 2];
    assert!(
        parent.contains(&format!("[{}]", std::process::id())),
        "{parent}"
    );
    assert!(!parent.contains("← target"), "{parent}");
}

#[test]
fn tree_ancestors_json_lists_the_chain_root_first() {
    let owned = Owned::listener();
    let (json, code) = run_json(&["tree", &owned.pid().to_string(), "--ancestors", "--json"]);
    assert_eq!(code, 0);
    let node = &json["ancestry"][0];
    assert_eq!(node["target_pid"].as_u64(), Some(u64::from(owned.pid())));
    assert_eq!(node["target_name"], owned.file_name().as_str());
    let chain = node["chain"].as_array().unwrap();
    assert_eq!(node["depth"].as_u64(), Some(chain.len() as u64));
    assert!(chain.len() >= 2, "{json}");
    let last = chain.last().unwrap();
    assert_eq!(last["pid"].as_u64(), Some(u64::from(owned.pid())));
    let parent = &chain[chain.len() - 2];
    assert_eq!(parent["pid"].as_u64(), Some(u64::from(std::process::id())));
}

#[test]
fn tree_ancestors_takes_a_name() {
    let owned = Owned::listener();
    let (out, _, code) = run_text(&["tree", &owned.name, "--ancestors"]);
    assert_eq!(code, 0);
    assert!(out.contains("← target"), "{out}");
    let (json, _) = run_json(&["tree", &owned.name, "--ancestors", "--json"]);
    assert_eq!(json["ancestry"].as_array().unwrap().len(), 1, "{json}");
}

#[test]
fn tree_ancestors_of_a_name_nothing_has_is_not_found() {
    let name = nobody();
    let (out, err, code) = run_text(&["tree", &name, "--ancestors"]);
    assert_eq!(code, 2, "{out}{err}");
    assert!(
        err.contains(&format!("No process found matching '{name}'")),
        "{err}"
    );
}

#[test]
fn tree_name_is_the_whole_name_and_cmd_widens_it_to_the_command_line() {
    let word = nobody();
    let owned = Owned::listener_with_args(&[&word]);
    let hint = format!("1 process has '{word}' in its command line (add --cmd to match those)");

    // only in the command line: not found by default (exit 2), and the error
    // says where the name is
    for args in [vec!["tree", &word], vec!["tree", &word, "--ancestors"]] {
        let (out, err, code) = run_text(&args);
        assert_eq!(code, 2, "{args:?}: {out}{err}");
        assert!(
            err.contains(&format!("No process found matching '{word}'")),
            "{args:?}: {err}"
        );
        assert!(err.contains(&hint), "{args:?}: {err}");
    }
    let (json, code) = run_json(&["tree", &word, "--json"]);
    assert_eq!(code, 2, "{json}");

    // with --cmd it is the match, for the tree and for the ancestry
    let (json, _) = run_json(&["tree", &word, "--cmd", "--json", "--depth", "0"]);
    assert_eq!(json["tree"][0]["pid"], owned.pid(), "{json}");
    let (json, _) = run_json(&["tree", &word, "--cmd", "--ancestors", "--json"]);
    assert_eq!(json["ancestry"][0]["target_pid"], owned.pid(), "{json}");

    // part of the name is not the name either
    let part = &owned.name[..owned.name.len() - 1];
    let (json, code) = run_json(&["tree", part, "--json", "--depth", "0"]);
    assert_eq!(code, 2, "{json}");
    let (json, _) = run_json(&["tree", part, "--cmd", "--json", "--depth", "0"]);
    assert!(
        json["tree"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["pid"] == owned.pid()),
        "{json}"
    );
}

#[test]
fn tree_ancestors_without_a_target_asks_for_one() {
    let (out, err, code) = run_text(&["tree", "--ancestors"]);
    assert_eq!(code, 4, "{out}{err}");
    assert!(
        err.contains("--ancestors needs a target (PID, :port, or name)"),
        "{err}"
    );
}

#[cfg(unix)]
#[test]
fn tree_ancestors_by_name_with_cmd_skips_the_shell_whose_command_line_holds_the_name() {
    let owned = Owned::listener();
    let proc_exe = assert_cmd::cargo::cargo_bin("proc");
    let output = std::process::Command::new("sh")
        .args(["-c", r#""$0" "$@"; exit $?"#])
        .arg(&proc_exe)
        .args(["tree", &owned.name, "--cmd", "--ancestors", "--json"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let targets: Vec<u64> = json["ancestry"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["target_pid"].as_u64().unwrap())
        .collect();
    assert_eq!(targets, vec![u64::from(owned.pid())], "{json}");
}
