//! Holds a terminal's session for the pty tests (`tests/terminal.rs`,
//! `tests/human_output.rs`) and runs a program in it as the foreground job.
//!
//! portable-pty makes the program it starts the leader of the terminal's
//! session, and when a session leader exits, FreeBSD and NetBSD revoke its
//! terminal: what it wrote that the test had not read yet is gone (a leader
//! that wrote `hello` and exited left nothing to read, 3 runs of 3 on FreeBSD
//! 15.1, where a process that was not the leader left all of it), and a key
//! typed after that fails with EIO (NetBSD). So the tests start this instead,
//! and it does what a shell does: it keeps the session and runs
//! `session <program> [args...]` in a process group of its own, as the
//! terminal's foreground job, so keys, Ctrl+C and a resize reach the job
//! alone.
//!
//! `PROC_TEST_SESSION` names a directory the test owns. When the job ends,
//! this writes `status` there (`exited <code>` or `signaled <number>`) and
//! waits for the test to create `release`, so the test reads everything
//! first; then it ends the way the job did, with the same exit code or by the
//! same signal. `pid` there names the job, for a test that must kill it.
//! TERM, INT, HUP, QUIT, USR1 and USR2 sent to this process go to the job.
//!
//! Not proc's own code: it may use the std print macros that `clippy.toml`
//! bans in proc itself.

#![allow(clippy::disallowed_macros)]

#[cfg(unix)]
fn main() {
    unix::main();
}

#[cfg(not(unix))]
fn main() {
    eprintln!("session: Unix only");
    std::process::exit(2);
}

#[cfg(unix)]
mod unix {
    use std::ffi::{CString, OsString};
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::time::{Duration, Instant};

    /// Sent on to the job
    const FORWARDED: [libc::c_int; 6] = [
        libc::SIGTERM,
        libc::SIGINT,
        libc::SIGHUP,
        libc::SIGQUIT,
        libc::SIGUSR1,
        libc::SIGUSR2,
    ];

    /// The longest this waits for `release` after the job has ended
    const RELEASE_LIMIT: Duration = Duration::from_secs(60);

    /// The job's PID while it runs, 0 once it has been reaped
    static JOB: AtomicI32 = AtomicI32::new(0);

    extern "C" fn forward(signal: libc::c_int) {
        let job = JOB.load(Ordering::SeqCst);
        if job > 0 {
            // SAFETY: kill is async-signal-safe; `job` is our unreaped child
            unsafe {
                libc::kill(job, signal);
            }
        }
    }

    pub fn main() {
        let dir = PathBuf::from(
            std::env::var_os("PROC_TEST_SESSION")
                .expect("session: PROC_TEST_SESSION names the session's directory"),
        );
        let argv: Vec<CString> = std::env::args_os()
            .skip(1)
            .map(|arg: OsString| CString::new(arg.as_bytes()).expect("an argument without NUL"))
            .collect();
        assert!(!argv.is_empty(), "usage: session <program> [args...]");

        // Block the forwarded signals until their handlers are in place, so
        // one sent early is held, not taken by the default action
        let mut forwarded = empty_set();
        for signal in FORWARDED {
            // SAFETY: `forwarded` is an initialised set
            unsafe { libc::sigaddset(&mut forwarded, signal) };
        }
        let mut before = empty_set();
        // SAFETY: both sets are initialised
        unsafe { libc::sigprocmask(libc::SIG_BLOCK, &forwarded, &mut before) };

        // SAFETY: this program has one thread here, so the child may run any code
        let job = unsafe { libc::fork() };
        if job < 0 {
            panic!("session: fork: {}", std::io::Error::last_os_error());
        }
        if job == 0 {
            run_job(&argv, &before);
        }

        // As a shell does, from both sides, so neither waits on the other.
        // Taking the terminal from the background would stop this process, so
        // SIGTTOU is ignored here (the job's handlers are its own after exec).
        // SAFETY: plain calls on our own child and our own terminal (stdin)
        unsafe {
            libc::setpgid(job, job);
            libc::signal(libc::SIGTTOU, libc::SIG_IGN);
            libc::tcsetpgrp(0, job);
        }
        JOB.store(job, Ordering::SeqCst);
        for signal in FORWARDED {
            // SAFETY: `forward` only calls kill, which is async-signal-safe
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = forward as extern "C" fn(libc::c_int) as libc::sighandler_t;
                action.sa_flags = libc::SA_RESTART;
                libc::sigemptyset(&mut action.sa_mask);
                libc::sigaction(signal, &action, std::ptr::null_mut());
            }
        }
        // SAFETY: restores the mask saved above
        unsafe { libc::sigprocmask(libc::SIG_SETMASK, &before, std::ptr::null_mut()) };
        write_atomically(&dir.join("pid"), &job.to_string());

