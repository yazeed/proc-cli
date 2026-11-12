//! The order proc prints things in, driven through the real binary.
//!
//! Every process whose place in a list these tests check is one the test
//! started: copies of the listener example (`examples/listener.rs`) under a name
//! of their own, in a fresh temp directory. Each copy is killed and reaped on
//! drop. The tests share one lock, so the only children this test process has
//! are the ones the running test started.
//!
//! The rules under test:
//! - a process source lists PID ascending, so the rows of a name, `--limit` and
//!   the preview of a kill are the same on every run
//! - a sort breaks ties by PID
//! - a process's ports are ascending
//! - the list a person approves is in the order typed, then PID ascending
//!   inside each target
//! - `tree` gives one order in its human and JSON forms

mod built;
mod contract;

use assert_cmd::Command;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

fn proc_cmd() -> Command {
    let mut cmd = Command::cargo_bin("proc").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd
}

/// Run proc, expect one JSON document on stdout, and return it with the exit code
fn run_json(args: &[&str]) -> (Value, i32) {
    let output = proc_cmd().args(args).output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout is not a single JSON document ({e}):\n{stdout}"));
    let code = output.status.code().unwrap();
    contract::check(&json, code, args);
    (json, code)
}

/// Run proc and return its stdout and exit code
fn run_text(args: &[&str]) -> (String, i32) {
    let output = proc_cmd().args(args).output().unwrap();
    (
        String::from_utf8(output.stdout).unwrap(),
        output.status.code().unwrap(),
    )
}

static SERIAL: Mutex<()> = Mutex::new(());
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Held for the whole test: one of these tests at a time
fn serial() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn listener_path() -> PathBuf {
    built::example("listener")
}

/// Copies of the listener under one name no other process has
struct Herd {
    children: Vec<Child>,
    /// The name every copy runs under
    name: String,
    /// Per child: the port it printed first, then the `ALSO` ports in the order
    /// it bound them
    ports: Vec<Vec<u16>>,
    _tmp: tempfile::TempDir,
}

impl Herd {
    /// `count` copies of one listener, each holding `1 + also` ports
    fn start(count: usize, also: usize) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let name = format!("pord{}n{n}x", std::process::id());
        let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        let dir = tmp.path().canonicalize().unwrap();
        std::fs::copy(listener_path(), dir.join(&file)).expect("copy the listener example");
        let mut herd = Herd {
            children: Vec::new(),
            name,
            ports: Vec::new(),
            _tmp: tmp,
        };
        for _ in 0..count {
            let mut args = Vec::new();
            if also > 0 {
                args.extend(["--also".to_string(), also.to_string()]);
            }
            let mut child = spawn_with_retry(&dir.join(&file), &args);
            let mut lines = BufReader::new(child.stdout.take().unwrap());
            let mut ports = Vec::new();
            for expected in std::iter::once("PORT").chain(std::iter::repeat_n("ALSO", also)) {
                let mut line = String::new();
                lines.read_line(&mut line).expect("read a port line");
                let port = line
                    .trim()
                    .strip_prefix(expected)
                    .and_then(|p| p.trim().parse().ok())
                    .unwrap_or_else(|| {
                        panic!("the listener printed {line:?}, not `{expected} <n>`")
                    });
                ports.push(port);
            }
            herd.ports.push(ports);
            herd.children.push(child);
        }
        herd
    }

    /// The PIDs, ascending
    fn pids(&self) -> Vec<u32> {
        let mut pids: Vec<u32> = self.children.iter().map(Child::id).collect();
        pids.sort_unstable();
        pids
    }

    fn pid_of(&self, index: usize) -> u32 {
        self.children[index].id()
    }
}

