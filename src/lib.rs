#![warn(missing_docs)]
//! # proc - Semantic Process Management CLI
//!
//! Semantic CLI tool for process management. Target by port, PID, name or path.
//!
//! ## Features
//!
//! - **Unified Targets**: `:port`, `PID`, and `name` work the same everywhere
//! - **Multi-Target**: `proc kill :3000 :8080 node` - separate arguments (comma-separated targets still work, but are deprecated)
//! - **Query Language**: `proc by node --in .` - composable filters
//! - **Working Directory**: See which project folder a process is running from
//! - **One Process Card**: `proc on :3000`, `proc on 1234`, `proc info`, `proc for` (one hit) and `proc why` show the same card: command, directory, path, user, parent, status, resources, start time, children, listening ports and their connections
//! - **Terminal-Adaptive Tables**: Tables adjust to terminal width automatically
//! - **Consistent Filters**: `--in`, `--by`, `--min-uptime`, `--parent`, `--range`, `--sort`, `--limit`
//! - **PID Guards**: `proc stop 1234 --start-time 1700000000 --yes` - act only while the PID still belongs to the process you saw (with `--in` and `--by` for its directory and name)
//! - **JSON Output**: `--json` on every command except `completions` and `manpage` — one `{action, success, ...}` document per run (`watch` streams one per refresh), and `success: false` always exits non-zero
//! - **Custom Signals**: `proc stop worker --signal INT` - send a different first signal; whatever is still running after `--timeout` is force-killed
//! - **Real-Time Monitoring**: `proc watch` / `proc top` - live process table (TUI on a terminal, NDJSON stream with `--json`)
//! - **Process Waiting**: `proc wait node` - block until process(es) exit (pipe-friendly)
//! - **Freeze/Thaw**: `proc freeze` / `proc thaw` - pause and resume by port, PID, or name
//! - **Port Freeing**: `proc free :3000` - stop what holds the port (SIGTERM, then SIGKILL after up to 5 seconds) and verify it is available
//! - **Ancestry Tracing**: `proc why :3000` - trace why a port is busy
//! - **Orphan Detection**: `proc orphans --in .` - find processes whose parent has exited (candidates: on macOS every app launchd started has parent 1 too)
//! - **File Lookup**: `proc for ./server` - find by file path
//! - **Cross-Platform**: macOS, Linux, Windows, FreeBSD and NetBSD
//! - **Shell Completions**: bash, zsh, fish, elvish, PowerShell via `proc completions`
//! - **Man Pages**: `proc manpage` generates the top-level page (commands and examples; per-command flags are in `proc <command> --help`)
//!
//! ## Quick Start
//!
//! ```bash
//! # What's on port 3000?
//! proc on :3000
//!
//! # What is PID 1234, and what ports is it using?
//! proc on 1234
//!
//! # What has this file open, or runs it as its program?
//! proc for ./server
//!
//! # Kill multiple targets
//! proc kill :3000 :8080 node
//!
//! # Node processes in current directory
//! proc by node --in .
//!
//! # Preview before killing
//! proc kill node --dry-run
//!
//! # Kill only node processes in current directory
//! proc kill node --in .
//!
//! # Watch processes in real-time
//! proc watch node --in .
//!
//! # Wait for a process to finish
//! proc wait node --timeout 3600
//!
//! # Free a port (stop what holds it, then check it is free)
//! proc free :3000
//!
//! # Why is this port busy?
//! proc why :3000
//!
//! # Pause and resume
//! proc freeze node && proc thaw node
//!
//! # Ask politely with SIGINT instead of SIGTERM
//! proc stop worker --signal INT
//!
//! # JSON output for scripting/LLMs
//! proc by node --json
//!
//! # Generate shell completions
//! proc completions zsh > ~/.zsh/completions/_proc
//! ```
//!
//! ## Commands
//!
//! **Find out** (read-only): `on`, `why`, `for`, `by`, `in`, `list`, `info`, `ports`, `tree`, `stuck`, `orphans`
//!
//! **Change it** (asks first, or previews with `--dry-run`): `stop`, `kill`, `free`, `freeze`, `thaw`, `unstick`
//!
//! **Keep watching**: `watch` (interactive TUI, or NDJSON with `--json`), `wait` (blocking, pipe-friendly)
//!
//! **Setup**: `completions`, `manpage`
//!
//! ## Exit Codes
//!
//! `0` success, `1` error or partial failure (including `wait` timeouts),
//! `2` not found, `3` permission denied, `4` invalid input (including unknown flags),
//! an action this OS does not support, or a command that changes processes run
//! without `--yes` where nobody can confirm (`--json`, or no terminal): nothing
//! was changed. A signal that ends proc while `watch` has the screen or a
//! prompt is waiting (SIGINT, SIGTERM, SIGHUP, SIGQUIT) puts the terminal back,
//! then ends proc by that signal, so a shell shows 128 plus its number.
//!
//! ## Errors
//!
//! Every fallible function returns [`Result`], whose error is a [`ProcError`].
//! [`ProcError::reason`] gives the fixed word the CLI prints as the JSON
//! error's `reason` (one of the `REASON_*` constants in [`error`]), and
//! [`error::ExitCode::from`] the exit code the CLI ends with. Branch on those,
//! not on the message, which is for people.
//!
//! | `reason` | Exit code | When |
//! |---|---|---|
//! | `system_error` | 1 | The OS gave an error, a signal failed, or an operation timed out |
//! | `not_found` | 2 | No process or port matched, or the process exited meanwhile |
//! | `command_line_only` | 2 | A name matched nothing, but is in some command lines (`--cmd` matches those) |
//! | `ruled_out` | 2 | A process holds the target, but a guard (`--start-time`, `--by`, `--in`) rules it out, or every match was a process proc runs under |
//! | `permission_denied` | 3 | proc may not read it, or a port's holder is out of sight (another user's process) |
//! | `invalid_input` | 4 | A wrong flag, value or argument |
//! | `needs_confirmation` | 4 | A command that changes processes had nobody to ask (`--json` or no terminal, without `--yes`) |
//! | `not_supported` | 4 | This OS cannot do it (`freeze`, `thaw`, `orphans` and `stop --signal` on Windows) |
//!
//! [`ProcError::Reported`] is the one error with no `reason`: its command
//! already printed its result, and the error only carries the exit code. The
//! JSON documents each command prints are described in
//! [reference.md](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md).
//!
//! ```
//! use proc_cli::core::parse_port;
//! use proc_cli::error::{ExitCode, REASON_INVALID_INPUT};
//!
//! let err = parse_port(":http").unwrap_err();
//! assert_eq!(err.reason(), Some(REASON_INVALID_INPUT));
//! assert_eq!(ExitCode::from(&err) as i32, 4);
//! assert_eq!(parse_port(":3000").unwrap(), 3000);
//! ```

// proc reads processes, ports and open files with code for each system it
// supports; anywhere else the build would fail on whichever of those it hits
// first, so it stops here and says which systems those are
#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "windows",
    target_os = "freebsd",
    target_os = "netbsd"
)))]
compile_error!(
    "proc supports macOS, Linux, Windows, FreeBSD and NetBSD, and this system is not one of them \
     (ROADMAP.md, \"More operating systems\", lists the ones under consideration)"
);

pub mod cli;
pub mod commands;
pub mod core;
pub mod error;
pub mod ui;
pub mod version;

#[cfg(test)]
mod test_support;

pub use error::{ProcError, Result};

/// Library version: the Cargo version for a release, and for any other build
/// from git the version with the commit named (see [`version`])
pub const VERSION: &str = env!("PROC_VERSION");
