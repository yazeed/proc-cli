//! A pseudo-terminal session run through `examples/session`, which holds the
//! terminal's session and runs the program as its foreground job, as a shell
//! does. The program then isn't the session's leader, so when it exits the
//! terminal isn't revoked (FreeBSD, NetBSD) and the test can still read all it
//! printed; the session ends when the test has read it and releases it, the
//! way the program ended (`examples/session.rs` has the measurements). Unix.

// Each test binary uses part of it
#![allow(dead_code)]

use portable_pty::CommandBuilder;
use std::path::PathBuf;

/// How the job ended, as `examples/session` writes it in `status`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    Exited(i32),
    Signaled(i32),
}

/// The directory a session reports in; removed when the test is done with it
pub struct Session {
    dir: tempfile::TempDir,
}

impl Session {
    pub fn new() -> Session {
        Session {
            dir: tempfile::tempdir().expect("a directory for the session"),
        }
    }

    /// `examples/session <program> <args>`, reporting in this session's directory
    pub fn command(&self, program: &str, args: &[&str]) -> CommandBuilder {
        let mut cmd = CommandBuilder::new(session_path());
        cmd.arg(program);
        cmd.args(args);
        cmd.env("PROC_TEST_SESSION", self.dir.path());
        cmd
    }

    /// How the job ended, once it has
    pub fn ended(&self) -> Option<Ended> {
        let status = std::fs::read_to_string(self.dir.path().join("status")).ok()?;
        let (how, number) = status.split_once(' ')?;
        let number = number.trim().parse().ok()?;
        match how {
            "exited" => Some(Ended::Exited(number)),
            "signaled" => Some(Ended::Signaled(number)),
            _ => None,
        }
    }

    /// The job's PID, once it has started
    pub fn job(&self) -> Option<i32> {
        std::fs::read_to_string(self.dir.path().join("pid"))
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// Lets the session end: everything the job printed has been read
    pub fn release(&self) {
        std::fs::write(self.dir.path().join("release"), "").expect("release the session");
    }

    /// Kill the job if it is still running, and release the session
    pub fn abandon(&self) {
        if self.ended().is_none() {
            if let Some(job) = self.job() {
                // SAFETY: signalling the job this session started
                unsafe {
                    libc::kill(job, libc::SIGKILL);
                }
            }
        }
        let _ = std::fs::write(self.dir.path().join("release"), "");
    }
}

/// `target/debug/examples/session` (every test binary that uses this module
/// declares `mod built`)
fn session_path() -> PathBuf {
    crate::built::example("session")
}
