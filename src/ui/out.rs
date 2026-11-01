//! Every byte proc writes to stdout or stderr goes through this module.
//!
//! `println!` and friends panic when the write fails, and Rust ignores SIGPIPE,
//! so a reader that stops early (`proc list | head -n1`) used to end proc with
//! a panic and exit 101, even when the command had just killed something. The
//! policy lives here, in one place:
//!
//! - **A broken pipe** means the reader is gone. Writing stops, quietly, on
//!   that stream, and the command goes on to the end and exits with the code
//!   its own result calls for: a `kill` that worked exits 0, a not-found exits
//!   2. A broken pipe on stderr changes nothing either.
//! - **Any other write error** (a full disk, a file size limit, an I/O error)
//!   is a failure: nothing more is written to that stream, and [`finish`] turns
//!   an exit 0 into exit 1 and says why on stderr, if stderr still works. A
//!   command that already failed keeps its own, more specific exit code.
//! - Nothing here panics, and nothing writes about a broken pipe.
//!
//! The commands print through [`out_println!`](crate::out_println!),
//! [`out_print!`](crate::out_print!), [`err_println!`](crate::err_println!) and
//! [`err_print!`](crate::err_print!), or through the writers [`stdout`] and
//! [`stderr`]. The failure is recorded on the stream when it happens, so a
//! deep helper does not need to pass an error up through every caller; `main`
//! reads it back once, in [`finish`], and unwinds normally (destructors and
//! terminal restore still run). `clippy.toml` bans the std print macros and
//! `std::io::stdout`/`stderr` everywhere else, so a new direct print fails
//! `cargo clippy -- -D warnings`.

// This module is the one place that may touch the std streams.
#![allow(clippy::disallowed_macros, clippy::disallowed_methods)]

use std::cell::Cell;
use std::fmt;
use std::io::{self, ErrorKind, IsTerminal, Write};
use std::sync::Mutex;
use std::time::Duration;

/// What has gone wrong on one stream so far
#[derive(Default)]
struct State {
    /// The reader is gone (`EPIPE`)
    broken: bool,
    /// The first other write error: its kind and message
    failure: Option<(ErrorKind, String)>,
}

/// One stream's health. Once a write has failed, later writes fail at once
/// without touching the stream, so a command that keeps printing costs nothing.
struct Channel {
    state: Mutex<State>,
}

