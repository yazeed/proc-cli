//! The flag matrix: a table of cases checked against the clap definitions.
//!
//! `every_flag_is_exercised` walks `Cli::command()` and fails for any
//! `command --flag` or `command -f` that no case uses, so a new flag cannot
//! ship untested.
//! `flag_matrix_cases_behave` runs every case against the real binary.
//! `destructive_cases_only_target_what_tests_own` keeps the table safe to run
//! on a machine with real work on it.
//!
//! Placeholders in a case's `args`, filled in fresh for every run (a kill case
//! consumes its target):
//! - `{pid}`: a process the test owns (the listener example)
//! - `{port}`: the port that same process listens on
//! - `{dir}`: a fresh temp directory, which is also the owned process's cwd
//! - `{exe}`: the owned process's executable (the listener example)
//! - `{name}`: the owned process's name, for name targets and `--by`
//! - `{part}`: the start of that name, which matches only with `--cmd`

mod built;
mod contract;

use assert_cmd::Command;
use clap::CommandFactory;
use proc_cli::cli::Cli;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::Duration;

/// What a case must do
#[derive(Debug)]
enum Expect {
    /// Exit with this code and print one JSON document
    Code(i32),
    /// Exit codes that differ by OS (freeze, thaw and orphans are unsupported
    /// on Windows, as is `stop --signal`)
    PerOs { unix: i32, windows: i32 },
    /// A streaming command: the first stdout line is a JSON document whose
    /// `action` is the command, and then the child is killed
    FirstJsonLine,
}

/// One row of the matrix: `proc <cmd> <args...>`, with `--json` added unless
/// the case already has it (and never for `watch`, which would stream forever)
#[derive(Debug)]
struct Case {
    cmd: &'static str,
    args: &'static [&'static str],
    expect: Expect,
}

