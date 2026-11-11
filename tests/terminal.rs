//! Interactive behavior, through a real pseudo-terminal: the confirmation
//! prompts of every command that asks, and the `watch` screen.
//!
//! With no terminal on stdin a missing `--yes` is an error, so only a PTY
//! reaches the prompt. Every target is a process the test starts, named by PID,
//! port, or a fresh temp directory.
//!
//! Windows runs them through ConPTY. It rewrites what a program prints: it
//! draws with cursor movements instead of line breaks, may leave an unchanged
//! line out of a redraw, and answers nothing a program asks of the terminal
//! (conhost asks for the cursor position at start-up, and this harness
//! answers). So on Windows the output is read with its escape sequences taken
//! out (`plain`), a repeated frame is not waited for, and the cases that need
//! signals (`freeze`, `thaw`), a real orphan (Windows has no PID 1 to adopt
//! one), exact escape sequences, or a resize stay Unix only.

#[cfg(windows)]
use portable_pty::CommandBuilder;
use portable_pty::{native_pty_system, PtySize};
use proc_cli::core::filters::strip_verbatim_prefix;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

#[cfg(unix)]
mod session;

#[cfg(unix)]
const LEAVE_ALT_SCREEN: &str = "\x1b[?1049l";

// ---- the pseudo-terminal harness ----

/// One step of a scripted session
#[derive(Clone, Copy, Debug)]
enum Step {
    /// Wait until this text has appeared in the output, after the previous match
    Expect(&'static str),
    /// Type this (raw bytes: "\r" is Enter, "\x03" is Ctrl+C, "\x1b" is Esc)
    Send(&'static str),
    /// Resize the terminal to this many columns and rows, as dragging the window does
    #[cfg_attr(not(unix), allow(dead_code))]
    Resize(u16, u16),
    /// Do nothing for this long. For a session that must go through some
    /// intervals with no key pressed; nothing is asserted about what happened
    /// in the meantime, so a slow machine only makes the wait longer than needed.
    #[cfg_attr(unix, allow(dead_code))]
    Pause(Duration),
    /// The child has not exited by now
    #[cfg_attr(unix, allow(dead_code))]
    StillRunning,
    /// Wait until the terminal is in raw mode (line editing off): the child
    /// has turned it on. Unix only.
    #[cfg(unix)]
    Raw,
    /// Send the child this signal, after noting the terminal's mode. Unix only.
    #[cfg(unix)]
    Signal(i32),
}

/// What a session printed, and how the child ended
#[derive(Debug)]
struct PtyRun {
    output: String,
    /// The bytes as the terminal received them, escape sequences and all
    #[cfg_attr(unix, allow(dead_code))]
    raw: Vec<u8>,
    exit_code: u32,
    /// The signal that ended the child, as the system names it (`strsignal`)
    #[cfg(unix)]
    ended_by: Option<String>,
    /// The terminal's local mode flags (`ICANON`, `ECHO`, ...) when a `Signal`
    /// step was about to send its signal
    #[cfg(unix)]
    mode_at_signal: Option<u64>,
    /// ... and once the child had ended
    #[cfg(unix)]
    mode_at_end: Option<u64>,
}

/// The terminal's local mode flags, if they can be read. (`tcflag_t` is 32 bits
/// on Linux and 64 on macOS, hence the cast.)
#[cfg(unix)]
#[allow(clippy::unnecessary_cast)]
fn local_flags(master: &dyn portable_pty::MasterPty) -> Option<u64> {
    master.get_termios().map(|t| t.local_flags.bits() as u64)
}

/// Whether `flags` have line editing (`ICANON`) and echo (`ECHO`) both on
#[cfg(unix)]
#[allow(clippy::unnecessary_cast)]
fn is_cooked(flags: Option<u64>) -> bool {
    let wanted = (libc::ICANON | libc::ECHO) as u64;
    flags.is_some_and(|flags| flags & wanted == wanted)
}

/// How many bytes the child wrote that the reader hasn't taken yet. Linux
/// counts them with FIONREAD on our end. macOS answers that with 0 however much
/// is waiting, and counts them with TIOCOUTQ (`_IOR('t', 115, int)`; libc has
/// no name for it there or on NetBSD), which FreeBSD and NetBSD answer too;
/// all three measured with 23 bytes waiting
#[cfg(unix)]
fn unread(master: &dyn portable_pty::MasterPty) -> libc::c_int {
    #[cfg(target_os = "linux")]
    let request = libc::FIONREAD as libc::c_ulong;
    #[cfg(not(target_os = "linux"))]
    let request: libc::c_ulong = 0x4004_7473;
    let fd = master.as_raw_fd().expect("the pty's descriptor");
    let mut waiting: libc::c_int = 0;
    // SAFETY: the request writes one int, into `waiting`
    if unsafe { libc::ioctl(fd, request as _, &mut waiting) } != 0 {
        return 0;
    }
    waiting
}

/// Unix: once the program has ended, read what it printed to the very end.
/// The session still holds the terminal, so all of it is in the terminal's
/// queue: wait until the reader has taken it (nothing unread), let the session
/// end, then close our copy of the child's end, so the reader stores its last
/// chunk and then sees end-of-file. Bounded, like the session
#[cfg(unix)]
fn finish_reading(
    master: &dyn portable_pty::MasterPty,
    slave: &mut Option<Box<dyn portable_pty::SlavePty + Send>>,
    reading: &std::thread::JoinHandle<()>,
    guard: &mut ChildGuard,
) {
    let until = Instant::now() + Duration::from_secs(5);
    while unread(master) > 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    guard.1.release();
    while guard.0.try_wait().expect("poll the session").is_none() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(slave.take());
    while !reading.is_finished() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Held while a session opens its terminal and starts the child in it; the
/// sessions then run side by side. portable-pty opens a terminal and marks its
/// descriptors close-on-exec in a second step, so a child another test starts
/// in between inherits them, and the child's own cleanup can't close them
/// where `/dev/fd` lists only 0 to 2 (FreeBSD without fdescfs). On FreeBSD,
/// width tests opening terminals 4 at a time then failed to start proc (EPERM)
/// in 5 runs of 8, and in 0 of 12 holding this; one at a time, 0 of 8
static SPAWNING: Mutex<()> = Mutex::new(());

/// The whole session may take this long; then the child is killed and the test fails
const SESSION_LIMIT: Duration = Duration::from_secs(20);

/// Kills and reaps the child on every path out of a session, including a
/// failed assertion or a timeout. On Unix the child is `examples/session`, and
/// the program it runs is killed too.
struct ChildGuard(
    Box<dyn portable_pty::Child + Send + Sync>,
    #[cfg(unix)] session::Session,
);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        self.1.abandon();
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// What a terminal shows of `raw`, as text: on Windows, ConPTY's output with the
/// escape sequences taken out (a cursor move forward becomes spaces, a move to
/// another line becomes a line break, an incomplete sequence at the end is left
/// for the next read). On Unix the output is read as it is.
fn plain(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw).into_owned();
    if cfg!(unix) {
        return text;
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '\x1b' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        match chars.get(i + 1) {
            // an escape at the very end: the rest has not arrived
            None => break,
            Some('[') => {
                let mut j = i + 2;
                while j < chars.len() && !('\x40'..='\x7e').contains(&chars[j]) {
                    j += 1;
                }
                if j == chars.len() {
                    break;
                }
                let params: String = chars[i + 2..j].iter().collect();
                match chars[j] {
                    'C' => {
                        let n: usize = params.parse().unwrap_or(1);
                        out.push_str(&" ".repeat(n));
                    }
                    'H' | 'd' | 'B' | 'E' => out.push('\n'),
                    _ => {}
                }
                i = j + 1;
            }
            Some(']') => {
                // a title or hyperlink, ended by BEL or ESC \
                let mut j = i + 2;
                while j < chars.len()
                    && chars[j] != '\u{7}'
                    && !(chars[j] == '\x1b' && chars.get(j + 1) == Some(&'\\'))
                {
                    j += 1;
                }
                if j >= chars.len() {
                    break;
                }
                i = if chars[j] == '\u{7}' { j + 1 } else { j + 2 };
            }
            Some(_) => i += 2,
        }
    }
    out
}

/// conhost asks the terminal for the cursor position (`ESC [ 6 n`) when a
/// session starts and waits for the answer; this is the answer, once for each
/// time it asked. Nothing on Unix asks.
#[cfg(windows)]
fn answer_cursor_queries(raw: &[u8], answered: &mut usize, writer: &mut dyn Write) {
    let asked = raw.windows(4).filter(|w| *w == b"\x1b[6n").count();
    while *answered < asked {
        writer
            .write_all(b"\x1b[1;1R")
            .expect("answer the cursor query");
        writer.flush().expect("flush the pty");
        *answered += 1;
    }
}

/// Run `proc <args>` with a pseudo-terminal as stdin, stdout and stderr, play
/// `script` against it, and return everything it printed and its exit code.
///
/// The terminal is read on its own thread, so a child that blocks or stops
/// printing cannot hang the test: `Expect` polls with a deadline. The whole
/// session is limited to [`SESSION_LIMIT`].
fn run_in_pty(args: &[&str], script: &[Step]) -> PtyRun {
    try_run_in_pty(args, script).unwrap_or_else(|why| panic!("{why}"))
}

/// [`run_in_pty`], except that a session that never prints what an `Expect`
/// waits for (it timed out, or proc ended without printing it) is an `Err`
/// with what proc printed, for a caller that may ask again.
fn try_run_in_pty(args: &[&str], script: &[Step]) -> Result<PtyRun, String> {
    try_run_program_in_pty(env!("CARGO_BIN_EXE_proc"), args, script)
}

/// [`try_run_in_pty`] for any program: a shell that redirects what proc
/// prints, for one
fn try_run_program_in_pty(program: &str, args: &[&str], script: &[Step]) -> Result<PtyRun, String> {
    let deadline = Instant::now() + SESSION_LIMIT;
    let spawning = SPAWNING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open a pseudo-terminal");

    // Unix: through `examples/session`, which holds the terminal's session so
    // that proc's exit doesn't revoke it before its output is read
    #[cfg(unix)]
    let session = session::Session::new();
    #[cfg(unix)]
    let mut cmd = session.command(program, args);
    #[cfg(windows)]
    let mut cmd = CommandBuilder::new(program);
    #[cfg(windows)]
    cmd.args(args);
    cmd.env("TERM", "xterm-256color");
    cmd.env_remove("NO_COLOR");
    let child = pair
        .slave
        .spawn_command(cmd)
        .expect("spawn proc in the pty");
    drop(spawning);
    // Windows: the child holds the only other end; with ours closed, the
    // reader sees end-of-file when the child is gone.
    // Unix: we keep our copy of the child's end until we have read everything,
    // then close it, and the reader sees end-of-file. When the last copy of
    // that end closes, macOS and NetBSD throw away output not yet read (a
    // child that wrote and exited at once lost all of it, measured on both),
    // so proc's last lines went missing whenever it outran the reader
    #[cfg(windows)]
    drop(pair.slave);
    #[cfg(unix)]
    let mut slave = Some(pair.slave);
    #[cfg(unix)]
    let mut guard = ChildGuard(child, session);
    #[cfg(windows)]
    let mut guard = ChildGuard(child);

    let mut writer = pair.master.take_writer().expect("pty writer");
    let mut reader = pair.master.try_clone_reader().expect("pty reader");
    let captured = Arc::new(Mutex::new(Vec::<u8>::new()));
    let sink = Arc::clone(&captured);
    let reading = std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        // Linux reports the closed terminal as an error (EIO), macOS as 0 bytes
        while let Ok(n) = reader.read(&mut chunk) {
            if n == 0 {
                break;
            }
            sink.lock().unwrap().extend_from_slice(&chunk[..n]);
        }
    });