        let status = loop {
            let mut status = 0;
            // SAFETY: waits for our own child
            if unsafe { libc::waitpid(job, &mut status, 0) } == job {
                break status;
            }
            let error = std::io::Error::last_os_error();
            assert_eq!(
                error.kind(),
                std::io::ErrorKind::Interrupted,
                "session: waitpid: {error}"
            );
        };
        JOB.store(0, Ordering::SeqCst);

        let ended = if libc::WIFSIGNALED(status) {
            format!("signaled {}", libc::WTERMSIG(status))
        } else {
            format!("exited {}", libc::WEXITSTATUS(status))
        };
        write_atomically(&dir.join("status"), &ended);

        let release = dir.join("release");
        let until = Instant::now() + RELEASE_LIMIT;
        while !release.exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }

        if libc::WIFSIGNALED(status) {
            end_by(libc::WTERMSIG(status));
        }
        std::process::exit(libc::WEXITSTATUS(status));
    }

    /// The child: a process group of its own, the terminal's foreground, the
    /// signal mask it was born with, then the program
    fn run_job(argv: &[CString], mask: &libc::sigset_t) -> ! {
        // SAFETY: plain calls in the freshly forked child, before exec
        unsafe {
            libc::setpgid(0, 0);
            // the child is in the background until the terminal is its
            let mut ttou = empty_set();
            libc::sigaddset(&mut ttou, libc::SIGTTOU);
            libc::sigprocmask(libc::SIG_BLOCK, &ttou, std::ptr::null_mut());
            libc::tcsetpgrp(0, libc::getpid());
            libc::sigprocmask(libc::SIG_SETMASK, mask, std::ptr::null_mut());

            let mut pointers: Vec<*const libc::c_char> =
                argv.iter().map(|arg| arg.as_ptr()).collect();
            pointers.push(std::ptr::null());
            libc::execvp(pointers[0], pointers.as_ptr());
        }
        eprintln!(
            "session: cannot run {:?}: {}",
            argv[0],
            std::io::Error::last_os_error()
        );
        // SAFETY: ends the child without running the parent's exit handlers
        unsafe { libc::_exit(127) }
    }

    /// End by `signal`, as the job did, without leaving a core file
    fn end_by(signal: libc::c_int) -> ! {
        // SAFETY: plain calls on this process alone
        unsafe {
            let none = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            libc::setrlimit(libc::RLIMIT_CORE, &none);
            libc::signal(signal, libc::SIG_DFL);
            let mut only = empty_set();
            libc::sigaddset(&mut only, signal);
            libc::sigprocmask(libc::SIG_UNBLOCK, &only, std::ptr::null_mut());
            libc::kill(libc::getpid(), signal);
        }
        std::process::exit(128 + signal);
    }

    fn empty_set() -> libc::sigset_t {
        // SAFETY: sigemptyset initialises the whole set
        unsafe {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            set
        }
    }

    /// Written whole or not at all, so the test never reads half a file
    fn write_atomically(path: &Path, text: &str) {
        let partial = path.with_extension("partial");
        std::fs::write(&partial, text).expect("session: write in the session's directory");
        std::fs::rename(&partial, path).expect("session: rename in the session's directory");
    }
}