const fn code(cmd: &'static str, args: &'static [&'static str], exit: i32) -> Case {
    Case {
        cmd,
        args,
        expect: Expect::Code(exit),
    }
}

const fn per_os(cmd: &'static str, args: &'static [&'static str], unix: i32, windows: i32) -> Case {
    Case {
        cmd,
        args,
        expect: Expect::PerOs { unix, windows },
    }
}

const fn first_line(cmd: &'static str, args: &'static [&'static str]) -> Case {
    Case {
        cmd,
        args,
        expect: Expect::FirstJsonLine,
    }
}

/// Commands that change processes
const DESTRUCTIVE: &[&str] = &[
    "kill", "stop", "free", "freeze", "thaw", "unstick", "orphans", "stuck",
];

/// Not process commands: they print shell completions or the man page
const NOT_PROCESS_COMMANDS: &[&str] = &["completions", "manpage"];

/// A PID no machine has: aiming at it can only report "not found"
const MISSING_PID: &str = "99999999";

/// Commands that change processes. They ask before acting, so they take
/// --yes/-y and --dry-run (`orphans` and `stuck` only change them with --kill)
const CHANGES_PROCESSES: &[&str] = DESTRUCTIVE;

/// The matrix. One row per way of using a flag; each flag of each command has
/// at least one, and the shorter spellings (-j, -y, ...) are used where cheap.
#[rustfmt::skip]
const CASES: &[Case] = &[
    // ---- on ----
    code("on", &["{pid}"], 0),
    code("on", &[":{port}"], 0),
    code("on", &["{name}", "--json"], 0),
    code("on", &["{pid}", "-j"], 0),
    code("on", &["{pid}", "--verbose"], 0),
    code("on", &[":{port}", "-v"], 0),
    code("on", &[":{port}", "--in", "{dir}"], 0),
    code("on", &["{pid}", "-i", "{dir}"], 0),
    code("on", &["{pid}", "--by", "{name}"], 0),
    code("on", &["{pid}", "-b", "{name}"], 0),
    code("on", &["{pid}", "--by", "no-such-process-name"], 2),
    code("on", &["{part}", "--cmd"], 0),
    code("on", &["{part}"], 2),
    code("on", &["{pid}", "--by", "{part}", "--cmd"], 0),
    code("on", &[MISSING_PID], 2),
    code("on", &["{pid}", ":{port}"], 0),
    code("on", &["{pid}", ":{port},{name}", MISSING_PID], 0),
    // ---- for ----
    code("for", &["{exe}"], 0),
    code("for", &["{exe}", "--json"], 0),
    code("for", &["{exe}", "-j", "-v"], 0),
    code("for", &["{exe}", "--verbose", "--wide"], 0),
    code("for", &["{exe}", "-w"], 0),
    // a file has nothing below it: the same answer as without the flag
    code("for", &["{exe}", "--recursive"], 0),
    code("for", &["{exe}", "-r", "--json"], 0),
    code("for", &["{exe}", "--in", "{dir}"], 0),
    code("for", &["{exe}", "-i", "{dir}", "-b", "{name}"], 0),
    code("for", &["{exe}", "--by", "{name}"], 0),
    code("for", &["{exe}", "--by", "{part}", "--cmd"], 0),
    code("for", &["{exe}", "--min-cpu", "0", "--min-mem", "0"], 0),
    code("for", &["{exe}", "--min-uptime", "0"], 0),
    // scoped to this case's directory: the cases run on several threads, and
    // another case's listener, killed and not yet reaped, is a zombie of the
    // same executable
    code("for", &["{exe}", "--status", "zombie", "--in", "{dir}"], 2),
    code("for", &["{exe}", "--status", "idle", "--in", "{dir}"], 2),
    code("for", &["{exe}", "--status", "starting", "--in", "{dir}"], 2),
    code("for", &["{exe}", "--status", "disk-wait", "--in", "{dir}"], 2),
    code("for", &["{exe}", "--sort", "mem", "--limit", "5"], 0),
    code("for", &["{exe}", "-s", "pid", "-n", "5"], 0),
    code("for", &["{exe}", "--sort", "name"], 0),
    code("for", &["{exe}", "--sort", "cpu"], 0),
    code("for", &["/nonexistent/proc-test-file"], 4),
    // ---- by ----
    code("by", &["{name}"], 0),
    code("by", &["{name}", "--json"], 0),
    code("by", &["{part}", "--cmd"], 0),
    code("by", &["{part}"], 2),
    code("by", &["{name}", "-j", "-v"], 0),
    code("by", &["{name}", "--verbose", "--wide"], 0),
    code("by", &["{name}", "-w"], 0),
    code("by", &["{name}", "--in", "{dir}"], 0),
    code("by", &["{name}", "-i", "{dir}"], 0),
    code("by", &["{name}", "--min-cpu", "0", "--min-mem", "0"], 0),
    code("by", &["{name}", "--min-uptime", "0", "--parent", "1"], 0),
    code("by", &["{name}", "--status", "zombie"], 0),
    code("by", &["{name}", "--status", "idle"], 0),
    code("by", &["{name}", "--status", "starting"], 0),
    code("by", &["{name}", "--status", "disk-wait"], 0),
    code("by", &["{name}", "--limit", "1", "--sort", "mem"], 0),
    code("by", &["{name}", "-n", "1", "-s", "pid"], 0),
    code("by", &["{name}", "--sort", "name"], 0),
    code("by", &["{name}", "--sort", "cpu"], 0),
    // ---- in ----
    code("in", &["{dir}"], 0),
    code("in", &["{dir}", "--json"], 0),
    code("in", &["{dir}", "-j", "-v"], 0),
    code("in", &["{dir}", "--verbose", "--wide"], 0),
    code("in", &["{dir}", "-w"], 0),
    code("in", &["{dir}", "--by", "{name}"], 0),
    code("in", &["{dir}", "--by", "{part}", "--cmd"], 0),
    code("in", &["{dir}", "-b", "{name}"], 0),
    code("in", &["{dir}", "--min-cpu", "0", "--min-mem", "0"], 0),
    code("in", &["{dir}", "--min-uptime", "0", "--parent", "1"], 0),
    code("in", &["{dir}", "--status", "zombie"], 0),
    code("in", &["{dir}", "--status", "idle"], 0),
    code("in", &["{dir}", "--status", "starting"], 0),
    code("in", &["{dir}", "--status", "disk-wait"], 0),
    code("in", &["{dir}", "--limit", "1", "--sort", "mem"], 0),
    code("in", &["{dir}", "-n", "1", "-s", "pid"], 0),
    code("in", &["{dir}", "--sort", "name"], 0),
    code("in", &["{dir}", "--sort", "cpu"], 0),
    code("in", &["/nonexistent/proc-test-dir"], 4),
    // ---- list ----
    code("list", &[], 0),
    code("list", &["{name}", "--json"], 0),
    code("list", &["{part}", "--cmd"], 0),
    code("list", &["{name}", "no-such-process-name"], 0),
    code("list", &["{name},no-such-process-name", "{name}"], 0),
    code("list", &["-j", "-v"], 0),
    code("list", &["--verbose", "--wide"], 0),
    code("list", &["-w"], 0),
    code("list", &["--in", "{dir}"], 0),
    code("list", &["-i", "{dir}"], 0),
    code("list", &["--in"], 0),
    code("list", &["--path", "{exe}"], 0),
    code("list", &["-p", "{exe}"], 0),
    code("list", &["--min-cpu", "0", "--min-mem", "0"], 0),
    code("list", &["--min-uptime", "0", "--parent", "1"], 0),
    code("list", &["--status", "zombie"], 0),
    code("list", &["--status", "idle"], 0),
    code("list", &["--status", "starting"], 0),
    code("list", &["--status", "disk-wait"], 0),
    code("list", &["--status", "disk_wait"], 0),
    code("list", &["--limit", "1", "--sort", "mem"], 0),
    code("list", &["-n", "1", "-s", "pid"], 0),
    code("list", &["--sort", "name"], 0),
    code("list", &["--sort", "cpu"], 0),
    code("list", &["--status", "no-such-status"], 4),
    // ---- info ----
    code("info", &["{pid}"], 0),
    code("info", &["{pid}", "--json"], 0),
    code("info", &[":{port}", "-j", "-v"], 0),
    code("info", &["{pid}", "--verbose"], 0),
    code("info", &["{pid}", ":{port}"], 0),
    code("info", &["{pid}", ":{port},{name}", MISSING_PID], 0),
    code("info", &["{pid}", "--in", "{dir}"], 0),
    code("info", &["{pid}", "-i", "{dir}"], 0),
    code("info", &["{pid}", "--by", "{name}"], 0),
    code("info", &["{pid}", "-b", "{name}"], 0),
    code("info", &["{part}", "--cmd"], 0),
    code("info", &["{part}"], 2),
    code("info", &[MISSING_PID], 2),
    // a start time no running process has: the guard rules it out
    code("info", &["{pid}", "--start-time", "1"], 2),
    // ---- ports ----
    code("ports", &[], 0),
    code("ports", &["--json"], 0),
    code("ports", &["-j", "-v"], 0),
    code("ports", &["--verbose"], 0),
    code("ports", &["--by", "{name}"], 0),
    code("ports", &["-b", "{name}"], 0),
    code("ports", &["--by", "{part}", "--cmd"], 0),
    code("ports", &["--in", "{dir}"], 0),
    code("ports", &["-i", "{dir}"], 0),
    code("ports", &["--exposed"], 0),
    code("ports", &["-e"], 0),
    code("ports", &["--local"], 0),
    code("ports", &["-l"], 0),
    code("ports", &["--sort", "port", "--limit", "3"], 0),
    code("ports", &["-s", "pid", "-n", "3"], 0),
    code("ports", &["--sort", "name"], 0),
    code("ports", &["--range", "1-65535"], 0),
    code("ports", &["-r", "1024-65535"], 0),
    // ---- tree ----
    code("tree", &[], 0),
    code("tree", &["{pid}"], 0),
    code("tree", &["{pid}", "--json"], 0),
    code("tree", &["{pid}", "-j", "-v"], 0),
    code("tree", &["{pid}", "--verbose"], 0),
    code("tree", &["{pid}", "--ancestors"], 0),
    code("tree", &["{pid}", "-a", "-C", "-d", "2"], 0),
    code("tree", &["{pid}", "--compact", "--depth", "1"], 0),
    code("tree", &["{pid}", "--min-cpu", "0", "--min-mem", "0"], 0),
    code("tree", &["{pid}", "--min-uptime", "0", "--status", "zombie"], 0),
    code("tree", &["{pid}", "--min-uptime", "0", "--status", "idle"], 0),
    code("tree", &["{pid}", "--min-uptime", "0", "--status", "starting"], 0),
    code("tree", &["{pid}", "--min-uptime", "0", "--status", "disk-wait"], 0),
    code("tree", &["--in", "{dir}"], 0),
    code("tree", &["-i", "{dir}", "-b", "{name}"], 0),
    code("tree", &["--by", "{name}"], 0),
    code("tree", &["{part}", "--cmd"], 0),
    code("tree", &["--status", "no-such-status"], 4),
    // ---- watch (a table snapshot when stdout isn't a terminal) ----
    code("watch", &["{pid}"], 0),
    code("watch", &["{pid}", ":{port}", "{name}"], 0),
    code("watch", &["{pid},:{port}", "{name}"], 0),
    code("watch", &["{pid}", "--interval", "1", "--limit", "3"], 0),
    code("watch", &["{pid}", "-n", "1", "-l", "3", "-s", "mem", "-w"], 0),
    code("watch", &["{pid}", "--verbose", "--wide"], 0),
    code("watch", &["{pid}", "-v"], 0),
    code("watch", &["--in", "{dir}", "--by", "{name}"], 0),
    code("watch", &["-i", "{dir}", "-b", "{name}"], 0),
    code("watch", &["{name}", "--min-cpu", "0", "--min-mem", "0"], 0),
    code("watch", &["{name}", "--sort", "pid"], 0),
    code("watch", &["{name}", "--sort", "name"], 0),
    code("watch", &["{name}", "--sort", "cpu"], 0),
    code("watch", &["{part}", "--cmd"], 0),
    // ---- watch --json streams: read the first line, then kill it ----
    first_line("watch", &["{pid}", "--json", "--interval", "1"]),
    first_line("watch", &["{pid}", "-j", "-v"]),
    // ---- kill ----
    code("kill", &["{pid}", "--yes"], 0),
    code("kill", &[":{port}", "-y", "-j"], 0),
    code("kill", &["{pid}", ":{port}", "--dry-run"], 0),
    code("kill", &["{pid}", ":{port},{pid}", "--yes"], 0),
    code("kill", &[MISSING_PID, "{pid}", "--yes"], 0),
    code("kill", &["--in", "{dir}", "{pid}", ":{port}", "--yes"], 0),
    code("kill", &["{pid}", MISSING_PID, "--yes", "--in", "{dir}"], 0),
    // on Windows a graceful kill is taskkill without /F, which a console
    // program refuses ("can only be terminated forcefully"): exit 1
    per_os("kill", &["{pid}", "--graceful", "--yes"], 0, 1),
    per_os("kill", &["{pid}", "-g", "-y", "--verbose"], 0, 1),
    code("kill", &["{pid}", "-y", "-v"], 0),
    code("kill", &["{pid}", "--dry-run"], 0),
    code("kill", &["{pid}", "--yes", "--in", "{dir}"], 0),
    code("kill", &["{pid}", "-y", "-i", "{dir}", "-b", "{name}"], 0),
    code("kill", &["{pid}", "--yes", "--by", "{name}"], 0),
    code("kill", &["{part}", "--cmd", "--dry-run"], 0),
    code("kill", &["{pid}", "--yes", "--by", "{part}", "--cmd"], 0),
    code("kill", &["{pid}", "--yes", "--by", "{part}"], 2),
    code("kill", &["{pid}", "--yes", "--by", "no-such-process-name"], 2),
    code("kill", &["{pid}", "--yes", "--start-time", "1"], 2),
    code("kill", &["{pid}", "--json"], 4),
    code("kill", &[MISSING_PID, "--yes"], 2),
    // ---- stop ----
    code("stop", &["{pid}", "--yes"], 0),
    code("stop", &[":{port}", "-y", "-j"], 0),
    code("stop", &["{pid}", ":{port}", "--dry-run"], 0),
    code("stop", &["{pid}", ":{port},{pid}", "--yes"], 0),
    code("stop", &[MISSING_PID, "{pid}", "--yes"], 0),
    code("stop", &["{pid}", "--yes", "--timeout", "2"], 0),
    code("stop", &["{pid}", "-y", "-t", "2", "-v"], 0),
    code("stop", &["{pid}", "--dry-run"], 0),
    code("stop", &["{pid}", "--yes", "--in", "{dir}"], 0),
    code("stop", &["{pid}", "-y", "-i", "{dir}", "-b", "{name}"], 0),
    code("stop", &["{pid}", "--yes", "--by", "{name}", "--verbose"], 0),
    code("stop", &["{part}", "--cmd", "--dry-run"], 0),
    code("stop", &["{pid}", "--yes", "--by", "{part}", "--cmd"], 0),
    code("stop", &["{pid}", "--yes", "--start-time", "1"], 2),
    per_os("stop", &["{pid}", "--yes", "--signal", "TERM"], 0, 4),
    per_os("stop", &["{pid}", "-y", "-S", "HUP"], 0, 4),
    code("stop", &["{pid}", "--yes", "--signal", "NOT-A-SIGNAL"], 4),
    code("stop", &["{pid}", "--json"], 4),
    // ---- free ----
    code("free", &[":{port}", "--yes"], 0),
    code("free", &[":{port}", "-y", "-j"], 0),
    code("free", &[":{port}", ":{port}", "--yes"], 0),
    code("free", &[":{port},:{port}", ":{port}", "--dry-run"], 0),
    code("free", &[":{port}", "--yes", "--wait", "2"], 0),
    code("free", &[":{port}", "--yes", "--verbose"], 0),
    code("free", &[":{port}", "-y", "-v"], 0),
    code("free", &[":{port}", "--dry-run"], 0),
    code("free", &[":{port}", "--json"], 4),
    // ---- freeze (SIGSTOP: unsupported on Windows) ----
    per_os("freeze", &["{pid}", "--yes"], 0, 4),
    per_os("freeze", &[":{port}", "-y", "-j"], 0, 4),
    per_os("freeze", &["{pid}", ":{port}", "--yes"], 0, 4),
    per_os("freeze", &["{pid}", ":{port},{pid}", "--dry-run"], 0, 4),
    per_os("freeze", &["{pid}", "--yes", "--verbose"], 0, 4),
    per_os("freeze", &["{pid}", "-y", "-v"], 0, 4),
    per_os("freeze", &["{pid}", "--dry-run"], 0, 4),
    per_os("freeze", &["{pid}", "--yes", "--in", "{dir}"], 0, 4),
    per_os("freeze", &["{pid}", "-y", "-i", "{dir}", "-b", "{name}"], 0, 4),
    per_os("freeze", &["{pid}", "--yes", "--by", "{name}"], 0, 4),
    per_os("freeze", &["{part}", "--cmd", "--dry-run"], 0, 4),
    per_os("freeze", &["{pid}", "--yes", "--by", "{part}", "--cmd"], 0, 4),
    per_os("freeze", &["{pid}", "--yes", "--start-time", "1"], 2, 4),
    code("freeze", &["{pid}", "--json"], 4),
    // ---- thaw (SIGCONT: unsupported on Windows) ----
    per_os("thaw", &["{pid}", "--yes"], 0, 4),
    per_os("thaw", &[":{port}", "-y", "-j"], 0, 4),
    per_os("thaw", &["{pid}", ":{port}", "--yes"], 0, 4),
    per_os("thaw", &["{pid}", ":{port},{pid}", "--dry-run"], 0, 4),
    per_os("thaw", &["{pid}", "--yes", "--verbose"], 0, 4),
    per_os("thaw", &["{pid}", "-y", "-v"], 0, 4),
    per_os("thaw", &["{pid}", "--dry-run"], 0, 4),
    per_os("thaw", &["{pid}", "--yes", "--in", "{dir}"], 0, 4),
    per_os("thaw", &["{pid}", "-y", "-i", "{dir}", "-b", "{name}"], 0, 4),
    per_os("thaw", &["{pid}", "--yes", "--by", "{name}"], 0, 4),
    per_os("thaw", &["{part}", "--cmd", "--dry-run"], 0, 4),
    per_os("thaw", &["{pid}", "--yes", "--by", "{part}", "--cmd"], 0, 4),
    per_os("thaw", &["{pid}", "--yes", "--start-time", "1"], 2, 4),
    code("thaw", &["{pid}", "--json"], 4),
    // ---- unstick (has a Windows path: it can only terminate, and only with --force) ----
    code("unstick", &["{pid}", "--yes"], 0),
    code("unstick", &["{pid}", "-y", "-f", "-j"], 0),
    code("unstick", &["{pid}", ":{port}", "--dry-run"], 0),
    code("unstick", &["{pid}", ":{port},{pid}", "--yes"], 0),
    code("unstick", &["{pid}", "--yes", "--force", "--verbose"], 0),
    code("unstick", &["{pid}", "--yes", "--timeout", "1"], 0),
    code("unstick", &["{pid}", "--dry-run", "-v"], 0),
    code("unstick", &["--dry-run", "--in", "{dir}", "--by", "{name}"], 0),
    code("unstick", &["{part}", "--cmd", "--dry-run"], 0),
    code("unstick", &["--dry-run", "--in", "{dir}", "--by", "{part}", "--cmd"], 0),
    code("unstick", &["--yes", "--in", "{dir}", "-t", "1"], 0),
    code("unstick", &["--yes", "-i", "{dir}", "-b", "{name}"], 0),
    code("unstick", &["{pid}", "--yes", "--start-time", "1"], 2),
    code("unstick", &["{pid}", "--json"], 4),
    // ---- orphans (unsupported on Windows: nothing re-parents an orphan) ----
    per_os("orphans", &["--in", "{dir}"], 0, 4),
    per_os("orphans", &["--in", "{dir}", "--json"], 0, 4),
    per_os("orphans", &["-i", "{dir}", "-j", "-v"], 0, 4),
    per_os("orphans", &["--in", "{dir}", "--verbose", "--wide"], 0, 4),
    per_os("orphans", &["-i", "{dir}", "-w", "-b", "{name}"], 0, 4),
    per_os("orphans", &["--in", "{dir}", "--by", "{name}"], 0, 4),
    per_os("orphans", &["--in", "{dir}", "--by", "{part}", "--cmd"], 0, 4),
    per_os("orphans", &["--kill", "--yes", "--in", "{dir}"], 0, 4),
    per_os("orphans", &["-k", "-y", "-i", "{dir}"], 0, 4),
    per_os("orphans", &["--kill", "--dry-run", "--in", "{dir}"], 0, 4),
    per_os("orphans", &["--kill", "--in", "{dir}", "--json"], 0, 4),
    // ---- stuck ----
    code("stuck", &["--in", "{dir}"], 0),
    code("stuck", &["--in", "{dir}", "--json"], 0),
    code("stuck", &["-i", "{dir}", "-j", "-v"], 0),
    code("stuck", &["--in", "{dir}", "--verbose", "--wide"], 0),
    code("stuck", &["-i", "{dir}", "-w", "-b", "{name}"], 0),
    code("stuck", &["--in", "{dir}", "--by", "{name}", "--timeout", "1"], 0),
    code("stuck", &["--in", "{dir}", "--by", "{part}", "--cmd"], 0),
    code("stuck", &["-i", "{dir}", "-t", "1"], 0),
    code("stuck", &["--kill", "--yes", "--in", "{dir}"], 0),
    code("stuck", &["-k", "-y", "-i", "{dir}", "-t", "1"], 0),
    code("stuck", &["--kill", "--dry-run", "--in", "{dir}"], 0),
    code("stuck", &["--kill", "--in", "{dir}", "--json"], 0),
    // ---- why ----
    code("why", &["{pid}"], 0),
    code("why", &[":{port}", "--json"], 0),
    code("why", &["{pid}", "-j", "-v"], 0),
    code("why", &["{pid}", "--verbose"], 0),
    code("why", &["{part}", "--cmd"], 0),
    code("why", &["{part}"], 2),
    code("why", &["{pid},:{port}"], 0),
    code("why", &["{pid}", ":{port}"], 0),
    code("why", &["{pid}", ":{port},{name}", MISSING_PID], 0),
    code("why", &[MISSING_PID], 2),
    // ---- wait (the owned process never exits, so each one times out) ----
    code("wait", &["{pid}", "--timeout", "1"], 1),
    code("wait", &["{pid}", "--timeout", "1", "--start-time", "1"], 2),
    code("wait", &[":{port}", "-t", "1", "--json"], 1),
    code("wait", &["{pid}", "-t", "1", "-j"], 1),
    code("wait", &["{pid}", "-t", "1", "--interval", "1", "--quiet"], 1),
    code("wait", &["{pid}", "-t", "1", "-n", "1", "-q", "-v"], 1),
    code("wait", &["{pid}", "-t", "1", "--verbose"], 1),
    code("wait", &["{pid}", "-t", "1", "--in", "{dir}"], 1),
    code("wait", &["{pid}", "-t", "1", "-i", "{dir}", "-b", "{name}"], 1),
    code("wait", &["{pid}", "-t", "1", "--by", "{name}"], 1),
    code("wait", &["{pid}", "-t", "1", "--by", "{part}", "--cmd"], 1),
    code("wait", &[MISSING_PID, "--timeout", "1"], 2),
    code("wait", &["{pid}", ":{port}", "-t", "1"], 1),
    code("wait", &["{pid}", ":{port},{pid}", "-t", "1"], 1),
    code("wait", &[MISSING_PID, "{pid}", "-t", "1"], 1),
];

#[test]
fn every_command_follows_the_flag_conventions() {
    let cli = Cli::command();
    let mut problems = Vec::new();

    for cmd in cli.get_subcommands() {
        let name = cmd.get_name();
        if NOT_PROCESS_COMMANDS.contains(&name) {
            continue;
        }
        let mut need = |long: &str, short: Option<char>| {
            let found = cmd.get_arguments().find(|a| a.get_long() == Some(long));
            match (found, short) {
                (None, _) => problems.push(format!("{name}: no --{long}")),
                (Some(a), Some(s)) if a.get_short() != Some(s) => {
                    problems.push(format!("{name}: --{long} should have -{s}"))
                }
                _ => {}
            }
        };
        need("json", Some('j'));
        need("verbose", Some('v'));
        if CHANGES_PROCESSES.contains(&name) {
            need("yes", Some('y'));
            need("dry-run", None);
        }
    }

    for name in CHANGES_PROCESSES.iter().chain(NOT_PROCESS_COMMANDS) {
        if cli.find_subcommand(name).is_none() {
            problems.push(format!("{name} is listed here but isn't a command"));
        }
    }
    assert!(
        problems.is_empty(),
        "convention broken:\n{}",
        problems.join("\n")
    );
}

/// Does `arg` spell `--long` (alone or as `--long=value`)?
fn spells(arg: &str, long: &str) -> bool {
    arg.strip_prefix("--").is_some_and(|rest| {
        rest == long || rest.strip_prefix(long).is_some_and(|r| r.starts_with('='))
    })
}

/// Does `arg` spell the short flag `-s` (one flag per word, as the cases write them)?
fn spells_short(arg: &str, short: char) -> bool {
    let mut chars = arg.chars();
    chars.next() == Some('-') && chars.next() == Some(short) && chars.next().is_none()
}

#[test]
fn every_flag_is_exercised() {
    let cli = Cli::command();
    let mut missing = Vec::new();

    for sub in cli.get_subcommands() {
        let name = sub.get_name();
        if NOT_PROCESS_COMMANDS.contains(&name) {
            continue;
        }
        let cases = || CASES.iter().filter(|c| c.cmd == name);
        for arg in sub.get_arguments() {
            if let Some(long) = arg.get_long() {
                let used = cases().any(|c| c.args.iter().any(|a| spells(a, long)));
                if long != "help" && long != "version" && !used {
                    missing.push(format!("{name} --{long}"));
                }
            }
            if let Some(short) = arg.get_short() {
                let used = cases().any(|c| c.args.iter().any(|a| spells_short(a, short)));
                if short != 'h' && short != 'V' && !used {
                    missing.push(format!("{name} -{short}"));
                }
            }
        }
    }

    assert!(
        missing.is_empty(),
        "{} flag(s) have no case in CASES:\n{}",
        missing.len(),
        missing.join("\n")
    );
}

/// The positional arguments of a case: every word that is not a flag and not the
/// value of a flag. Which flags take a value comes from the clap definitions, so
/// `--in {dir}` and `--timeout 2` are not mistaken for targets, and a target
/// that follows a flag is still found.
fn positionals(case: &Case) -> Vec<&'static str> {
    let mut cli = Cli::command();
    cli.build();
    let sub = cli
        .find_subcommand(case.cmd)
        .unwrap_or_else(|| panic!("{} is not a command", case.cmd));
    let mut found = Vec::new();
    let mut words = case.args.iter().copied().peekable();
    while let Some(word) = words.next() {
        let def = if let Some(long) = word.strip_prefix("--") {
            if long.contains('=') {
                continue; // `--in={dir}`: the value is attached
            }
            sub.get_arguments().find(|a| a.get_long() == Some(long))
        } else if let Some(short) = word.strip_prefix('-') {
            let mut chars = short.chars();
            let letter = chars.next();
            assert!(
                letter.is_some() && chars.next().is_none(),
                "{word:?}: the safety check reads one short flag per word"
            );
            sub.get_arguments().find(|a| a.get_short() == letter)
        } else {
            found.push(word);
            continue;
        };
        let def = def.unwrap_or_else(|| panic!("{} has no flag {word}", case.cmd));
        if def.get_action().takes_values() {
            let must_have_one = def.get_num_args().is_some_and(|n| n.min_values() > 0);
            // An optional value (`--in`) takes the next word unless it is a flag
            if words
                .peek()
                .is_some_and(|next| must_have_one || !next.starts_with('-'))
            {
                words.next();
            }
        }
    }
    found
}

/// Why a case could touch something the test doesn't own, if it could
fn unsafe_reason(case: &Case) -> Option<String> {
    if !DESTRUCTIVE.contains(&case.cmd) || case.args.contains(&"--dry-run") {
        return None;
    }
    // Every positional is a target, and each comma-separated piece of each must be ours
    let targets = positionals(case);
    if !targets.is_empty() {
        for target in targets {
            let stranger = target
                .split(',')
                .find(|piece| !(*piece == "{pid}" || *piece == ":{port}" || *piece == MISSING_PID));
            if let Some(piece) = stranger {
                return Some(format!(
                    "target {piece:?} (in {target:?}) isn't {{pid}} or :{{port}}"
                ));
            }
        }
        return None;
    }
    // No target: the case must be scoped to the fresh temp directory
    let scoped = case
        .args
        .windows(2)
        .any(|w| (w[0] == "--in" || w[0] == "-i") && w[1] == "{dir}")
        || case.args.contains(&"--in={dir}");
    if scoped {
        None
    } else {
        Some("no target, no --dry-run and no `--in {dir}`".to_string())
    }
}

#[test]
fn destructive_cases_only_target_what_tests_own() {
    let bad: Vec<String> = CASES
        .iter()
        .filter_map(|c| {
            unsafe_reason(c).map(|why| format!("proc {} {}: {why}", c.cmd, c.args.join(" ")))
        })
        .collect();
    assert!(bad.is_empty(), "unsafe cases:\n{}", bad.join("\n"));
}

#[test]
fn the_safety_check_rejects_what_it_should() {
    let bare_name = code("kill", &["node", "--yes"], 0);
    let no_target = code("orphans", &["--kill", "--yes"], 0);
    let wrong_dir = code("stuck", &["--kill", "--in", "/tmp", "--yes"], 0);
    let listed_with_a_stranger = code("kill", &["{pid},node", "--yes"], 0);
    let second_target_a_stranger = code("kill", &["{pid}", "node", "--yes"], 0);
    let third_target_a_stranger = code("stop", &["{pid}", ":{port}", "node", "--yes"], 0);
    let stranger_after_a_flag = code("kill", &["--yes", "{pid}", "node"], 0);
    let stranger_after_a_valued_flag = code("kill", &["--in", "{dir}", "{pid}", "node", "-y"], 0);
    let stranger_in_a_later_comma_list = code("free", &[":{port}", ":{port},:9", "--yes"], 0);
    for case in [
        bare_name,
        no_target,
        wrong_dir,
        listed_with_a_stranger,
        second_target_a_stranger,
        third_target_a_stranger,
        stranger_after_a_flag,
        stranger_after_a_valued_flag,
        stranger_in_a_later_comma_list,
    ] {
        assert!(unsafe_reason(&case).is_some(), "{case:?} should be unsafe");
    }

    let ours = [
        code("kill", &["{pid}", "--yes"], 0),
        code("free", &[":{port}", "--yes"], 0),
        code("kill", &["node", "--dry-run"], 0),
        code("kill", &["{pid}", ":{port}", "--yes"], 0),
        code("kill", &["--yes", "{pid}", ":{port},{pid}"], 0),
        code("kill", &["--in", "{dir}", "{pid}", "-y"], 0),
        code("stop", &["{pid}", "--timeout", "2", "-y"], 0),
        code("stop", &["{pid}", "--yes", "--by", "{name}"], 0),
        code("orphans", &["--kill", "--yes", "--in", "{dir}"], 0),
        code("list", &["--min-cpu", "5"], 0),
    ];
    for case in ours {
        assert_eq!(unsafe_reason(&case), None, "{case:?} should be fine");
    }
}

/// A process the test owns: the listener example, started with its cwd in a
/// fresh temp directory. Killed and reaped on drop, whatever a case did to it.
struct Fixture {
    child: Child,
    port: u16,
    /// Keeps the directory alive; `dir` is its path as processes report it
    _tmp: tempfile::TempDir,
    dir: PathBuf,
}

impl Fixture {
    fn start() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        // A process reports its cwd with symlinks resolved (/var is /private/var
        // on macOS), and `--in` compares paths literally
        #[cfg(unix)]
        let dir = tmp.path().canonicalize().unwrap();
        #[cfg(not(unix))]
        let dir = tmp.path().to_path_buf();
        let mut child = std::process::Command::new(listener_path())
            .current_dir(&dir)
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
        match port {
            Some(port) => Fixture {
                child,
                port,
                _tmp: tmp,
                dir,
            },
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("the listener printed {line:?} instead of `PORT <n>`");
            }
        }
    }

    fn fill(&self, arg: &str) -> String {
        arg.replace("{pid}", &self.child.id().to_string())
            .replace("{port}", &self.port.to_string())
            .replace("{dir}", &self.dir.to_string_lossy())
            .replace("{exe}", &listener_path().to_string_lossy())
            .replace("{name}", "listener")
            .replace("{part}", "listene")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `target/debug/examples/listener[.exe]`
fn listener_path() -> PathBuf {
    built::example("listener")
}

/// The argument list for a case, placeholders filled and `--json` added
fn argv(case: &Case, fixture: Option<&Fixture>) -> Vec<String> {
    let mut argv = vec![case.cmd.to_string()];
    for arg in case.args {
        argv.push(match fixture {
            Some(f) => f.fill(arg),
            None => arg.to_string(),
        });
    }
    let has_json = case
        .args
        .iter()
        .any(|a| *a == "--json" || (a.starts_with('-') && !a.starts_with("--") && a.contains('j')));
    if !has_json && case.cmd != "watch" {
        argv.push("--json".to_string());
    }
    argv
}

fn needs_fixture(case: &Case) -> bool {
    const PLACEHOLDERS: [&str; 6] = ["{pid}", "{port}", "{dir}", "{exe}", "{name}", "{part}"];
    case.args
        .iter()
        .any(|a| PLACEHOLDERS.iter().any(|p| a.contains(p)))
}

fn expected_exit(expect: &Expect) -> Option<i32> {
    match expect {
        Expect::Code(c) => Some(*c),
        Expect::PerOs { unix, windows } => Some(if cfg!(windows) { *windows } else { *unix }),
        Expect::FirstJsonLine => None,
    }
}

/// Run one case; returns a description of what went wrong, if anything
fn run_case(case: &Case) -> Result<(), String> {
    let fixture = needs_fixture(case).then(Fixture::start);
    let argv = argv(case, fixture.as_ref());
    let shown = format!("proc {}", argv.join(" "));

    let Some(want) = expected_exit(&case.expect) else {
        return first_json_line(case, &argv).map_err(|e| format!("{shown}: {e}"));
    };

    let output = Command::cargo_bin("proc")
        .unwrap()
        .args(&argv)
        .timeout(Duration::from_secs(60))
        .output()
        .map_err(|e| format!("{shown}: {e}"))?;
    let got = output.status.code();
    let stdout = String::from_utf8_lossy(&output.stdout);
    if got != Some(want) {
        return Err(format!(
            "{shown}: exit {got:?}, expected {want}\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // `watch` without --json is a table snapshot, not a document
    if case.cmd != "watch" {
        let json: Value = serde_json::from_str(&stdout)
            .map_err(|e| format!("{shown}: stdout is not one JSON document ({e}):\n{stdout}"))?;
        if let Some(wrong) = contract::violation(&json, want) {
            return Err(format!("{shown}: {wrong}\n{json:#}"));
        }
    }
    Ok(())
}

/// Spawn a streaming command, read its first stdout line, then kill it
fn first_json_line(case: &Case, argv: &[String]) -> Result<(), String> {
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("proc"))
        .args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().unwrap();

    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = tx.send(line);
    });
    let line = rx.recv_timeout(Duration::from_secs(30));

    // Stop the child by its own handle whatever happened, then reap it
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();

    let line = line.map_err(|_| "no output within 30s".to_string())?;
    let json: Value = serde_json::from_str(line.trim())
        .map_err(|e| format!("first line is not JSON ({e}): {line:?}"))?;
    if json["action"] != case.cmd {
        return Err(format!("first line has action {}", json["action"]));
    }
    Ok(())
}

/// Cases run on this many threads, each running one `proc` at a time
const WORKERS: usize = 4;

#[test]
fn flag_matrix_cases_behave() {
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::<(usize, String)>::new());

    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(case) = CASES.get(i) else { break };
                if let Err(why) = run_case(case) {
                    failures.lock().unwrap().push((i, why));
                }
            });
        }
    });

    let mut failures = failures.into_inner().unwrap();
    failures.sort();
    let report: Vec<String> = failures.into_iter().map(|(_, why)| why).collect();
    assert!(
        report.is_empty(),
        "{} of {} cases failed:\n\n{}",
        report.len(),
        CASES.len(),
        report.join("\n\n")
    );
}