    let text = || plain(&captured.lock().unwrap());
    #[cfg(windows)]
    let mut answered = 0;
    #[cfg(windows)]
    let mut answer = |writer: &mut Box<dyn Write + Send>| {
        answer_cursor_queries(&captured.lock().unwrap(), &mut answered, writer)
    };
    let mut cursor = 0;
    #[cfg(unix)]
    let mut mode_at_signal = None;
    for step in script {
        match *step {
            #[cfg(unix)]
            Step::Raw => {
                let until = Instant::now() + Duration::from_secs(5);
                while is_cooked(local_flags(&*pair.master)) {
                    if Instant::now() >= until {
                        return Err(format!(
                            "the terminal never went raw; proc printed:\n{:?}",
                            text()
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            #[cfg(unix)]
            Step::Signal(signal) => {
                mode_at_signal = local_flags(&*pair.master);
                // `examples/session` sends it on to proc
                let pid = guard.0.process_id().expect("the child's PID");
                // SAFETY: signalling the child this session started and has not reaped
                unsafe {
                    libc::kill(pid as i32, signal);
                }
            }
            Step::Send(keys) => {
                writer
                    .write_all(keys.as_bytes())
                    .expect("type into the pty");
                writer.flush().expect("flush the pty");
            }
            Step::Pause(span) => std::thread::sleep(span),
            Step::StillRunning => assert!(
                guard.0.try_wait().expect("poll the child").is_none(),
                "proc exited early; it printed:\n{:?}",
                text()
            ),
            Step::Resize(cols, rows) => {
                pair.master
                    .resize(PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    })
                    .expect("resize the pty");
            }
            Step::Expect(wanted) => loop {
                #[cfg(windows)]
                answer(&mut writer);
                // Read before looking: if proc had ended and its output had
                // been read to the end by now, `seen` is all it will ever print
                #[cfg(unix)]
                let exited = guard.1.ended().is_some();
                #[cfg(windows)]
                let exited = guard.0.try_wait().expect("poll the child").is_some();
                #[cfg(unix)]
                if exited {
                    finish_reading(&*pair.master, &mut slave, &reading, &mut guard);
                }
                let over = exited && reading.is_finished();
                let seen = text();
                if let Some(at) = seen.get(cursor..).and_then(|rest| rest.find(wanted)) {
                    cursor += at + wanted.len();
                    break;
                }
                if over {
                    return Err(format!(
                        "proc ended without printing {wanted:?}; it printed:\n{seen:?}"
                    ));
                }
                if Instant::now() >= deadline {
                    return Err(format!(
                        "timed out waiting for {wanted:?}; proc printed:\n{seen:?}"
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            },
        }
    }

    #[cfg(windows)]
    let status = loop {
        answer(&mut writer);
        match guard.0.try_wait().expect("poll the child") {
            Some(status) => break status,
            None => {
                assert!(
                    Instant::now() < deadline,
                    "proc did not exit in {SESSION_LIMIT:?}; it printed:\n{:?}",
                    text()
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };
    // Unix: proc has ended once the session says so; the session itself ends
    // after its output is read (`finish_reading`), the way proc did
    #[cfg(unix)]
    while guard.1.ended().is_none() {
        assert!(
            Instant::now() < deadline,
            "proc did not exit in {SESSION_LIMIT:?}; it printed:\n{:?}",
            text()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    #[cfg(unix)]
    let mode_at_end = local_flags(&*pair.master);
    // Whatever it printed last is still in the pipe: let the reader reach the end.
    // ConPTY keeps the pipe open until the terminal is closed, so on Windows the
    // terminal is closed here (the reader keeps draining while it closes); the
    // end of file is then the end of the output, not a guess at a quiet spell.
    // Closing can block on some Windows versions, so it is done on a thread the
    // test does not wait for: the wait below has a deadline.
    #[cfg(windows)]
    {
        let master = pair.master;
        std::thread::spawn(move || drop(master));
    }
    #[cfg(unix)]
    finish_reading(&*pair.master, &mut slave, &reading, &mut guard);
    #[cfg(unix)]
    let status = guard
        .0
        .try_wait()
        .expect("poll the session")
        .expect("the session ends once released");
    let drained = Instant::now() + Duration::from_secs(5);
    while !reading.is_finished() && Instant::now() < drained {
        std::thread::sleep(Duration::from_millis(5));
    }
    let raw = captured.lock().unwrap().clone();
    Ok(PtyRun {
        output: text(),
        raw,
        exit_code: status.exit_code(),
        #[cfg(unix)]
        ended_by: status.signal().map(String::from),
        #[cfg(unix)]
        mode_at_signal,
        #[cfg(unix)]
        mode_at_end,
    })
}

#[test]
fn harness_runs_a_script() {
    let run = run_in_pty(&["--version"], &[]);
    assert_eq!(run.exit_code, 0, "{}", run.output);
    assert!(run.output.contains("proc"), "{}", run.output);
}

// Exact escape sequences: ConPTY rewrites them, so Unix only
#[cfg(unix)]
#[test]
fn the_comma_notice_is_coloured_on_a_terminal() {
    // PIDs above every OS maximum: a dry run that finds nothing, exit 2
    let run = run_in_pty(&["kill", "99999999,99999998", "--dry-run"], &[]);
    assert_eq!(run.exit_code, 2, "{}", run.output);
    assert!(
        run.output
            .contains("\x1b[1;36mnote:\x1b[0m comma-separated targets are deprecated"),
        "{:?}",
        run.output
    );
}

// ---- fixtures ----

/// Held for as long as a busy process runs, so no more than one spins at a time
static BUSY: Mutex<()> = Mutex::new(());

/// A shell loop that uses a whole core and ends itself after two minutes, so a
/// test binary killed without running `Drop` cannot leave it spinning for long
#[cfg(unix)]
const SPIN: &str = "end=$(( $(date +%s) + 120 )); n=0; \
    while :; do n=$((n+1)); \
    if [ $((n % 50000)) -eq 0 ] && [ \"$(date +%s)\" -ge \"$end\" ]; then exit 0; fi; done";

/// A process the test owns, killed and reaped on drop whatever a case did to it
struct Owned {
    child: Child,
    port: Option<u16>,
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    /// Set for a busy process; released after `Drop` has killed it
    _busy: Option<MutexGuard<'static, ()>>,
}

fn fresh_dir() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    // A process reports its cwd with symlinks resolved (/var is /private/var
    // on macOS) and no Windows `\\?\` prefix, and `--in` compares paths literally
    let dir = tmp.path().canonicalize().unwrap();
    let dir = PathBuf::from(strip_verbatim_prefix(&dir.to_string_lossy()));
    (tmp, dir)
}

impl Owned {
    /// The listener example: owns a port, sleeps, ends on SIGTERM
    fn listener() -> Self {
        Self::listener_with_args(&[])
    }

    /// The listener with words on its command line (it ignores them)
    fn listener_with_args(args: &[&str]) -> Self {
        let (tmp, dir) = fresh_dir();
        let mut child = Command::new(listener_path())
            .args(args)
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
        if port.is_none() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the listener printed {line:?} instead of `PORT <n>`");
        }
        Owned {
            child,
            port,
            _tmp: tmp,
            dir,
            _busy: None,
        }
    }

    /// A process spinning in a busy loop in a fresh directory: stuck by proc's
    /// measure (over half a core in two readings a second apart). A shell on
    /// Unix, the listener example on Windows. Only one spins at a time, and it
    /// ends itself after two minutes.
    fn busy() -> Self {
        let guard = BUSY.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (tmp, dir) = fresh_dir();
        #[cfg(unix)]
        let mut command = {
            let mut command = Command::new("sh");
            command.args(["-c", SPIN]);
            command
        };
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new(listener_path());
            command.arg("--busy");
            command
        };
        let child = command
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .expect("spawn a busy process");
        let busy = Owned {
            child,
            port: None,
            _tmp: tmp,
            dir,
            _busy: Some(guard),
        };
        busy.wait_until_stuck();
        busy
    }

    /// Return once `proc stuck` lists this process. proc has no CPU reading for
    /// a process that has used no CPU yet, so one started this instant is not
    /// found until a later look; a command that runs before then would find
    /// nothing.
    fn wait_until_stuck(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let out = Command::new(env!("CARGO_BIN_EXE_proc"))
                .args(["stuck", "--timeout", "0", "--in", &self.dir(), "--json"])
                .output()
                .expect("run proc stuck");
            let json: serde_json::Value =
                serde_json::from_slice(&out.stdout).expect("proc stuck prints JSON");
            let listed = json["processes"].as_array().is_some_and(|items| {
                items
                    .iter()
                    .any(|p| p["pid"].as_u64() == Some(u64::from(self.child.id())))
            });
            if listed {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the busy shell was never seen as stuck: {json}"
            );
        }
    }

    fn pid(&self) -> String {
        self.child.id().to_string()
    }

    fn dir(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }

    fn port(&self) -> String {
        self.port.expect("a listener").to_string()
    }

    #[cfg(unix)]
    fn signal(&self, sig: i32) {
        // SAFETY: signalling a child this test started and has not reaped
        unsafe {
            libc::kill(self.child.id() as i32, sig);
        }
    }

    /// Whether the child is still there: not exited, and not a zombie either
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Wait for the child to end (it is ours, so waiting also reaps it)
    fn ended_within(&mut self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if !self.alive() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// The process state letters from `ps` ("T" while stopped), empty if gone
    #[cfg(unix)]
    fn state(&self) -> String {
        let out = Command::new("ps")
            .args(["-o", "stat=", "-p", &self.pid()])
            .output()
            .expect("run ps");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[cfg(unix)]
    fn wait_state(&self, stopped: bool, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if self.state().starts_with('T') == stopped {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A process whose parent has already exited: a shell starts `sleep` in the
/// background and ends, so the init process (PID 1) adopts the sleeper. Unix
/// only: on Windows nothing adopts an orphan, and proc has none to find.
#[cfg(unix)]
struct Orphan {
    pid: i32,
    /// Set once the sleeper has been seen gone, so `drop` never signals the PID
    /// again: the kernel may have given it to some other process by then
    gone: std::cell::Cell<bool>,
    _tmp: tempfile::TempDir,
    dir: PathBuf,
}

#[cfg(unix)]
impl Orphan {
    fn start() -> Self {
        let (tmp, dir) = fresh_dir();
        let out = Command::new("sh")
            .args(["-c", "sleep 300 >/dev/null 2>&1 </dev/null & echo $!"])
            .current_dir(&dir)
            .stdin(Stdio::null())
            .output()
            .expect("spawn the orphan's parent");
        let pid: i32 = String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse()
            .expect("the shell prints the sleeper's pid");
        let orphan = Orphan {
            pid,
            gone: std::cell::Cell::new(false),
            _tmp: tmp,
            dir,
        };
        // Wait until the kernel has handed it to init
        let deadline = Instant::now() + Duration::from_secs(5);
        while orphan.ppid() != Some(1) {
            assert!(Instant::now() < deadline, "the sleeper was not reparented");
            std::thread::sleep(Duration::from_millis(50));
        }
        orphan
    }

    fn ppid(&self) -> Option<i32> {
        let out = Command::new("ps")
            .args(["-o", "ppid=", "-p", &self.pid.to_string()])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }

    fn dir(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }

    fn alive(&self) -> bool {
        // SAFETY: signal 0 only checks that the process exists
        let alive = unsafe { libc::kill(self.pid, 0) == 0 };
        if !alive {
            self.gone.set(true);
        }
        alive
    }

    /// The sleeper is not our child, so nobody reaps it but init: gone means
    /// signalling it fails (a zombie waiting on init is shown by `ps` as `Z`)
    fn gone_within(&self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            let out = Command::new("ps")
                .args(["-o", "stat=", "-p", &self.pid.to_string()])
                .output()
                .expect("run ps");
            let state = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if state.is_empty() || state.starts_with('Z') {
                self.gone.set(true);
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }
}

#[cfg(unix)]
impl Drop for Orphan {
    fn drop(&mut self) {
        if self.gone.get() {
            return;
        }
        // SAFETY: the pid is the sleeper this test started, and it has not been
        // seen gone, so the PID is still its own
        unsafe {
            libc::kill(self.pid, libc::SIGKILL);
        }
    }
}

/// `target/debug/examples/listener`, found relative to this test binary
/// (`target/debug/deps/terminal-<hash>`)
fn listener_path() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("examples");
    path.push(format!("listener{}", std::env::consts::EXE_SUFFIX));
    assert!(path.exists(), "build the example first: {}", path.display());
    path
}

// ---- prompt cases ----

const WAIT: Duration = Duration::from_secs(5);

/// How long a session against a busy process is started again, when proc found
/// nothing stuck to ask about
const BUSY_PATIENCE: Duration = Duration::from_secs(30);

/// [`run_in_pty`] for a session that begins with a question. If proc says
/// there is nothing stuck, the busy process got no CPU in the two readings it
/// takes (proc was right; `Owned::busy` returned once an earlier look had seen
/// it busy, and this run measures again, over a window of its own). It asked
/// nothing and changed nothing, so the session is started again, for at most
/// [`BUSY_PATIENCE`]; any other failure is final.
fn ask(args: &[&str], script: &[Step]) -> PtyRun {
    let deadline = Instant::now() + BUSY_PATIENCE;
    loop {
        match try_run_in_pty(args, script) {
            Ok(run) => return run,
            Err(why) if why.contains("No stuck processes found") && Instant::now() < deadline => {}
            Err(why) => panic!("{why}"),
        }
    }
}

/// Ask, answer `y`: the command acts. The prompt takes `y` or `n` as one key,
/// with no Enter, and an Enter typed after it can find proc gone: where the
/// kernel revokes the terminal when its session leader exits (NetBSD), that
/// write fails with EIO. So a session answers with the key alone.
fn answers_y(args: &[&str], prompt: &'static str) -> PtyRun {
    ask(args, &[Step::Expect(prompt), Step::Send("y")])
}

/// Ask, answer `n`: nothing happens, and the command says `declined`
fn answers_n(args: &[&str], prompt: &'static str, declined: &'static str) -> PtyRun {
    let run = ask(
        args,
        &[
            Step::Expect(prompt),
            Step::Send("n"),
            Step::Expect(declined),
        ],
    );
    assert!(run.output.contains(declined), "{}", run.output);
    run
}

/// Ask, press Enter alone: the default is no
fn answers_enter(args: &[&str], prompt: &'static str, declined: &'static str) -> PtyRun {
    let run = ask(
        args,
        &[
            Step::Expect(prompt),
            Step::Send("\r"),
            Step::Expect(declined),
        ],
    );
    assert!(run.output.contains(declined), "{}", run.output);
    run
}

fn assert_ok(run: &PtyRun) {
    assert_eq!(run.exit_code, 0, "{}", run.output);
}

#[test]
fn kill_asks_first() {
    let mut p = Owned::listener();
    let args = ["kill", &p.pid()];
    let run = answers_n(&args, "Kill 1 process", "Cancelled");
    assert_ok(&run);
    assert!(p.alive(), "answering n left it running");

    let run = answers_enter(&args, "Kill 1 process", "Cancelled");
    assert_ok(&run);
    assert!(p.alive(), "Enter alone left it running");

    let run = answers_y(&args, "Kill 1 process");
    assert_ok(&run);
    assert!(
        p.ended_within(WAIT),
        "answering y killed it: {}",
        run.output
    );
}

#[test]
fn the_prompt_shows_the_whole_command_line() {
    // A command line longer than the 120-column terminal: the list cuts it,
    // the question's list is what a person approves, so it does not
    let note: &'static str = Box::leak(format!("--note={}", "word-".repeat(40)).into_boxed_str());
    let mut p = Owned::listener_with_args(&[note]);
    let args = ["kill", &p.pid()];
    let run = answers_n(&args, note, "Cancelled");
    assert_ok(&run);
    assert!(p.alive());
    assert!(!run.output.contains('…'), "{}", run.output);
}

#[test]
fn stop_asks_first() {
    let mut p = Owned::listener();
    // Windows cannot ask a console program to close, so stop waits out the
    // timeout before it kills
    let args = ["stop", &p.pid(), "--timeout", "1"];
    let run = answers_n(&args, "Stop 1 process", "Cancelled");
    assert_ok(&run);
    assert!(p.alive());

    let run = answers_enter(&args, "Stop 1 process", "Cancelled");
    assert_ok(&run);
    assert!(p.alive());

    let run = answers_y(&args, "Stop 1 process");
    assert_ok(&run);
    assert!(
        p.ended_within(WAIT),
        "answering y stopped it: {}",
        run.output
    );
}

#[test]
fn free_asks_first() {
    let mut p = Owned::listener();
    let target = format!(":{}", p.port());
    let args = ["free", &target];
    let run = answers_n(&args, "Kill 1 process to free 1 port", "Aborted");
    assert_ok(&run);
    assert!(p.alive());

    let run = answers_enter(&args, "Kill 1 process to free 1 port", "Aborted");
    assert_ok(&run);
    assert!(p.alive());

    let run = answers_y(&args, "Kill 1 process to free 1 port");
    assert_ok(&run);
    assert!(p.ended_within(WAIT), "answering y freed it: {}", run.output);
}

#[cfg(unix)]
#[test]
fn freeze_asks_first() {
    let p = Owned::listener();
    let args = ["freeze", &p.pid()];
    let run = answers_n(&args, "Freeze 1 process", "Cancelled");
    assert_ok(&run);
    assert!(!p.state().starts_with('T'), "answering n left it running");

    let run = answers_enter(&args, "Freeze 1 process", "Cancelled");
    assert_ok(&run);
    assert!(!p.state().starts_with('T'));

    let run = answers_y(&args, "Freeze 1 process");
    assert_ok(&run);
    assert!(
        p.wait_state(true, WAIT),
        "answering y froze it ({}): {}",
        p.state(),
        run.output
    );
}

#[cfg(unix)]
#[test]
fn thaw_asks_first() {
    let p = Owned::listener();
    p.signal(libc::SIGSTOP);
    assert!(p.wait_state(true, WAIT), "the test froze its listener");
    let args = ["thaw", &p.pid()];
    let run = answers_n(&args, "Thaw 1 process", "Cancelled");
    assert_ok(&run);
    assert!(p.state().starts_with('T'), "answering n left it frozen");

    let run = answers_enter(&args, "Thaw 1 process", "Cancelled");
    assert_ok(&run);
    assert!(p.state().starts_with('T'));

    let run = answers_y(&args, "Thaw 1 process");
    assert_ok(&run);
    assert!(
        p.wait_state(false, WAIT),
        "answering y thawed it ({}): {}",
        p.state(),
        run.output
    );
}

#[test]
fn unstick_asks_first() {
    let mut p = Owned::busy();
    let dir = p.dir();
    let args = ["unstick", "--force", "--timeout", "0", "--in", &dir];
    let run = answers_n(&args, "Unstick 1 process", "Aborted");
    assert_ok(&run);
    assert!(p.alive());

    let run = answers_enter(&args, "Unstick 1 process", "Aborted");
    assert_ok(&run);
    assert!(p.alive());

    let run = answers_y(&args, "Unstick 1 process");
    assert_ok(&run);
    assert!(
        p.ended_within(WAIT),
        "answering y terminated it: {}",
        run.output
    );
}

#[test]
fn unstick_without_force_says_it_will_only_try_to_recover() {
    let mut p = Owned::busy();
    let dir = p.dir();
    let args = ["unstick", "--timeout", "0", "--in", &dir];
    let run = answers_n(&args, "Unstick 1 process", "Aborted");
    assert_ok(&run);
    assert!(
        run.output.contains("Will attempt recovery only"),
        "{}",
        run.output
    );
    assert!(p.alive());
}

#[test]
fn stuck_kill_asks_first() {
    let mut p = Owned::busy();
    let dir = p.dir();
    let args = ["stuck", "--kill", "--timeout", "0", "--in", &dir];
    let run = answers_n(&args, "Kill 1 stuck process", "Cancelled");
    assert_ok(&run);
    assert!(p.alive());

    let run = answers_enter(&args, "Kill 1 stuck process", "Cancelled");
    assert_ok(&run);
    assert!(p.alive());

    let run = answers_y(&args, "Kill 1 stuck process");
    assert_ok(&run);
    assert!(
        p.ended_within(WAIT),
        "answering y killed it: {}",
        run.output
    );
}

#[cfg(unix)]
#[test]
fn orphans_kill_asks_first() {
    let o = Orphan::start();
    let dir = o.dir();
    let args = ["orphans", "--kill", "--in", &dir];
    let run = answers_n(&args, "Kill 1 orphaned process", "Cancelled");
    assert_ok(&run);
    assert!(o.alive());

    let run = answers_enter(&args, "Kill 1 orphaned process", "Cancelled");
    assert_ok(&run);
    assert!(o.alive());

    let run = answers_y(&args, "Kill 1 orphaned process");
    assert_ok(&run);
    assert!(o.gone_within(WAIT), "answering y killed it: {}", run.output);
}

// ---- watch ----

/// Start `watch`, wait for its first screen, then send `keys`
fn watch_quits_on(keys: &'static str) {
    let p = Owned::listener();
    let pid = p.pid();
    let run = run_in_pty(
        &["watch", &pid],
        &[Step::Expect("Watching"), Step::Send(keys)],
    );
    assert_ok(&run);
    #[cfg(unix)]
    assert!(
        run.output.ends_with(LEAVE_ALT_SCREEN),
        "the screen is left last: {:?}",
        &run.output[run.output.len().saturating_sub(60)..]
    );
    // ConPTY adds its own sequences after it, so on Windows the screen is left
    // somewhere, not last; both switches still reach the terminal
    #[cfg(windows)]
    {
        let raw = String::from_utf8_lossy(&run.raw);
        let entered = raw.find("\x1b[?1049h");
        let left = raw.rfind("\x1b[?1049l");
        assert!(
            matches!((entered, left), (Some(e), Some(l)) if e < l),
            "the alternate screen is entered, then left: {raw:?}"
        );
    }
}

#[test]
fn watch_quits_on_q() {
    watch_quits_on("q");
}

// A lone Escape byte is a key only after a timeout in ConPTY, and Ctrl+C is a
// signal there unless the program has turned that off: Unix only
#[cfg(unix)]
#[test]
fn watch_quits_on_escape() {
    watch_quits_on("\x1b");
}

#[cfg(unix)]
#[test]
fn watch_quits_on_ctrl_c() {
    // A byte typed into raw mode, not a signal
    watch_quits_on("\x03");
}

#[test]
fn watch_shows_the_pid() {
    let p = Owned::listener();
    let pid = p.pid();
    let run = run_in_pty(
        &["watch", &pid],
        &[
            Step::Expect("Watching"),
            Step::Expect("listener"),
            Step::Send("q"),
        ],
    );
    assert_ok(&run);
    assert!(run.output.contains(&pid), "{}", run.output);
}

// ConPTY leaves a line that did not change out of a redraw, so a repeated
// frame is not there to wait for: Unix only
#[cfg(unix)]
#[test]
fn watch_redraws_every_interval() {
    let p = Owned::listener();
    let pid = p.pid();
    let run = run_in_pty(
        &["watch", &pid, "--interval", "0.2"],
        &[
            Step::Expect("Watching"),
            Step::Expect("Watching"),
            Step::Expect("Watching"),
            Step::Send("q"),
        ],
    );
    assert_ok(&run);
    assert!(run.output.contains("Refresh: 0.2s"), "{}", run.output);
    assert!(
        run.output.ends_with(LEAVE_ALT_SCREEN),
        "the screen is left last"
    );
}

#[cfg(unix)]
#[test]
fn watch_keeps_running_when_another_key_is_pressed() {
    let p = Owned::listener();
    let pid = p.pid();
    let run = run_in_pty(
        &["watch", &pid, "--interval", "0.2"],
        &[
            Step::Expect("Watching"),
            Step::Send("x\r"),
            // Another frame after the keys: it did not quit
            Step::Expect("Watching"),
            Step::Expect("Watching"),
            Step::Send("q"),
        ],
    );
    assert_ok(&run);
}

/// The same on Windows, where a second frame is not waited for (ConPTY leaves
/// what did not change out of a redraw, so there is no text that proves one):
/// a key that is not a quit key is sent, several intervals go by, and `watch` is
/// still there; only `q` ends it, and what it drew is the target
#[cfg(windows)]
#[test]
fn watch_ignores_other_keys_and_quits_on_q() {
    let p = Owned::listener();
    let pid = p.pid();
    let run = run_in_pty(
        &["watch", &pid, "--interval", "0.2"],
        &[
            Step::Expect("Watching"),
            Step::Send("x"),
            Step::Pause(Duration::from_millis(1000)),
            Step::StillRunning,
            Step::Send("q"),
        ],
    );
    assert_ok(&run);
    assert!(run.output.contains(&pid), "{}", run.output);
}

/// With no key pressed the loop waits out each interval and goes on; a few
/// intervals go by here, and `watch` is still running at the end of them
#[cfg(windows)]
#[test]
fn watch_goes_on_when_no_key_is_pressed() {
    let p = Owned::listener();
    let pid = p.pid();
    let run = run_in_pty(
        &["watch", &pid, "--interval", "0.2"],
        &[
            Step::Expect("Watching"),
            Step::Pause(Duration::from_millis(1000)),
            Step::StillRunning,
            Step::Send("q"),
        ],
    );
    assert_ok(&run);
    assert!(run.output.contains("Refresh: 0.2s"), "{}", run.output);
}

#[cfg(unix)]
#[test]
fn watch_draws_the_new_size_after_a_resize() {
    let p = Owned::listener();
    let pid = p.pid();
    let run = run_in_pty(
        &["watch", &pid, "--interval", "0.2"],
        &[
            Step::Expect("Watching"),
            Step::Expect("listener"),
            // Five rows leave none for processes: the one process is counted instead
            Step::Resize(120, 5),
            Step::Expect(" ... and 1 more (use --limit to control)"),
            // Back to a normal size, narrow enough to cut the command
            Step::Resize(80, 40),
            Step::Expect("listener"),
            Step::Send("q"),
        ],
    );
    assert_ok(&run);
    assert!(
        run.output.ends_with(LEAVE_ALT_SCREEN),
        "the screen is left last"
    );
}

#[test]
fn watch_says_when_nothing_matches() {
    // a name nothing has yet is watched until one appears
    let name = format!("pcnobody{}x", std::process::id());
    let run = run_in_pty(
        &["watch", &name],
        &[
            Step::Expect("No matching processes found."),
            Step::Send("q"),
        ],
    );
    assert_ok(&run);
    assert!(run.output.contains("0 processes"), "{}", run.output);
}

#[test]
fn watch_of_a_pid_nothing_holds_exits_before_taking_the_screen() {
    let run = run_in_pty(
        &["watch", "99999999"],
        &[Step::Expect("No process found matching '99999999'")],
    );
    assert_eq!(run.exit_code, 2, "{}", run.output);
    assert!(
        !run.output.contains("No matching processes found."),
        "{}",
        run.output
    );
}

#[test]
fn watch_shows_the_sort_in_its_header() {
    let p = Owned::listener();
    let pid = p.pid();
    let run = run_in_pty(
        &["watch", &pid, "--sort", "mem"],
        &[Step::Expect("Sort: Memory"), Step::Send("q")],
    );
    assert_ok(&run);
}

#[test]
fn watch_shows_the_same_columns_as_list() {
    let p = Owned::listener();
    let pid = p.pid();
    let run = run_in_pty(
        &["watch", &pid],
        &[
            Step::Expect("PID"),
            Step::Expect("NAME"),
            Step::Expect("USER"),
            Step::Expect("CPU%"),
            Step::Expect("MEM"),
            Step::Expect("UPTIME"),
            Step::Expect("COMMAND"),
            Step::Send("q"),
        ],
    );
    assert_ok(&run);
    assert!(!run.output.contains("ARGS"), "{}", run.output);
}

#[test]
fn watch_verbose_adds_the_columns_and_cuts_nothing() {
    let p = Owned::listener();
    let pid = p.pid();
    let dir = p.dir();
    let run = run_in_pty(
        &["watch", &pid, "--verbose"],
        &[
            Step::Expect("PPID"),
            Step::Expect("STATUS"),
            Step::Expect("DIR"),
            Step::Expect("COMMAND"),
            Step::Send("q"),
        ],
    );
    assert_ok(&run);
    // The whole directory is in the row. Windows may report it in its 8.3
    // short form, in any case: the path shown and the one the test made must
    // be the same directory
    #[cfg(unix)]
    assert!(run.output.contains(&dir), "{}", run.output);
    #[cfg(windows)]
    {
        let same_dir = |shown: &str| {
            let resolve = |path: &str| {
                std::fs::canonicalize(path)
                    .map(|p| strip_verbatim_prefix(&p.to_string_lossy()).to_lowercase())
            };
            matches!((resolve(shown), resolve(&dir)), (Ok(a), Ok(b)) if a == b)
        };
        let shown: Vec<&str> = run
            .output
            .split_whitespace()
            .filter(|word| word.contains(':') && word.contains('\\'))
            .collect();
        assert!(
            shown.iter().any(|path| same_dir(path)),
            "no cell names {dir}: {shown:?}"
        );
    }
    assert!(!run.output.contains("cwd: "), "{}", run.output);
}

// ---- signals and redirected output (Unix: they use a shell and signals) ----

/// The signals that end a program
#[cfg(unix)]
const END_SIGNALS: [(i32, &str); 4] = [
    (libc::SIGTERM, "SIGTERM"),
    (libc::SIGINT, "SIGINT"),
    (libc::SIGHUP, "SIGHUP"),
    (libc::SIGQUIT, "SIGQUIT"),
];

/// proc ended by `signal` itself, as a program without a handler would: a
/// shell then reports 128 plus its number and knows a signal ended proc, so
/// Ctrl-C stops a script's loop. An exit status of 130 only reads the same.
#[cfg(unix)]
fn assert_ended_by(run: &PtyRun, signal: i32, what: &str) {
    // SAFETY: strsignal returns a valid C string for a valid signal number
    let name = unsafe { std::ffi::CStr::from_ptr(libc::strsignal(signal)) }
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        run.ended_by.as_deref(),
        Some(name.as_str()),
        "{what}: exit {}: {}",
        run.exit_code,
        run.output
    );
}

/// Position of the last `needle` in what the terminal received
#[cfg(unix)]
fn last_at(raw: &str, needle: &str) -> Option<usize> {
    raw.rfind(needle)
}

/// The terminal got line editing and echo back, and the cursor was shown after
/// it was last hidden
#[cfg(unix)]
fn assert_terminal_back(run: &PtyRun, what: &str) {
    let raw = String::from_utf8_lossy(&run.raw);
    assert!(
        is_cooked(run.mode_at_end),
        "{what}: ICANON and ECHO are on again at the end: {:x?}\n{raw:?}",
        run.mode_at_end
    );
    let hidden = last_at(&raw, "\x1b[?25l");
    let shown = last_at(&raw, "\x1b[?25h");
    assert!(
        hidden.is_some() && shown > hidden,
        "{what}: the cursor is shown after it was hidden: {raw:?}"
    );
}

/// `proc <args>` with stdin and stderr on the pseudo-terminal and stdout sent
/// where the shell `redirect` says (`> file`, `| head -c0`). Like [`ask`], a
/// session that fails is started again for as long as [`BUSY_PATIENCE`]: a
/// busy process may not have looked stuck yet, and what proc says about it is
/// on the redirected stdout, out of the test's sight.
#[cfg(unix)]
fn ask_with_stdout(redirect: &str, args: &[&str], script: &[Step]) -> PtyRun {
    let command = format!("\"$@\" {redirect}");
    let mut shell = vec!["-c", command.as_str(), "sh", env!("CARGO_BIN_EXE_proc")];
    shell.extend_from_slice(args);
    let deadline = Instant::now() + BUSY_PATIENCE;
    loop {
        match try_run_program_in_pty("sh", &shell, script) {
            Ok(run) => return run,
            Err(_) if Instant::now() < deadline => {}
            Err(why) => panic!("{why}"),
        }
    }
}

#[cfg(unix)]
#[test]
fn watch_puts_the_terminal_back_when_a_signal_ends_it() {
    for (signal, name) in END_SIGNALS {
        let p = Owned::listener();
        let run = run_in_pty(
            &["watch", &p.pid()],
            &[Step::Expect("Watching"), Step::Raw, Step::Signal(signal)],
        );
        assert_ended_by(&run, signal, name);
        assert!(
            !is_cooked(run.mode_at_signal),
            "{name}: watch had turned raw mode on: {:x?}",
            run.mode_at_signal
        );
        let raw = String::from_utf8_lossy(&run.raw);
        let entered = last_at(&raw, "\x1b[?1049h");
        let left = last_at(&raw, LEAVE_ALT_SCREEN);
        assert!(
            entered.is_some() && left > entered,
            "{name}: the alternate screen is left: {raw:?}"
        );
        assert_terminal_back(&run, name);
    }
}

#[cfg(unix)]
#[test]
fn a_prompt_puts_the_terminal_back_when_a_signal_ends_it() {
    for (signal, name) in END_SIGNALS {
        let mut p = Owned::listener();
        let run = run_in_pty(
            &["kill", &p.pid()],
            &[
                Step::Expect("Kill 1 process"),
                Step::Raw,
                Step::Signal(signal),
            ],
        );
        assert_ended_by(&run, signal, name);
        assert!(
            !is_cooked(run.mode_at_signal),
            "{name}: the prompt had turned line editing and echo off: {:x?}",
            run.mode_at_signal
        );
        assert_terminal_back(&run, name);
        assert!(p.alive(), "{name}: the signal killed the target");
    }
}

#[cfg(unix)]
#[test]
fn ctrl_c_at_a_prompt_puts_the_terminal_back() {
    let mut p = Owned::listener();
    let run = run_in_pty(
        &["kill", &p.pid()],
        &[
            Step::Expect("Kill 1 process"),
            Step::Raw,
            Step::Send("\x03"),
        ],
    );
    // Ctrl+C typed at the prompt is a key, not a signal, and still ends proc
    // the way the signal would
    assert_ended_by(&run, libc::SIGINT, "Ctrl+C");
    assert_terminal_back(&run, "Ctrl+C");
    assert!(p.alive(), "Ctrl+C killed the target");
}

/// The list and the question reach the person even though stdout does not:
/// the list is on the terminal beside the question, and not in stdout
#[cfg(unix)]
fn assert_list_beside_the_question(run: &PtyRun, listed: &str, asked: &str, file: &PathBuf) {
    let list = run.output.find(listed);
    let question = run.output.find(asked);
    assert!(
        list.is_some() && question.is_some() && list < question,
        "the list ({listed:?}) comes before the question ({asked:?}):\n{}",
        run.output
    );
    let stdout = std::fs::read_to_string(file).unwrap_or_default();
    assert!(
        !stdout.contains(listed),
        "the list is not in stdout, which is the command's output:\n{stdout}"
    );
}

#[cfg(unix)]
#[test]
fn kill_shows_its_list_on_the_terminal_when_stdout_is_a_file() {
    let mut p = Owned::listener();
    let (_tmp, dir) = fresh_dir();
    let file = dir.join("stdout.txt");
    let redirect = format!("> '{}'", file.display());
    let run = ask_with_stdout(
        &redirect,
        &["kill", &p.pid()],
        &[
            Step::Expect("Found 1 process to kill"),
            Step::Expect("Kill 1 process"),
            Step::Send("n"),
        ],
    );
    assert_ok(&run);
    assert_list_beside_the_question(&run, "Found 1 process to kill", "Kill 1 process", &file);
    assert!(p.alive());
    // the answer to the question is the command's output, so it is in the file
    let stdout = std::fs::read_to_string(&file).unwrap();
    assert!(stdout.contains("Cancelled"), "{stdout}");
}

#[cfg(unix)]
#[test]
fn kill_shows_its_list_when_the_reader_of_stdout_has_left() {
    // `proc kill node | head -n1`: the reader is gone before it prints a thing
    let mut p = Owned::listener();
    let run = ask_with_stdout(
        "| true",
        &["kill", &p.pid()],
        &[
            Step::Expect("Found 1 process to kill"),
            Step::Expect("Kill 1 process"),
            Step::Send("n"),
        ],
    );
    assert_ok(&run);
    assert!(!run.output.contains("panicked"), "{}", run.output);
    assert!(p.alive());
}

#[cfg(unix)]
#[test]
fn free_shows_its_list_on_the_terminal_when_stdout_is_a_file() {
    let mut p = Owned::listener();
    let target = format!(":{}", p.port());
    let (_tmp, dir) = fresh_dir();
    let file = dir.join("stdout.txt");
    let redirect = format!("> '{}'", file.display());
    let run = ask_with_stdout(
        &redirect,
        &["free", &target],
        &[
            Step::Expect("Found 1 process to free"),
            Step::Expect("Kill 1 process to free 1 port"),
            Step::Send("n"),
        ],
    );
    assert_ok(&run);
    assert_list_beside_the_question(
        &run,
        "Found 1 process to free",
        "Kill 1 process to free 1 port",
        &file,
    );
    assert!(p.alive());
}

#[cfg(unix)]
#[test]
fn unstick_shows_its_list_on_the_terminal_when_stdout_is_a_file() {
    let mut p = Owned::busy();
    let dir = p.dir();
    let (_tmp, out_dir) = fresh_dir();
    let file = out_dir.join("stdout.txt");
    let redirect = format!("> '{}'", file.display());
    let run = ask_with_stdout(
        &redirect,
        &["unstick", "--force", "--timeout", "0", "--in", &dir],
        &[
            Step::Expect("Found stuck 1 process"),
            Step::Expect("Unstick 1 process"),
            Step::Send("n"),
        ],
    );
    assert_ok(&run);
    assert_list_beside_the_question(&run, "Found stuck 1 process", "Unstick 1 process", &file);
    assert!(p.alive());
}

#[cfg(unix)]
#[test]
fn stuck_shows_its_list_on_the_terminal_when_stdout_is_a_file() {
    let mut p = Owned::busy();
    let dir = p.dir();
    let (_tmp, out_dir) = fresh_dir();
    let file = out_dir.join("stdout.txt");
    let redirect = format!("> '{}'", file.display());
    let run = ask_with_stdout(
        &redirect,
        &["stuck", "--kill", "--timeout", "0", "--in", &dir],
        &[
            Step::Expect("CPU%"),
            Step::Expect("Kill 1 stuck process"),
            Step::Send("n"),
        ],
    );
    assert_ok(&run);
    assert_list_beside_the_question(&run, "CPU%", "Kill 1 stuck process", &file);
    assert!(p.alive());
}

#[cfg(unix)]
#[test]
fn orphans_shows_its_list_on_the_terminal_when_stdout_is_a_file() {
    let o = Orphan::start();
    let dir = o.dir();
    let (_tmp, out_dir) = fresh_dir();
    let file = out_dir.join("stdout.txt");
    let redirect = format!("> '{}'", file.display());
    let run = ask_with_stdout(
        &redirect,
        &["orphans", "--kill", "--in", &dir],
        &[
            Step::Expect("CPU%"),
            Step::Expect("Kill 1 orphaned process"),
            Step::Send("n"),
        ],
    );
    assert_ok(&run);
    assert_list_beside_the_question(&run, "CPU%", "Kill 1 orphaned process", &file);
    assert!(o.alive());
}

#[cfg(unix)]
#[test]
fn watch_without_a_terminal_on_stdin_shows_one_snapshot_and_says_so() {
    let p = Owned::listener();
    let pid = p.pid();
    let run = try_run_program_in_pty(
        "sh",
        &[
            "-c",
            "\"$@\" < /dev/null",
            "sh",
            env!("CARGO_BIN_EXE_proc"),
            "watch",
            &pid,
        ],
        &[
            Step::Expect("stdin is not a terminal"),
            Step::Expect("CPU%"),
        ],
    )
    .unwrap_or_else(|why| panic!("{why}"));
    assert_ok(&run);
    assert!(run.output.contains(&pid), "{}", run.output);
    let raw = String::from_utf8_lossy(&run.raw);
    assert!(
        !raw.contains("\x1b[?1049h"),
        "no alternate screen is drawn: {raw:?}"
    );
    assert!(
        !run.output.contains("Failed to initialize"),
        "{}",
        run.output
    );
}
