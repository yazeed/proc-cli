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
//! - **Terminal-Adaptive Tables**: Tables adjust to terminal width automatically
//! - **Consistent Filters**: `--in`, `--by`, `--min-uptime`, `--parent`, `--range`, `--sort`, `--limit`
//! - **PID Guards**: `proc stop 1234 --start-time 1700000000 --yes` - act only while the PID still belongs to the process you saw (with `--in` and `--by` for its directory and name)
//! - **JSON Output**: `--json` on every command except `completions` and `manpage` — one `{action, success, ...}` document per run (`watch` streams one per refresh), and `success: false` always exits non-zero
//! - **Custom Signals**: `proc stop worker --signal INT` - send a different first signal; whatever is still running after `--timeout` is force-killed
//! - **Real-Time Monitoring**: `proc watch` / `proc top` - live process table (TUI on a terminal, NDJSON stream with `--json`)
//! - **Process Waiting**: `proc wait node` - block until process(es) exit (pipe-friendly)
//! - **Freeze/Thaw**: `proc freeze` / `proc thaw` - pause and resume by port, PID, or name
//! - **Port Freeing**: `proc free :3000` - kill and verify port is available
//! - **Ancestry Tracing**: `proc why :3000` - trace why a port is busy
//! - **Orphan Detection**: `proc orphans` - find abandoned child processes
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
//! # Free a port (kill + verify available)
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
//! **Discovery**: `on`, `for`, `by`, `in`, `list`, `info`, `ports`, `tree`, `stuck`, `why`, `orphans`
//!
//! **Lifecycle**: `kill`, `stop`, `unstick`, `freeze`, `thaw`, `free`
//!
//! **Monitoring**: `watch` (interactive TUI, or NDJSON with `--json`), `wait` (blocking, pipe-friendly)
//!
//! **Tooling**: `completions`, `manpage`
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