impl Drop for Herd {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Run the copy. On Linux a file another thread has just written can be busy
/// for an instant; try again.
fn spawn_with_retry(copy: &std::path::Path, args: &[String]) -> Child {
    let mut attempts = 0;
    loop {
        match std::process::Command::new(copy)
            .args(args)
            .stdout(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
        {
            Ok(child) => return child,
            Err(_) if attempts < 100 => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => panic!("spawn the listener copy: {e}"),
        }
    }
}

/// The `pid` of every entry of `json[key]`
fn pids_in(json: &Value, key: &str) -> Vec<u32> {
    json[key]
        .as_array()
        .unwrap_or_else(|| panic!("no `{key}` array in {json}"))
        .iter()
        .map(|p| p["pid"].as_u64().unwrap() as u32)
        .collect()
}

fn is_ascending<T: PartialOrd>(values: &[T]) -> bool {
    values.windows(2).all(|w| w[0] <= w[1])
}

// --- a name: PID ascending, the same on every run -----------------------------

#[test]
fn by_a_name_lists_the_pids_ascending_on_every_run() {
    let _lock = serial();
    let herd = Herd::start(4, 0);
    // --sort pid: the default (cpu) is not guaranteed to tie between copies
    let first = run_json(&["by", &herd.name, "--sort", "pid", "--json"]);
    let second = run_json(&["by", &herd.name, "--sort", "pid", "--json"]);
    assert_eq!(first.1, 0);
    assert_eq!(pids_in(&first.0, "processes"), herd.pids());
    assert_eq!(pids_in(&second.0, "processes"), herd.pids());
}

#[test]
fn list_with_a_name_and_a_limit_keeps_the_same_rows() {
    let _lock = serial();
    let herd = Herd::start(5, 0);
    let pids = herd.pids();
    // every row ties on the name, so which two are kept is down to the tie-break
    // (cpu and mem ties are covered by the unit tests: copies need not tie)
    for _ in 0..2 {
        let (json, code) = run_json(&[
            "list", &herd.name, "--sort", "name", "--limit", "2", "--json",
        ]);
        assert_eq!(code, 0);
        assert_eq!(pids_in(&json, "processes"), pids[..2], "--sort name");
    }
}

#[test]
fn by_with_a_limit_picks_the_same_rows_every_run() {
    let _lock = serial();
    let herd = Herd::start(5, 0);
    let pids = herd.pids();
    for _ in 0..2 {
        let (json, _) = run_json(&["by", &herd.name, "--sort", "pid", "--limit", "3", "--json"]);
        assert_eq!(pids_in(&json, "processes"), pids[..3]);
        let (json, _) = run_json(&["by", &herd.name, "--sort", "name", "--limit", "3", "--json"]);
        assert_eq!(pids_in(&json, "processes"), pids[..3]);
    }
}

#[test]
fn every_sort_breaks_its_ties_by_pid_over_all_processes() {
    let _lock = serial();
    // a herd of equal names guarantees ties for the name sort
    let _herd = Herd::start(3, 0);
    for sort in ["cpu", "mem", "name", "pid"] {
        let (json, code) = run_json(&["list", "--sort", sort, "--json"]);
        assert_eq!(code, 0);
        let rows = json["processes"].as_array().unwrap();
        for pair in rows.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let (pa, pb) = (a["pid"].as_u64().unwrap(), b["pid"].as_u64().unwrap());
            let order = match sort {
                "cpu" => b["cpu_percent"]
                    .as_f64()
                    .partial_cmp(&a["cpu_percent"].as_f64()),
                "mem" => b["memory_mb"]
                    .as_f64()
                    .partial_cmp(&a["memory_mb"].as_f64()),
                "name" => Some(
                    a["name"]
                        .as_str()
                        .unwrap()
                        .to_lowercase()
                        .cmp(&b["name"].as_str().unwrap().to_lowercase()),
                ),
                _ => Some(std::cmp::Ordering::Equal),
            };
            match order {
                Some(std::cmp::Ordering::Less) => {}
                Some(std::cmp::Ordering::Equal) => {
                    assert!(pa < pb, "--sort {sort}: tie not by PID: {a} then {b}")
                }
                other => panic!("--sort {sort}: out of order ({other:?}): {a} then {b}"),
            }
        }
    }
}

// --- tree: one order for people and for scripts -------------------------------

/// The PIDs on the lines of a `--compact` tree that sit at `depth` (0 is a root)
fn compact_pids(text: &str, depth: usize) -> Vec<u32> {
    text.lines()
        .filter_map(|line| {
            let stripped = line.trim_start_matches(['│', ' ']);
            let indent = line.chars().count() - stripped.chars().count();
            let pid = stripped
                .strip_prefix("├── ")
                .or_else(|| stripped.strip_prefix("└── "))?;
            (indent == depth * 4).then(|| pid.trim().parse().ok())?
        })
        .collect()
}

#[test]
fn tree_children_are_the_same_in_the_human_form_and_json() {
    let _lock = serial();
    let herd = Herd::start(5, 0);
    let me = std::process::id().to_string();
    for _ in 0..2 {
        let (json, code) = run_json(&["tree", &me, "--depth", "1", "--json"]);
        assert_eq!(code, 0);
        let children: Vec<u32> = json["tree"][0]["children"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["pid"].as_u64().unwrap() as u32)
            .collect();
        let (text, code) = run_text(&["tree", &me, "--depth", "1", "--compact"]);
        assert_eq!(code, 0);
        let human = compact_pids(&text, 1);
        assert!(is_ascending(&children), "json children: {children:?}");
        assert_eq!(children, human, "human tree:\n{text}");
        let ours: Vec<u32> = children
            .iter()
            .copied()
            .filter(|p| herd.pids().contains(p))
            .collect();
        assert_eq!(ours, herd.pids());
    }
}

#[test]
fn tree_roots_for_a_name_are_the_same_in_the_human_form_and_json() {
    let _lock = serial();
    let herd = Herd::start(4, 0);
    for _ in 0..2 {
        let (json, code) = run_json(&["tree", &herd.name, "--json"]);
        assert_eq!(code, 0);
        assert_eq!(pids_in(&json, "tree"), herd.pids());
        let (text, _) = run_text(&["tree", &herd.name, "--compact"]);
        assert_eq!(compact_pids(&text, 0), herd.pids(), "{text}");
    }
}

#[test]
fn tree_ancestors_for_a_name_lists_the_targets_ascending() {
    let _lock = serial();
    let herd = Herd::start(4, 0);
    for _ in 0..2 {
        let (json, code) = run_json(&["tree", &herd.name, "--ancestors", "--json"]);
        assert_eq!(code, 0);
        let targets: Vec<u32> = json["ancestry"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["target_pid"].as_u64().unwrap() as u32)
            .collect();
        assert_eq!(targets, herd.pids());
    }
}

#[test]
fn tree_with_a_resource_filter_lists_the_processes_ascending() {
    let _lock = serial();
    let herd = Herd::start(3, 0);
    // the herd's copies are siblings whose parent (this test) is not a match,
    // so each is a root of its own: a match under another match is drawn in
    // that one's tree, not again as a root
    let (json, code) = run_json(&["tree", "--min-uptime", "0", "--by", &herd.name, "--json"]);
    assert_eq!(code, 0);
    let pids = pids_in(&json, "tree");
    assert_eq!(pids.len(), 3, "{pids:?}");
    assert!(is_ascending(&pids), "{pids:?}");
}

// --- ports: ascending ----------------------------------------------------------

#[test]
fn the_ports_of_a_pid_are_ascending() {
    let _lock = serial();
    let herd = Herd::start(1, 3);
    let mut want = herd.ports[0].clone();
    want.sort_unstable();
    let pid = herd.pid_of(0).to_string();
    for _ in 0..2 {
        let (json, code) = run_json(&["on", &pid, "--json"]);
        assert_eq!(code, 0);
        let got: Vec<u64> = json["ports"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["port"].as_u64().unwrap())
            .collect();
        let want: Vec<u64> = want.iter().map(|&p| u64::from(p)).collect();
        assert_eq!(got, want);
    }
}

#[test]
fn for_and_why_list_a_processs_ports_ascending() {
    let _lock = serial();
    let herd = Herd::start(1, 3);
    let mut want: Vec<u64> = herd.ports[0].iter().map(|&p| u64::from(p)).collect();
    want.sort_unstable();
    let pid = herd.pid_of(0).to_string();

    let (json, code) = run_json(&["why", &pid, "--json"]);
    assert_eq!(code, 0);
    let got: Vec<u64> = json["results"][0]["ports"]
        .as_array()
        .unwrap_or_else(|| panic!("no ports in {json}"))
        .iter()
        .map(|p| p["port"].as_u64().unwrap())
        .collect();
    assert_eq!(got, want, "why");

    let exe = {
        let (json, _) = run_json(&["info", &pid, "--json"]);
        json["processes"][0]["exe_path"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let (json, code) = run_json(&["for", &exe, "--json"]);
    assert_eq!(code, 0);
    let got: Vec<u64> = json["results"][0]["ports"]
        .as_array()
        .unwrap_or_else(|| panic!("no ports in {json}"))
        .iter()
        .map(|p| p["port"].as_u64().unwrap())
        .collect();
    assert_eq!(got, want, "for");
}

#[test]
fn ports_sorted_by_pid_or_name_break_ties_by_port() {
    let _lock = serial();
    let herd = Herd::start(2, 3);
    for sort in ["port", "pid", "name"] {
        let (json, code) = run_json(&["ports", "--sort", sort, "--json"]);
        assert_eq!(code, 0);
        let rows = json["ports"].as_array().unwrap();
        let key = |row: &Value| -> (String, u64, u64) {
            let port = row["port"].as_u64().unwrap();
            let pid = row["pid"].as_u64().unwrap();
            match sort {
                "port" => (String::new(), port, pid),
                "pid" => (String::new(), pid, port),
                _ => (
                    row["process_name"].as_str().unwrap().to_lowercase(),
                    port,
                    pid,
                ),
            }
        };
        let keys: Vec<_> = rows.iter().map(key).collect();
        assert!(is_ascending(&keys), "--sort {sort}: {keys:?}");
        // and our own listeners are in there, 8 ports in all
        let ours = rows
            .iter()
            .filter(|r| herd.pids().contains(&(r["pid"].as_u64().unwrap() as u32)))
            .count();
        assert_eq!(ours, 8, "--sort {sort}");
    }
}

// --- the list a person approves: typed order, then PID ascending ---------------

/// The PIDs a `--dry-run --json` of `command` lists
fn dry_run(command: &str, targets: &[&str]) -> Vec<u32> {
    let mut args = vec![command];
    args.extend(targets);
    args.extend(["--dry-run", "--json"]);
    let (json, code) = run_json(&args);
    assert_eq!(code, 0, "{json}");
    pids_in(&json, "processes")
}

#[test]
fn the_preview_of_an_action_lists_a_name_ascending_on_every_run() {
    let _lock = serial();
    let herd = Herd::start(4, 0);
    let commands: &[&str] = if cfg!(unix) {
        &["kill", "stop", "freeze", "thaw"]
    } else {
        &["kill", "stop"]
    };
    for command in commands {
        for _ in 0..2 {
            assert_eq!(dry_run(command, &[&herd.name]), herd.pids(), "{command}");
        }
    }
}

#[test]
fn the_preview_of_an_action_lists_targets_in_the_order_typed() {
    let _lock = serial();
    let herd = Herd::start(4, 0);
    let other = Herd::start(1, 0);
    let pids = herd.pids();
    let last = pids[3].to_string();
    let other_pid = other.pid_of(0);
    let other_arg = other_pid.to_string();

    // a name, then a PID of another process
    let mut want = pids.clone();
    want.push(other_pid);
    assert_eq!(dry_run("kill", &[&herd.name, &other_arg]), want);

    // the PID first, then the name
    let mut want = vec![other_pid];
    want.extend(&pids);
    assert_eq!(dry_run("kill", &[&other_arg, &herd.name]), want);

    // a member of the name first: it keeps its place, the rest follow ascending
    let mut want = vec![pids[3]];
    want.extend(&pids[..3]);
    assert_eq!(dry_run("stop", &[&last, &herd.name]), want);
}

#[test]
fn the_preview_of_free_lists_the_holders_in_the_order_the_ports_were_typed() {
    let _lock = serial();
    let a = Herd::start(1, 0);
    let b = Herd::start(1, 0);
    let (port_a, port_b) = (format!(":{}", a.ports[0][0]), format!(":{}", b.ports[0][0]));
    assert_eq!(
        dry_run("free", &[&port_a, &port_b]),
        [a.pid_of(0), b.pid_of(0)]
    );
    assert_eq!(
        dry_run("free", &[&port_b, &port_a]),
        [b.pid_of(0), a.pid_of(0)]
    );
}

#[test]
fn the_confirmation_request_lists_the_same_processes_in_the_same_order() {
    let _lock = serial();
    let herd = Herd::start(4, 0);
    let (json, code) = run_json(&["kill", &herd.name, "--json"]);
    // no --yes and no way to ask: exit 4, and the list that would have been killed
    assert_eq!(code, 4, "{json}");
    assert_eq!(json["needs_confirmation"], true);
    assert_eq!(pids_in(&json, "processes"), herd.pids());
    assert_eq!(dry_run("kill", &[&herd.name]), herd.pids());
}

#[test]
fn the_receipt_of_a_kill_lists_what_was_killed_in_the_order_of_the_preview() {
    let _lock = serial();
    let herd = Herd::start(4, 0);
    let pids = herd.pids();
    let first = pids[2].to_string();
    let preview = dry_run("kill", &[&first, &herd.name]);
    let (json, code) = run_json(&["kill", &first, &herd.name, "--yes", "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(pids_in(&json, "succeeded"), preview);
    assert_eq!(preview[0], pids[2]);
    assert!(is_ascending(&preview[1..]));
}

#[test]
fn the_receipt_of_a_stop_lists_what_was_stopped_in_the_order_of_the_preview() {
    let _lock = serial();
    let herd = Herd::start(4, 0);
    let preview = dry_run("stop", &[&herd.name]);
    let (json, code) = run_json(&["stop", &herd.name, "--yes", "--timeout", "1", "--json"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(pids_in(&json, "succeeded"), preview);
    assert_eq!(preview, herd.pids());
}
