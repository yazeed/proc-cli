//! Helpers shared by the unit tests

use std::io::Write;

/// A writer that accepts `left` more bytes and then fails
pub(crate) struct FailsAfter {
    pub(crate) left: usize,
    pub(crate) written: Vec<u8>,
}

impl Write for FailsAfter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            return Err(std::io::Error::other("disk full"));
        }
        let n = buf.len().min(self.left);
        self.written.extend_from_slice(&buf[..n]);
        self.left -= n;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Run `print` against a writer that fails after every possible number of
/// bytes: it must write exactly what fits, in order, and hand back the
/// writer's error instead of carrying on or panicking. Colours are off for
/// every run: the switch is global, so another test turning it off between
/// the full run and the cut ones would otherwise change the bytes (the Nix
/// build starts with colours on).
pub(crate) fn assert_stops_at_a_failed_write(
    print: impl Fn(&mut FailsAfter) -> std::io::Result<()>,
) {
    colored::control::set_override(false);
    let mut all = FailsAfter {
        left: usize::MAX,
        written: Vec::new(),
    };
    print(&mut all).unwrap();
    all.flush().unwrap();
    let full = all.written;
    assert!(!full.is_empty());
    for limit in 0..full.len() {
        let mut out = FailsAfter {
            left: limit,
            written: Vec::new(),
        };
        let err = print(&mut out).expect_err(&format!("limit {limit}"));
        assert_eq!(err.to_string(), "disk full", "limit {limit}");
        assert_eq!(out.written, full[..limit], "limit {limit}");
    }
}

/// A child process that only waits, for a test to look up, signal and reap.
/// `sleep` on Unix; `ping` on Windows, which has no `sleep` program and whose
/// `ping -n 300` lasts five minutes. Its output goes nowhere.
pub(crate) fn idle_child() -> std::process::Child {
    use std::process::{Command, Stdio};

    #[cfg(unix)]
    let mut command = {
        let mut command = Command::new("sleep");
        command.arg("300");
        command
    };
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("ping");
        command.args(["-n", "300", "127.0.0.1"]);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn an idle child")
}

/// A record of `child`, once its process shows in the process list
pub(crate) fn record_of(child: &std::process::Child) -> crate::core::Process {
    std::iter::repeat_with(|| {
        std::thread::sleep(std::time::Duration::from_millis(20));
        crate::core::Process::find_by_pid(child.id()).unwrap()
    })
    .take(500)
    .flatten()
    .next()
    .expect("the child never showed in the process list")
}

/// Kill `child` and wait for it, so a record of it is of a process that is gone
pub(crate) fn end(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// A record of `child` after it is gone: the child is killed and reaped, then
/// this waits until the process list no longer shows it (on Windows it can
/// linger for a moment after the kill)
pub(crate) fn gone_record(child: &mut std::process::Child) -> crate::core::Process {
    let record = record_of(child);
    end(child);
    std::iter::repeat_with(|| {
        std::thread::sleep(std::time::Duration::from_millis(20));
        record.is_running()
    })
    .take(500)
    .find(|running| !running)
    .expect("the process is still listed after it was killed");
    record
}