impl Channel {
    const fn new() -> Self {
        Channel {
            state: Mutex::new(State {
                broken: false,
                failure: None,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // A poisoned lock only means another thread panicked while holding it;
        // the state is plain data and still right
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Run a write on the stream, unless an earlier one already failed, and
    /// remember how it went
    fn run<T>(&self, write: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
        {
            let state = self.lock();
            if state.broken {
                return Err(ErrorKind::BrokenPipe.into());
            }
            if let Some((kind, message)) = &state.failure {
                return Err(io::Error::new(*kind, message.clone()));
            }
        }
        let result = write();
        if let Err(err) = &result {
            let mut state = self.lock();
            if err.kind() == ErrorKind::BrokenPipe {
                state.broken = true;
            } else if state.failure.is_none() {
                state.failure = Some((err.kind(), err.to_string()));
            }
        }
        result
    }

    /// Whether nothing has gone wrong on the stream so far: the reader is
    /// still there and no write has failed
    fn usable(&self) -> bool {
        let state = self.lock();
        !state.broken && state.failure.is_none()
    }

    /// The message of the first write error that was not a broken pipe
    fn failure(&self) -> Option<String> {
        self.lock()
            .failure
            .as_ref()
            .map(|(_, message)| message.clone())
    }
}

static STDOUT: Channel = Channel::new();
static STDERR: Channel = Channel::new();

/// Which stream a [`Out`] writes to
#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

/// A writer for stdout or stderr that applies the module's policy. Cheap to
/// make; make one wherever a `Write` is wanted.
#[derive(Clone, Copy)]
pub struct Out {
    stream: Stream,
}

impl Out {
    fn channel(self) -> &'static Channel {
        match self.stream {
            Stream::Stdout => &STDOUT,
            Stream::Stderr => &STDERR,
        }
    }
}

impl Write for Out {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_all(buf).map(|()| buf.len())
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.channel().run(|| match self.stream {
            Stream::Stdout => io::stdout().write_all(buf),
            Stream::Stderr => io::stderr().write_all(buf),
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.channel().run(|| match self.stream {
            Stream::Stdout => io::stdout().flush(),
            Stream::Stderr => io::stderr().flush(),
        })
    }
}

thread_local! {
    /// Set while [`on_stderr`] runs: what this thread prints to stdout goes to stderr
    static STDOUT_ON_STDERR: Cell<bool> = const { Cell::new(false) };
}

/// The writer for stdout (or for stderr, while [`on_stderr`] runs)
pub fn stdout() -> Out {
    Out {
        stream: if STDOUT_ON_STDERR.get() {
            Stream::Stderr
        } else {
            Stream::Stdout
        },
    }
}

/// Run `show` with everything it prints to stdout going to stderr instead.
///
/// For a list that goes with a question: the question is asked on stderr, so
/// when stdout is a file or a pipe the list must go beside it, or the person
/// answers without seeing what they approve.
pub fn on_stderr<T>(show: impl FnOnce() -> T) -> T {
    /// Puts the routing back even if `show` panics
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            STDOUT_ON_STDERR.set(self.0);
        }
    }
    let _reset = Reset(STDOUT_ON_STDERR.replace(true));
    show()
}

/// Whether stdout is still good: its reader is there and no write has failed
pub fn stdout_usable() -> bool {
    STDOUT.usable()
}

/// Whether stderr is still good: its reader is there and no write has failed
pub fn stderr_usable() -> bool {
    STDERR.usable()
}

/// A writer that goes straight to the stream, whatever has failed before.
///
/// For putting a terminal back: the escape codes must be tried even after an
/// earlier write failed (the terminal may well still be there), and nothing
/// about them is worth recording.
pub struct Unrecorded {
    to_stderr: bool,
}

impl Write for Unrecorded {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_all(buf).map(|()| buf.len())
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        if self.to_stderr {
            io::stderr().write_all(buf)
        } else {
            io::stdout().write_all(buf)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.to_stderr {
            io::stderr().flush()
        } else {
            io::stdout().flush()
        }
    }
}

/// The writer for stdout that ignores earlier failures (see [`Unrecorded`])
pub fn stdout_unrecorded() -> Unrecorded {
    Unrecorded { to_stderr: false }
}

/// The writer for stderr that ignores earlier failures (see [`Unrecorded`])
pub fn stderr_unrecorded() -> Unrecorded {
    Unrecorded { to_stderr: true }
}

/// The writer for stderr
pub fn stderr() -> Out {
    Out {
        stream: Stream::Stderr,
    }
}

/// Whether stdout is a terminal
pub fn stdout_is_terminal() -> bool {
    io::stdout().is_terminal()
}

/// Whether stderr is a terminal
pub fn stderr_is_terminal() -> bool {
    io::stderr().is_terminal()
}

/// Wait for `span`, or until nobody reads stdout any more; true in that case.
///
/// For a command that prints on a timer (`watch --json`): the broken pipe is
/// only noticed by a write, and the next one is a whole interval away, so a
/// reader that left would be waited for that long. On Unix the wait itself
/// watches the pipe and ends the moment the reader closes it. Windows has no
/// such wait without more system calls than this is worth: there the reader's
/// leaving is noticed by the next write, as for any other command.
pub fn reader_gone_within(span: Duration) -> bool {
    !STDOUT.usable() || hung_up_within(span)
}

#[cfg(unix)]
fn hung_up_within(span: Duration) -> bool {
    poll_hangup(libc::STDOUT_FILENO, span)
}

#[cfg(not(unix))]
fn hung_up_within(span: Duration) -> bool {
    std::thread::sleep(span);
    false
}

/// Wait up to `span` for the other end of `fd` to be closed (`POLLERR` on a
/// pipe whose reader is gone, `POLLHUP` on a terminal that hung up)
#[cfg(unix)]
fn poll_hangup(fd: i32, span: Duration) -> bool {
    let deadline = std::time::Instant::now() + span;
    // A wait that a signal interrupts is started again, for what is left
    let outcome = std::iter::repeat_with(|| poll_once(fd, deadline)).find_map(|done| done);
    match outcome {
        Some((0, _, _)) => false,
        Some((ready, events, _)) if ready > 0 && events & (libc::POLLERR | libc::POLLHUP) != 0 => {
            true
        }
        // Something else (the descriptor is not open, or poll failed): there is
        // nothing to learn from it, so just sit out the rest of the wait
        other => {
            std::thread::sleep(other.map_or(Duration::ZERO, |(_, _, left)| left));
            false
        }
    }
}

/// The longest one `poll` waits. NetBSD's poll doesn't wake when a pipe's
/// reader closes during the wait: a reader that closed 1 s into a 5 s poll
/// left it waiting the whole 5 s, while one already closed is reported at
/// once. So there it looks again every 100 ms; elsewhere one poll waits it all
#[cfg(target_os = "netbsd")]
const POLL_SLICE: Duration = Duration::from_millis(100);
#[cfg(all(unix, not(target_os = "netbsd")))]
const POLL_SLICE: Duration = Duration::MAX;

/// One `poll` of `fd` until `deadline` (at most [`POLL_SLICE`]): what it
/// returned, the events it reported and the time that was left when it
/// started. `None` if a signal interrupted it, or the slice ended before the
/// deadline with nothing to report: look again.
#[cfg(unix)]
fn poll_once(fd: i32, deadline: std::time::Instant) -> Option<(i32, i16, Duration)> {
    let left = deadline.saturating_duration_since(std::time::Instant::now());
    // Round up, so a wait of under a millisecond does not become a spin
    let millis = left
        .min(POLL_SLICE)
        .as_micros()
        .div_ceil(1000)
        .min(i32::MAX as u128) as i32;
    // Linux reports a pipe whose reader left (POLLERR) whatever is asked
    // for; macOS reports nothing unless POLLHUP is asked for
    let mut watched = libc::pollfd {
        fd,
        events: libc::POLLHUP,
        revents: 0,
    };
    // SAFETY: one valid pollfd, and a count of one
    let ready = unsafe { libc::poll(&mut watched, 1, millis) };
    let interrupted = ready < 0 && io::Error::last_os_error().kind() == ErrorKind::Interrupted;
    let slice_ended = ready == 0 && std::time::Instant::now() < deadline;
    (!interrupted && !slice_ended).then_some((ready, watched.revents, left))
}

/// Settle the result of a write whose failure is already recorded on its
/// stream (see the module docs): there is nothing left to do with it here.
pub fn recorded(result: io::Result<()>) {
    drop(result);
}

/// Write formatted text to stdout, and a newline after it when `newline` is
/// set. This is what [`out_print!`](crate::out_print!) and [`out_println!`](crate::out_println!) call.
pub fn print_stdout(args: fmt::Arguments<'_>, newline: bool) {
    print_to(&mut stdout(), args, newline);
}

/// Write formatted text to stderr, and a newline after it when `newline` is
/// set. This is what [`err_print!`](crate::err_print!) and [`err_println!`](crate::err_println!) call.
pub fn print_stderr(args: fmt::Arguments<'_>, newline: bool) {
    print_to(&mut stderr(), args, newline);
}

fn print_to(out: &mut impl Write, args: fmt::Arguments<'_>, newline: bool) {
    let written = out.write_fmt(args);
    recorded(written);
    if newline {
        recorded(out.write_all(b"\n"));
    }
}

/// Run something that prints to stdout by itself (clap's help and version,
/// which choose their own colours) under the same policy: a failed write is
/// recorded, and nothing is written once stdout has failed.
pub fn through_stdout(print: impl FnOnce() -> io::Result<()>) {
    recorded(STDOUT.run(print));
}

/// Write bytes to stdout in one go (a generated man page or completion script)
pub fn write_stdout(bytes: &[u8]) {
    recorded(stdout().write_all(bytes));
}

/// `print!` for stdout: a failed write is recorded, never a panic
#[macro_export]
macro_rules! out_print {
    ($($arg:tt)*) => {
        $crate::ui::out::print_stdout(::std::format_args!($($arg)*), false)
    };
}

/// `println!` for stdout: a failed write is recorded, never a panic
#[macro_export]
macro_rules! out_println {
    () => {
        $crate::ui::out::print_stdout(::std::format_args!(""), true)
    };
    ($($arg:tt)*) => {
        $crate::ui::out::print_stdout(::std::format_args!($($arg)*), true)
    };
}

/// `eprint!` for stderr: a failed write is recorded, never a panic
#[macro_export]
macro_rules! err_print {
    ($($arg:tt)*) => {
        $crate::ui::out::print_stderr(::std::format_args!($($arg)*), false)
    };
}

/// `eprintln!` for stderr: a failed write is recorded, never a panic
#[macro_export]
macro_rules! err_println {
    () => {
        $crate::ui::out::print_stderr(::std::format_args!(""), true)
    };
    ($($arg:tt)*) => {
        $crate::ui::out::print_stderr(::std::format_args!($($arg)*), true)
    };
}

/// The exit code once the command is over: `code`, unless a write failed.
///
/// Flushes stdout (so a failure of the last, unterminated line is seen too).
/// A broken pipe leaves `code` alone. Any other write error on either stream
/// makes an exit 0 an exit 1, and one on stdout is also reported on stderr.
pub fn finish(code: u8) -> u8 {
    recorded(stdout().flush());
    resolve(code, &STDOUT, &STDERR, &mut stderr())
}

fn resolve(code: u8, stdout: &Channel, stderr: &Channel, report: &mut impl Write) -> u8 {
    let stdout_failure = stdout.failure();
    if let Some(message) = &stdout_failure {
        recorded(writeln!(report, "proc: cannot write to stdout: {message}"));
    }
    if code == 0 && (stdout_failure.is_some() || stderr.failure().is_some()) {
        1
    } else {
        code
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn broken() -> io::Result<()> {
        Err(ErrorKind::BrokenPipe.into())
    }

    fn full() -> io::Result<()> {
        Err(io::Error::other("no space left on device"))
    }

    #[test]
    fn a_broken_pipe_stops_the_stream_without_a_failure() {
        let channel = Channel::new();
        assert!(channel.run(|| Ok(())).is_ok());
        assert_eq!(
            channel.run(broken).unwrap_err().kind(),
            ErrorKind::BrokenPipe
        );
        // later writes fail at once, without running
        let err = channel
            .run(|| -> io::Result<()> { panic!("must not write after a broken pipe") })
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::BrokenPipe);
        assert_eq!(channel.failure(), None);
    }

    #[test]
    fn another_write_error_is_a_failure_that_sticks() {
        let channel = Channel::new();
        assert_eq!(
            channel.run(full).unwrap_err().to_string(),
            "no space left on device"
        );
        let err = channel
            .run(|| -> io::Result<()> { panic!("must not write after a failure") })
            .unwrap_err();
        assert_eq!(err.to_string(), "no space left on device");
        assert_eq!(
            channel.failure().as_deref(),
            Some("no space left on device")
        );
    }

    #[test]
    fn the_first_failure_is_the_one_kept() {
        let channel = Channel::new();
        let _ = channel.run(full);
        // a broken pipe after a failure does not replace it
        let _ = channel.run(|| Err::<(), _>(io::Error::new(ErrorKind::BrokenPipe, "late")));
        assert_eq!(
            channel.failure().as_deref(),
            Some("no space left on device")
        );
    }

    #[test]
    fn a_poisoned_lock_is_still_usable() {
        let channel = Channel::new();
        std::thread::scope(|scope| {
            let _ = scope
                .spawn(|| {
                    let _held = channel.state.lock().unwrap();
                    panic!("poison the lock");
                })
                .join();
        });
        assert!(channel.state.is_poisoned());
        assert!(channel.run(|| Ok(())).is_ok());
    }

    #[test]
    fn the_exit_code_follows_the_command_unless_a_write_failed() {
        let (stdout, stderr) = (Channel::new(), Channel::new());
        let mut report = Vec::new();
        assert_eq!(resolve(0, &stdout, &stderr, &mut report), 0);
        assert_eq!(resolve(2, &stdout, &stderr, &mut report), 2);
        assert!(report.is_empty());

        // a closed reader on either stream changes nothing
        let _ = stdout.run(broken);
        let _ = stderr.run(broken);
        assert_eq!(resolve(0, &stdout, &stderr, &mut report), 0);
        assert_eq!(resolve(2, &stdout, &stderr, &mut report), 2);
        assert!(report.is_empty());
    }

    #[test]
    fn a_failed_stdout_is_exit_1_and_is_reported() {
        let (stdout, stderr) = (Channel::new(), Channel::new());
        let _ = stdout.run(full);
        let mut report = Vec::new();
        assert_eq!(resolve(0, &stdout, &stderr, &mut report), 1);
        assert_eq!(
            String::from_utf8(report).unwrap(),
            "proc: cannot write to stdout: no space left on device\n"
        );
        // a command that failed on its own keeps its code
        assert_eq!(resolve(2, &stdout, &stderr, &mut Vec::new()), 2);
    }

    #[test]
    fn a_failed_stderr_is_exit_1_and_says_nothing() {
        let (stdout, stderr) = (Channel::new(), Channel::new());
        let _ = stderr.run(full);
        let mut report = Vec::new();
        assert_eq!(resolve(0, &stdout, &stderr, &mut report), 1);
        assert!(report.is_empty());
        assert_eq!(resolve(3, &stdout, &stderr, &mut report), 3);
    }

    #[test]
    fn the_message_is_lost_quietly_when_the_report_fails_too() {
        let (stdout, stderr) = (Channel::new(), Channel::new());
        let _ = stdout.run(full);
        let mut gone = crate::test_support::FailsAfter {
            left: 0,
            written: Vec::new(),
        };
        assert_eq!(resolve(0, &stdout, &stderr, &mut gone), 1);
    }

    #[test]
    fn the_real_streams_take_an_empty_write() {
        for mut out in [stdout(), stderr()] {
            assert_eq!(out.write(b"").unwrap(), 0);
            out.flush().unwrap();
        }
    }

    #[test]
    fn a_stream_is_usable_until_something_goes_wrong_on_it() {
        let channel = Channel::new();
        assert!(channel.usable());
        let _ = channel.run(broken);
        assert!(!channel.usable());

        let channel = Channel::new();
        let _ = channel.run(full);
        assert!(!channel.usable());
    }

    #[test]
    fn on_stderr_sends_stdout_to_stderr_and_puts_it_back() {
        let on = || matches!(stdout().stream, Stream::Stderr);
        assert!(!on());
        assert!(on_stderr(on));
        // inside another on_stderr, it stays on when the inner one ends
        assert!(on_stderr(|| {
            on_stderr(|| ());
            on()
        }));
        assert!(!on());
        // and it is put back when the list panics
        let caught = std::panic::catch_unwind(|| on_stderr(|| panic!("a list that panics")));
        assert!(caught.is_err());
        assert!(!on());
    }

    #[test]
    fn unrecorded_writers_take_an_empty_write() {
        for mut out in [stdout_unrecorded(), stderr_unrecorded()] {
            assert_eq!(out.write(b"").unwrap(), 0);
            out.flush().unwrap();
        }
    }

    #[test]
    fn a_wait_with_a_reader_still_there_lasts_the_whole_time() {
        let started = std::time::Instant::now();
        assert!(!reader_gone_within(Duration::from_millis(40)));
        assert!(started.elapsed() >= Duration::from_millis(40));
    }

    #[cfg(unix)]
    mod hangup {
        use super::super::*;
        use std::os::fd::{AsRawFd, IntoRawFd};
        use std::time::Instant;

        #[test]
        fn a_pipe_whose_reader_left_ends_the_wait_at_once() {
            let (reader, writer) = std::io::pipe().unwrap();
            let fd = writer.as_raw_fd();

            // reader still there: the wait runs its course
            let started = Instant::now();
            assert!(!poll_hangup(fd, Duration::from_millis(40)));
            assert!(started.elapsed() >= Duration::from_millis(40));

            drop(reader);
            let started = Instant::now();
            assert!(poll_hangup(fd, Duration::from_secs(30)));
            assert!(started.elapsed() < Duration::from_secs(5));
        }

        #[test]
        fn a_file_has_no_reader_to_lose() {
            let file = std::fs::File::open("/dev/null").unwrap();
            let started = Instant::now();
            assert!(!poll_hangup(file.as_raw_fd(), Duration::from_millis(40)));
            assert!(started.elapsed() >= Duration::from_millis(40));
        }

        #[test]
        fn a_descriptor_that_is_not_open_just_waits() {
            let (reader, _writer) = std::io::pipe().unwrap();
            let closed = reader.into_raw_fd();
            // SAFETY: closing the descriptor taken from `reader` just above
            unsafe { libc::close(closed) };
            let started = Instant::now();
            assert!(!poll_hangup(closed, Duration::from_millis(40)));
            assert!(started.elapsed() >= Duration::from_millis(40));
        }
    }

    #[test]
    fn print_to_writes_the_text_and_the_newline() {
        let mut out = Vec::new();
        print_to(&mut out, format_args!("{} {}", "a", 1), true);
        print_to(&mut out, format_args!("b"), false);
        assert_eq!(out, b"a 1\nb");
    }

    #[test]
    fn print_to_survives_a_failing_writer() {
        let mut gone = crate::test_support::FailsAfter {
            left: 1,
            written: Vec::new(),
        };
        print_to(&mut gone, format_args!("abc"), true);
        assert_eq!(gone.written, b"a");
    }
}
