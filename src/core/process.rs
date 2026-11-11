//! Cross-platform process abstraction
//!
//! Provides a unified interface for discovering and managing processes
//! across macOS, Linux, and Windows.

use crate::core::name_match::{classify, Hit, NameMatch, Names};
use crate::error::{ProcError, Result};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::Duration;
#[cfg(windows)]
use sysinfo::Users;

/// Runs `refresh`, a sysinfo process refresh, so that no two run at once.
///
/// On FreeBSD libkvm reads a process's command line and environment into a
/// buffer kept in static variables (`kvm_argv` in lib/libkvm/kvm_proc.c), one
/// for the whole program, so two threads refreshing at the same time read each
/// other's half-written arrays and crash (seen in the parallel test suite). A
/// program-wide lock makes that impossible. And libprocstat, which sysinfo asks
/// for each process's files, prints `kinfo_getfile(): No such process` to
/// stderr for a process that exits while it reads (lib/libprocstat/libprocstat.c),
/// so stderr points at /dev/null while the refresh runs. Elsewhere this only
/// calls `refresh`.
pub(crate) fn sysinfo_locked<T>(refresh: impl FnOnce() -> T) -> T {
    #[cfg(target_os = "freebsd")]
    {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _held = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let _quiet = QuietStderr::new();
        refresh()
    }
    #[cfg(not(target_os = "freebsd"))]
    refresh()
}

/// Stderr pointed at /dev/null until dropped (FreeBSD, see [`sysinfo_locked`]).
/// If either descriptor can't be had, stderr is left as it is.
#[cfg(target_os = "freebsd")]
struct QuietStderr {
    saved: Option<libc::c_int>,
}

#[cfg(target_os = "freebsd")]
impl QuietStderr {
    fn new() -> Self {
        // SAFETY: plain descriptor calls; every descriptor opened here is
        // closed, and stderr is restored in Drop
        unsafe {
            let saved = libc::dup(libc::STDERR_FILENO);
            if saved < 0 {
                return QuietStderr { saved: None };
            }
            let null = libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC);
            if null < 0 {
                libc::close(saved);
                return QuietStderr { saved: None };
            }
            libc::dup2(null, libc::STDERR_FILENO);
            libc::close(null);
            QuietStderr { saved: Some(saved) }
        }
    }
}

#[cfg(target_os = "freebsd")]
impl Drop for QuietStderr {
    fn drop(&mut self) {
        if let Some(saved) = self.saved {
            // SAFETY: `saved` is the descriptor `new` duplicated stderr into
            unsafe {
                libc::dup2(saved, libc::STDERR_FILENO);
                libc::close(saved);
            }
        }
    }
}

/// `pids` for a refresh of exactly those processes, each once: sysinfo takes
/// the second refresh of one PID in a list as the process having ended and
/// drops it, and a port's holders list a process once per socket (IPv4 and
/// IPv6), so a holder could go missing
fn each_once(pids: &[u32]) -> Vec<Pid> {
    let mut once = pids.to_vec();
    once.sort_unstable();
    once.dedup();
    once.into_iter().map(Pid::from_u32).collect()
}

/// Each running process's name, by PID, as [`Process`] records it (one refresh
/// of exactly those PIDs); a PID that is gone is missing from the map. A port
/// row takes its name from here, so `ports`, `on` and `by` all show one name
/// per process; the socket table's own name (lsof's, the kernel's, sockstat's
/// cut-short one) is only for a process that has already gone
pub(crate) fn names_by_pid(pids: &[u32]) -> std::collections::HashMap<u32, String> {
    let wanted = each_once(pids);
    let mut sys = System::new();
    sysinfo_locked(|| sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&wanted), true));
    sys.processes()
        .iter()
        .map(|(pid, proc)| (pid.as_u32(), Process::from_sysinfo(*pid, proc).name))
        .collect()
}

/// A snapshot of every process and of the system, as `System::new_all` and
/// `refresh_all` give it, taken under [`sysinfo_locked`]
pub(crate) fn refreshed_system() -> System {
    sysinfo_locked(|| {
        let mut sys = System::new_all();
        sys.refresh_all();
        sys
    })
}
use sysinfo::{Pid, ProcessStatus as SysProcessStatus, System, Uid};

/// Resolve a user ID to its user name, falling back to the ID itself.
///
/// On Unix this asks the system's user database (getpwuid_r), as `ps` and
/// `id` do: sysinfo's user list leaves out every account whose shell ends in
/// `/false` (macOS's daemon accounts, many service accounts) and every ID from
/// 65536 up (directory and container users). Each ID is looked up once per run.
#[cfg(unix)]
fn user_name(uid: &Uid) -> String {
    use std::collections::HashMap;
    use std::sync::Mutex;

    static NAMES: OnceLock<Mutex<HashMap<u32, String>>> = OnceLock::new();
    let id: u32 = **uid;
    let mut names = NAMES.get_or_init(Default::default).lock().unwrap();
    names
        .entry(id)
        .or_insert_with(|| {
            nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(id))
                .ok()
                .flatten()
                .map(|user| user.name)
                .unwrap_or_else(|| id.to_string())
        })
        .clone()
}

/// Resolve a user ID to its user name, falling back to the ID itself.
/// The user list is read once per run.
#[cfg(windows)]
fn user_name(uid: &Uid) -> String {
    static USERS: OnceLock<Users> = OnceLock::new();
    USERS
        .get_or_init(Users::new_with_refreshed_list)
        .get_user_by_id(uid)
        .map(|user| user.name().to_string())
        .unwrap_or_else(|| uid.to_string())
}

/// Make text from other processes safe to print: control characters become
/// visible escapes, as `lsof` and `ps` do. Names, command lines and paths are
/// set by whoever runs a process, and printed raw an ESC sequence could
/// recolour, retitle or rewrite the terminal of whoever runs proc (and a
/// newline would split a table row). Applied where process data is read, so
/// every view and the JSON show the same text.
///
/// Escaped: C0 controls and DEL (`\x1b`, with `\n`, `\r`, `\t` by name), C1
/// controls (`\x9b`), and bidirectional overrides that reorder text on screen
/// (`\u{202e}`). Everything else, non-ASCII included, is unchanged. Works on
/// chars, never byte offsets, so it can't split a character.
pub fn escape_control(text: &str) -> std::borrow::Cow<'_, str> {
    fn needs_escape(c: char) -> bool {
        c.is_control()
            || matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    }
    if !text.chars().any(needs_escape) {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
            c if needs_escape(c) => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    std::borrow::Cow::Owned(out)
}

/// `stuck` and `unstick` take this many readings, this far apart, and a process
/// must look stuck in every one
const STUCK_READINGS: usize = 2;
const STUCK_READING_GAP: Duration = Duration::from_secs(1);

/// Stuck in one reading: busy (over half a core), or in uninterruptible sleep,
/// blocked in the kernel (usually on I/O) where it can't even be interrupted
fn looks_stuck(cpu_percent: f32, status: SysProcessStatus) -> bool {
    cpu_percent > 50.0 || status == SysProcessStatus::UninterruptibleDiskSleep
}

/// CPU use in percent of one core: the CPU time a process used between two
/// reads ([`cpu_time_ms`]) over the wall time between them.
/// sysinfo's own `cpu_usage` can't be trusted across two reads: on Windows a
/// process's first update stores no baseline, so the next one divides its
/// lifetime CPU by all CPU time since boot and reads ~0% for a full core.
fn cpu_percent_between(used_before_ms: u64, used_after_ms: u64, wall: Duration) -> f32 {
    let wall_ms = wall.as_secs_f64() * 1000.0;
    if wall_ms <= 0.0 {
        return 0.0;
    }
    (used_after_ms.saturating_sub(used_before_ms) as f64 / wall_ms * 100.0) as f32
}

/// The error for finding orphans on Windows (shared by the library call and the command)
#[cfg(not(unix))]
pub(crate) fn orphans_not_supported() -> ProcError {
    ProcError::NotSupported(
        "orphans is not supported on Windows: Windows doesn't re-parent processes, \
         so there is no reliable way to tell an orphan"
            .to_string(),
    )
}

/// Why a command such as `taskkill` failed, in its own words: what it wrote to
/// stderr, else to stdout, else its exit status
/// (compiled on Unix for the tests only)
#[cfg(any(windows, test))]
fn command_failure(stdout: &[u8], stderr: &[u8], status: &str) -> String {
    [stderr, stdout]
        .iter()
        .map(|out| {
            // one line, whatever the tool wrapped its reason over
            String::from_utf8_lossy(out)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .find(|text| !text.is_empty())
        .unwrap_or_else(|| format!("exited with {status}"))
}

/// The result of asking sysinfo to kill process `pid`, which says whether the
/// kill was delivered
fn kill_outcome(delivered: bool, pid: u32) -> Result<()> {
    if delivered {
        Ok(())
    } else {
        Err(ProcError::SignalError(format!(
            "Failed to kill process {}",
            pid
        )))
    }
}

/// Whether the process holding a PID now (start time `current`, 0 when it can't
/// be read) is the one a record with start time `recorded` describes. Checked
/// right before each signal. A record that knows its start time is the same
/// process only if the current one reads within a second of it: an unreadable
/// start time on the PID's holder now is not proof, so it gets no signal (on
/// macOS that is another user's process that took the PID). A record without a
/// start time has nothing to compare and counts as the same.
fn same_start(recorded: Option<u64>, current: u64) -> bool {
    match recorded {
        Some(ours) => current > 0 && current.abs_diff(ours) <= 1,
        None => true,
    }
}

/// The program `find_by_open_file` asks which processes have a file open
/// (Linux reads `/proc` instead)
#[cfg(any(target_os = "freebsd", target_os = "netbsd"))]
pub(crate) const OPEN_FILE_PROGRAM: &str = "fstat";
#[cfg(all(
    unix,
    not(any(target_os = "freebsd", target_os = "netbsd", target_os = "linux"))
))]
pub(crate) const OPEN_FILE_PROGRAM: &str = "lsof";

/// The PIDs that `program` (`lsof`, or `fstat`) says have `path` open. A
/// program that can't be started is an error naming it: finding no holders
/// because nothing could look would be a silent wrong answer
#[cfg(all(unix, any(not(target_os = "linux"), test)))]
fn open_file_pids_with(program: &str, path: &std::path::Path) -> Result<Vec<u32>> {
    use std::process::Command;

    let mut command = Command::new(program);
    // -t = terse (PIDs only); fstat has no such option
    #[cfg(not(any(target_os = "freebsd", target_os = "netbsd")))]
    command.arg("-t");
    let output = crate::ui::interrupt::unblocked_in_child(&mut command)
        .arg(&*path.to_string_lossy())
        .output()
        .map_err(|e| {
            ProcError::NotSupported(format!(
                "proc reads open files with {program}, which could not be run ({e})"
            ))
        })?;

    if !output.status.success() {
        return Ok(vec![]); // No processes have file open
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    #[cfg(any(target_os = "freebsd", target_os = "netbsd"))]
    let pids = fstat_pids(&stdout);
    #[cfg(not(any(target_os = "freebsd", target_os = "netbsd")))]
    let pids = lsof_pids(&stdout);
    Ok(pids)
}

/// The PIDs whose entry under `proc_root` (`/proc`) shows `path` in use the
/// ways lsof reports it: an open file descriptor, the working directory, the
/// root directory, the executable, or a file mapped into memory (a shared
/// library). An entry proc may not read (another user's process, without
/// root) is left out, as lsof leaves it out. PID ascending. A `proc_root`
/// that can't be read is an error, so a system without `/proc` doesn't read
/// as one where nothing holds the file
#[cfg(all(unix, any(target_os = "linux", test)))]
fn linux_open_file_holders(
    proc_root: &std::path::Path,
    path: &std::path::Path,
) -> Result<Vec<u32>> {
    let entries = std::fs::read_dir(proc_root)
        .map_err(|e| ProcError::SystemError(format!("{}: {e}", proc_root.display())))?;
    let mut pids: Vec<u32> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            holds_path(&entry.path(), path).then_some(pid)
        })
        .collect();
    pids.sort_unstable();
    Ok(pids)
}

/// Whether the `/proc/<pid>` entry `entry` shows `path` in use (see
/// `linux_open_file_holders`)
#[cfg(all(unix, any(target_os = "linux", test)))]
fn holds_path(entry: &std::path::Path, path: &std::path::Path) -> bool {
    let links_to = |link: std::path::PathBuf| std::fs::read_link(link).is_ok_and(|to| to == path);
    ["cwd", "root", "exe"]
        .iter()
        .any(|name| links_to(entry.join(name)))
        || std::fs::read_dir(entry.join("fd"))
            .is_ok_and(|fds| fds.flatten().any(|fd| links_to(fd.path())))
        || std::fs::read_to_string(entry.join("maps"))
            .is_ok_and(|maps| maps.lines().any(|line| mapped_path(line) == Some(path)))
}

/// The file a line of `/proc/<pid>/maps` maps, if it maps one:
/// `address perms offset dev inode path`, the path last and free to hold
/// spaces, absent for anonymous memory and `[heap]`-style names
#[cfg(all(unix, any(target_os = "linux", test)))]
fn mapped_path(line: &str) -> Option<&std::path::Path> {
    let mut rest = line;
    for _ in 0..5 {
        rest = rest.trim_start();
        rest = &rest[rest.find(' ')?..];
    }
    let mapped = rest.trim_start();
    mapped
        .starts_with('/')
        .then(|| std::path::Path::new(mapped))
}

/// The PIDs `lsof -t` printed, one per line
#[cfg(any(
    all(
        unix,
        not(any(target_os = "freebsd", target_os = "netbsd", target_os = "linux"))
    ),
    test
))]
fn lsof_pids(stdout: &str) -> Vec<u32> {
    stdout
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// The PIDs in what `fstat FILE` printed (FreeBSD and NetBSD): a header, then
/// one row per open file, `USER CMD PID FD MOUNT ... NAME`, so a process that
/// holds the file twice is listed twice. A command name can hold spaces and
/// digits ("Web 4021" is a valid executable name), so the first number is not
/// always the PID. The mount point always starts with `/`, and neither a user
/// name nor a command name can hold one (the kernel takes the command name
/// from the executable's file name): the PID is two fields before the first
/// field that starts with `/`, after at least one command-name field. A row
/// that doesn't fit is left out, so an odd row misses a holder, never names
/// another process. The header has no `/`
#[cfg(any(target_os = "freebsd", target_os = "netbsd", test))]
fn fstat_pids(stdout: &str) -> Vec<u32> {
    let mut pids: Vec<u32> = stdout
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let mount = fields.iter().position(|field| field.starts_with('/'))?;
            // USER, at least one CMD field, PID, FD, then the mount point
            let pid = fields.get(mount.checked_sub(2).filter(|&at| at >= 2)?)?;
            pid.bytes()
                .all(|b| b.is_ascii_digit())
                .then(|| pid.parse().ok())?
        })
        .collect();
    pids.sort_unstable();
    pids.dedup();
    pids
}

/// How many times a same-process check reads a process that reports no start time
const START_TIME_READS: u32 = 10;

/// The wait between those reads
const START_TIME_READ_GAP: Duration = Duration::from_millis(2);

/// Runs `read` (one refresh of a process: the start time it read, None if the
/// process is gone) once, and again while it reads `Some(0)`, up to
/// [`START_TIME_READS`] times in all; only once unless `wants_start`, since a
/// record without a start time compares none. NetBSD leaves the start time and
/// the CPU time out of a process's record (p_uvalid 0) for a few hundred
/// microseconds at a time while the process takes a signal or execs: in each
/// of 10 rounds, reads in the 2 ms after SIGCONT found such a record, between
/// reads with the real values. [`same_start`] takes 0 for another process, and
/// a CPU time of 0 makes the next reading's look like the process's whole
/// lifetime. Where 0 lasts (another user's process on macOS) the check still
/// fails closed, 20 ms later.
fn read_until_started(wants_start: bool, mut read: impl FnMut() -> Option<u64>) {
    let mut reads = 1;
    while read() == Some(0) && wants_start && reads < START_TIME_READS {
        std::thread::sleep(START_TIME_READ_GAP);
        reads += 1;
    }
}

/// What a `taskkill` run (no `/F`) came to: success, or the reason it gave
/// (compiled on Unix for the tests only)
#[cfg(any(windows, test))]
fn taskkill_outcome(output: &std::process::Output) -> Result<()> {
    if output.status.success() {
        Ok(())
    } else {
        Err(ProcError::SignalError(command_failure(
            &output.stdout,
            &output.stderr,
            &output.status.to_string(),
        )))
    }
}

/// The CPU time each process in `sys` has used so far, in ms
fn cpu_used(sys: &System) -> std::collections::HashMap<Pid, u64> {
    sys.processes()
        .iter()
        .map(|(pid, p)| (*pid, cpu_time_ms(*pid, p)))
        .collect()
}

/// The CPU time process `pid` has used so far, in ms. On NetBSD sysinfo counts
/// it in hundredths of a second and rounds it to whole seconds, so over a 0.2 s
/// window a process spinning at 100% read 0%: there it is the kernel's own
/// counter, which `ps` shows, read right after the refresh that found `process`.
/// 0 once the process is gone, so a window it ended in reads no CPU.
fn cpu_time_ms(pid: Pid, process: &sysinfo::Process) -> u64 {
    #[cfg(target_os = "netbsd")]
    {
        let _ = process;
        netbsd_cpu_time_ms(pid.as_u32()).unwrap_or(0)
    }
    #[cfg(not(target_os = "netbsd"))]
    {
        let _ = pid;
        process.accumulated_cpu_time()
    }
}

/// NetBSD: the CPU time a process has used (p_rtime), from its kinfo_proc2;
/// None if the kernel has no such process. The kernel leaves the CPU time out
/// with the start time (p_uvalid 0, both 0) for a moment while the process
/// takes a signal, so this reads again as [`read_until_started`] does
#[cfg(target_os = "netbsd")]
fn netbsd_cpu_time_ms(pid: u32) -> Option<u64> {
    let mut last = None;
    read_until_started(true, || {
        last = netbsd_kinfo(pid);
        last.map(|info| u64::from(info.p_ustart_sec))
    });
    last.filter(|info| info.p_uvalid != 0)
        .map(|info| u64::from(info.p_rtime_sec) * 1000 + u64::from(info.p_rtime_usec) / 1000)
}

/// The name to show for a process sysinfo named after its argv[0]: that name,
/// unless argv[0] was rewritten into a title (it has a space, or starts with
/// `-` as a login shell's does), when the kernel's own name for the process
/// (`kernel_name`, the one `ps -o comm` shows) is the program's. Linux and
/// macOS take the name from the kernel already
#[cfg(any(target_os = "freebsd", target_os = "netbsd", test))]
fn kernel_name_if_retitled(name: String, kernel_name: impl FnOnce() -> Option<String>) -> String {
    if !(name.contains(char::is_whitespace) || name.starts_with('-')) {
        return name;
    }
    kernel_name()
        .filter(|kernel| !kernel.is_empty())
        .unwrap_or(name)
}

/// The text of a NUL-terminated C string held in a fixed array, such as the
/// kernel's command name in a process record
#[cfg(any(target_os = "freebsd", target_os = "netbsd", all(test, unix)))]
fn c_array_text(chars: &[libc::c_char]) -> String {
    let bytes: Vec<u8> = chars
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The BSDs: the kernel's name for process `pid` (ki_comm on FreeBSD, p_comm
/// on NetBSD); None if the kernel has no such process
#[cfg(target_os = "freebsd")]
fn bsd_comm(pid: u32) -> Option<String> {
    let size = std::mem::size_of::<libc::kinfo_proc>();
    let mib = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_PID,
        pid as libc::c_int,
    ];
    // SAFETY: kinfo_proc is plain integers, pointers and arrays, so all zeroes
    // is a valid value
    let mut info: libc::kinfo_proc = unsafe { std::mem::zeroed() };
    let mut len = size;
    // SAFETY: `mib` holds its 4 names, and sysctl writes at most `len` bytes,
    // the size of `info`, into it
    let read = unsafe {
        libc::sysctl(
            mib.as_ptr(),
            mib.len() as libc::c_uint,
            (&mut info as *mut libc::kinfo_proc).cast(),
            &mut len,
            std::ptr::null(),
            0,
        )
    };
    (read == 0 && len == size).then(|| c_array_text(&info.ki_comm))
}

/// The BSDs: the kernel's name for process `pid` (ki_comm on FreeBSD, p_comm
/// on NetBSD); None if the kernel has no such process
#[cfg(target_os = "netbsd")]
fn bsd_comm(pid: u32) -> Option<String> {
    netbsd_kinfo(pid).map(|info| c_array_text(&info.p_comm))
}

/// NetBSD: the kernel's kinfo_proc2 record of process `pid`; None if there is none
#[cfg(target_os = "netbsd")]
fn netbsd_kinfo(pid: u32) -> Option<libc::kinfo_proc2> {
    let size = std::mem::size_of::<libc::kinfo_proc2>();
    let mib = [
        libc::CTL_KERN,
        libc::KERN_PROC2,
        libc::KERN_PROC_PID,
        pid as libc::c_int,
        size as libc::c_int,
        1,
    ];
    // SAFETY: kinfo_proc2 is plain integers and arrays, so all zeroes is a valid value
    let mut info: libc::kinfo_proc2 = unsafe { std::mem::zeroed() };
    let mut len = size;
    // SAFETY: `mib` holds its 6 names, and sysctl writes at most `len` bytes,
    // the size of `info`, into it
    let read = unsafe {
        libc::sysctl(
            mib.as_ptr(),
            mib.len() as libc::c_uint,
            (&mut info as *mut libc::kinfo_proc2).cast(),
            &mut len,
            std::ptr::null(),
            0,
        )
    };
    (read == 0 && len == size).then_some(info)
}

/// Whether a process has been running for `timeout_secs`. A process's age is in whole
/// seconds, so a process under a second old reads 0 and must still count for
/// `--timeout 0`: "longer than" means this many seconds or more.
fn ran_long_enough(run_time: u64, timeout_secs: u64) -> bool {
    run_time >= timeout_secs
}

/// Whether one reading of a process counts toward "stuck": a process, not a
/// thread, that has run for `timeout_secs` and looks stuck. `find_stuck` filters
/// through this, so a test of it guards the comparison where it is used.
fn counts_as_stuck(is_thread: bool, run_time: u64, timeout_secs: u64, looks_stuck: bool) -> bool {
    !is_thread && ran_long_enough(run_time, timeout_secs) && looks_stuck
}

/// macOS: sysinfo drops zombies, because they have no executable path or arguments
/// left to read, so `proc list --status zombie` could never find one. Read every
/// process from the kernel, as `ps` does, and keep the zombies sysinfo didn't return.
#[cfg(target_os = "macos")]
fn macos_zombies(known: &std::collections::HashSet<u32>) -> Vec<Process> {
    zombies_in(&kernel_table(libc::KERN_PROC_ALL, 0), known)
}

/// macOS: the zombie with this PID, if there is one. sysinfo drops a zombie
/// from a refresh of one PID as it does from the whole table (it has no
/// executable path or arguments left to read), so a lookup that finds nothing
/// asks the kernel for just this PID, as `ps -p` does.
#[cfg(target_os = "macos")]
fn macos_zombie(pid: u32) -> Option<Process> {
    let table = kernel_table(libc::KERN_PROC_PID, pid as libc::c_int);
    zombies_in(&table, &std::collections::HashSet::new())
        .into_iter()
        .find(|zombie| zombie.pid == pid)
}

/// The kernel's process table as `read_proc_table` reads it: `which` says
/// which processes (`KERN_PROC_ALL`, or `KERN_PROC_PID` with `arg` the PID)
#[cfg(target_os = "macos")]
fn kernel_table(which: libc::c_int, arg: libc::c_int) -> Vec<u8> {
    read_proc_table(|buf, size| {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROC, which, arg];
        let null = std::ptr::null_mut();
        // SAFETY: `buf` is null (report the size needed) or points to `*size`
        // writable bytes, which is how read_proc_table calls it
        unsafe { libc::sysctl(mib.as_mut_ptr(), 4, buf, size, null, 0) }
    })
}

/// The zombies the OS has that sysinfo's table leaves out and `known` does not
/// list: macOS only (see [`macos_zombies`]), none elsewhere, where sysinfo
/// lists them. For a view built straight from a sysinfo table.
pub(crate) fn unlisted_zombies(known: &std::collections::HashSet<u32>) -> Vec<Process> {
    #[cfg(target_os = "macos")]
    return macos_zombies(known);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = known;
        Vec::new()
    }
}

/// The zombie with this PID that sysinfo's table leaves out: macOS only (see
/// [`macos_zombie`]), none elsewhere
pub(crate) fn unlisted_zombie(pid: u32) -> Option<Process> {
    #[cfg(target_os = "macos")]
    return macos_zombie(pid);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        None
    }
}

/// libc has no kinfo_proc for macOS, so the few fields we need are read at their
/// offsets in the 64-bit layout (checked against a live process's known values)
#[cfg(target_os = "macos")]
const KINFO_PROC_SIZE: usize = 648;

/// The kernel's table of every process, as raw kinfo_proc records; empty if the
/// kernel refuses. `sysctl` is the call to make (a buffer or null, and the size
/// in and out), so a test can make it fail.
#[cfg(target_os = "macos")]
fn read_proc_table(
    sysctl: impl Fn(*mut libc::c_void, &mut libc::size_t) -> libc::c_int,
) -> Vec<u8> {
    let mut size: libc::size_t = 0;
    // the first call only reports the size needed
    if sysctl(std::ptr::null_mut(), &mut size) != 0 {
        return Vec::new();
    }
    // room for processes started in between
    size += 64 * KINFO_PROC_SIZE;
    let mut buf: Vec<u8> = vec![0; size];
    if sysctl(buf.as_mut_ptr().cast(), &mut size) != 0 {
        return Vec::new();
    }
    buf.truncate(size);
    buf
}

/// The zombies in a table read by `read_proc_table` that `known` doesn't list
#[cfg(target_os = "macos")]
fn zombies_in(buf: &[u8], known: &std::collections::HashSet<u32>) -> Vec<Process> {
    const P_STARTTIME_SEC: usize = 0;
    const P_STAT: usize = 36;
    const P_PID: usize = 40;
    const P_COMM: usize = 243; // MAXCOMLEN + 1 bytes
    const P_RUID: usize = 392;
    const E_PPID: usize = 560;
    const SZOMB: u8 = 5;

    if !buf.len().is_multiple_of(KINFO_PROC_SIZE) {
        return Vec::new(); // a layout we don't know; better to show nothing than garbage
    }

    let read_u32 = |p: &[u8], at: usize| u32::from_ne_bytes(p[at..at + 4].try_into().unwrap());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    buf.as_chunks::<KINFO_PROC_SIZE>()
        .0
        .iter()
        .filter(|p| p[P_STAT] == SZOMB)
        .filter_map(|p| {
            let pid = read_u32(p, P_PID);
            if pid == 0 || known.contains(&pid) {
                return None;
            }
            let comm = &p[P_COMM..P_COMM + 17];
            let comm = &comm[..comm.iter().position(|&c| c == 0).unwrap_or(comm.len())];
            let name = escape_control(&String::from_utf8_lossy(comm)).into_owned();
            let start =
                u64::from_ne_bytes(p[P_STARTTIME_SEC..P_STARTTIME_SEC + 8].try_into().unwrap());
            Some(Process {
                pid,
                name: if name.is_empty() {
                    "<defunct>".to_string()
                } else {
                    name
                },
                exe_path: None,
                cwd: None,
                command: None,
                argv0: None,
                raw: None,
                cpu_percent: 0.0,
                memory_mb: 0.0,
                status: ProcessStatus::Zombie,
                user: Uid::try_from(read_u32(p, P_RUID) as usize)
                    .ok()
                    .map(|uid| escape_control(&user_name(&uid)).into_owned()),
                parent_pid: Some(read_u32(p, E_PPID)),
                start_time: (start > 0).then_some(start),
                uptime_secs: (start > 0).then(|| now.saturating_sub(start)),
                parent_name: None,
                cpu_time_secs: None,
                open_files: None,
                child_count: None,
                runs_proc: false,
            })
        })
        .collect()
}

/// On Linux, sysinfo lists every thread of a process as an entry of its own,
/// with the process's command line, so a node server shows up a dozen times.
/// Process listings skip them; `thread_kind` is always `None` elsewhere.
pub fn is_thread(proc: &sysinfo::Process) -> bool {
    proc.thread_kind().is_some()
}

/// Linux reports a working directory that was deleted as "/path (deleted)".
/// Strip that marker so the cwd matches `--in` and reads the same as on macOS;
/// a real directory with that name still exists, so it is kept as is.
fn strip_deleted_marker(cwd: String) -> String {
    match cwd.strip_suffix(" (deleted)") {
        Some(path) if !std::path::Path::new(&cwd).exists() => path.to_string(),
        _ => cwd,
    }
}

/// Process status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessStatus {
    /// Process is actively executing on CPU
    Running,
    /// Process is waiting for an event or resource
    Sleeping,
    /// Process has been stopped (e.g., by SIGSTOP)
    Stopped,
    /// Process has terminated but not yet been reaped by parent
    Zombie,
    /// Process is being terminated
    Dead,
    /// Linux only: an idle kernel thread (state `I` in `/proc/<pid>/stat`), which has
    /// nothing to run. Before 2.3.0 this was `unknown`.
    Idle,
    /// macOS, FreeBSD and NetBSD: the process is being created by `fork` and is not
    /// running yet (`SIDL`). Before 2.3.0 this was `unknown`.
    Starting,
    /// Linux only: uninterruptible sleep, waiting on the disk or other I/O that a signal
    /// cannot cut short (state `D`). Before 2.3.0 this was `unknown`.
    #[serde(rename = "disk_wait")]
    DiskWait,
    /// Process status could not be determined
    Unknown,
}

impl ProcessStatus {
    /// The status as a person reads it: the word the card shows after `Status:`,
    /// lowercase (a table capitalises it). JSON spells it with `_` where this has a space.
    pub(crate) fn word(self) -> &'static str {
        match self {
            ProcessStatus::Running => "running",
            ProcessStatus::Sleeping => "sleeping",
            ProcessStatus::Stopped => "stopped",
            ProcessStatus::Zombie => "zombie",
            ProcessStatus::Dead => "dead",
            ProcessStatus::Idle => "idle",
            ProcessStatus::Starting => "starting",
            ProcessStatus::DiskWait => "disk wait",
            ProcessStatus::Unknown => "unknown",
        }
    }

    /// What sysinfo's `Idle` means on the OS named (`std::env::consts::OS`): an idle
    /// kernel thread on Linux, a process still being created by `fork` (`SIDL`) on
    /// macOS, FreeBSD and NetBSD, and nothing on any other OS (sysinfo does not give it
    /// there).
    fn of_sysinfo_idle(os: &str) -> ProcessStatus {
        match os {
            "linux" | "android" => ProcessStatus::Idle,
            "macos" | "freebsd" | "netbsd" => ProcessStatus::Starting,
            _ => ProcessStatus::Unknown,
        }
    }

    /// The status for a sysinfo one on the OS named (`std::env::consts::OS`)
    fn from_sysinfo_on(status: SysProcessStatus, os: &str) -> ProcessStatus {
        match status {
            SysProcessStatus::Run => ProcessStatus::Running,
            SysProcessStatus::Sleep => ProcessStatus::Sleeping,
            SysProcessStatus::Stop => ProcessStatus::Stopped,
            SysProcessStatus::Zombie => ProcessStatus::Zombie,
            SysProcessStatus::Dead => ProcessStatus::Dead,
            SysProcessStatus::Idle => ProcessStatus::of_sysinfo_idle(os),
            SysProcessStatus::UninterruptibleDiskSleep => ProcessStatus::DiskWait,
            _ => ProcessStatus::Unknown,
        }
    }
}

impl From<SysProcessStatus> for ProcessStatus {
    fn from(status: SysProcessStatus) -> Self {
        ProcessStatus::from_sysinfo_on(status, std::env::consts::OS)
    }
}

/// Represents a system process with relevant information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Process {
    /// Process ID
    pub pid: u32,
    /// Process name (executable name)
    pub name: String,
    /// Path to the executable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exe_path: Option<String>,
    /// Current working directory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Full command line (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// The first word of the command line, kept whole (a path with spaces in
    /// it stays one piece) and exactly as the OS gave it, for matching names;
    /// never printed, and not part of the JSON
    #[serde(skip)]
    pub argv0: Option<String>,
    /// The name, executable and command line exactly as the OS gave them, when
    /// escaping control characters for display changed any of them. Matching
    /// reads these, so a name matches the same way whether it is looked up in
    /// the process list or against a record; `None` when nothing was escaped
    /// (the fields above are then the raw text). Not part of the JSON.
    #[serde(skip)]
    pub raw: Option<Box<RawText>>,
    /// CPU usage percentage (0.0 - 100.0+)
    pub cpu_percent: f32,
    /// Memory usage in megabytes
    pub memory_mb: f64,
    /// Process status
    pub status: ProcessStatus,
    /// User who owns the process
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Parent process ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_pid: Option<u32>,
    /// Process start time (Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<u64>,
    /// Seconds the process has been running
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_secs: Option<u64>,
    /// The name of the parent process, as `name` records it; absent where the
    /// record was made without the process table, or when the parent is gone
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_name: Option<String>,
    /// Whole seconds of CPU time the process has used so far; absent where
    /// the OS gives none
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_time_secs: Option<u64>,
    /// How many files the process has open; absent where the OS doesn't say
    /// (Windows, NetBSD without procfs, another user's process on macOS)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_files: Option<u64>,
    /// How many processes have this one as their parent; absent where the
    /// record was made without the process table
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child_count: Option<u32>,
    /// This process is one proc runs under: its parent, or a parent of that,
    /// up to 10 levels (the shell or agent that ran proc). In the JSON only
    /// when true
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub runs_proc: bool,
}

/// A process proc runs under, as a name lookup reports one it left out
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Ancestor {
    /// Its PID
    pub pid: u32,
    /// Its name, as the process record has it (escaped for display)
    pub name: String,
}

impl From<&Process> for Ancestor {
    fn from(process: &Process) -> Self {
        Ancestor {
            pid: process.pid,
            name: process.name.clone(),
        }
    }
}

/// Processes as a message names them: `zsh (PID 11849), zsh (PID 81340)`
pub(crate) fn ancestor_list(ancestors: &[Ancestor]) -> String {
    ancestors
        .iter()
        .map(|a| format!("{} (PID {})", a.name, a.pid))
        .collect::<Vec<_>>()
        .join(", ")
}

/// What a name lookup does with a process proc runs under that matches
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ancestors {
    /// Keep it, marked `runs_proc`, when its name matches exactly (commands
    /// that only read); a match in its command line, which holds proc's own
    /// arguments, is left out
    Mark,
    /// Leave it out (commands that act on processes, and `wait`)
    LeaveOut,
}

/// The processes a name matched, and the ones proc runs under that it left out
#[derive(Debug, Clone, Default)]
pub struct NameLookup {
    /// The processes found, PID ascending
    pub processes: Vec<Process>,
    /// The processes proc runs under that matched and were left out, PID ascending
    pub left_out: Vec<Ancestor>,
}

impl NameLookup {
    /// The processes a PID or port target found, with nothing left out
    pub(crate) fn of(processes: Vec<Process>) -> Self {
        NameLookup {
            processes,
            left_out: Vec::new(),
        }
    }
}

/// Whether `pattern` matches `process` by its name, executable or argv[0],
/// leaving the rest of its command line out: one of them exactly, or (with
/// `--cmd`) a part of the name. A process proc runs under has proc's own
/// arguments in its command line (`sh -c 'proc by --cmd X'`), so a `--cmd`
/// hit there alone is no match at all
pub(crate) fn matches_outside_command_line(
    process: &Process,
    pattern: &str,
    mode: NameMatch,
) -> bool {
    let names = process.names();
    let own = Names {
        command: None,
        ..names
    };
    classify(pattern, &names, NameMatch::Exact) == Hit::Match
        || classify(pattern, &own, mode) == Hit::Match
}

/// The error [`Process::find_by_name`] has always given: a name whose matches
/// were all processes proc runs under is not found there
fn as_before(error: ProcError, pattern: &str) -> ProcError {
    match error {
        ProcError::AncestorsOnly { .. } => ProcError::name_not_found(pattern, 0),
        error => error,
    }
}

/// Split the matches of a name: a process in `ancestors` (proc runs under it)
/// stays, marked `runs_proc`, only when `policy` is [`Ancestors::Mark`] and the
/// name matched exactly; otherwise it is left out. Every other process stays as
/// it is. Both lists keep the order of `matches`
pub(crate) fn split_ancestors(
    matches: Vec<Process>,
    ancestors: &std::collections::HashSet<u32>,
    mode: NameMatch,
    policy: Ancestors,
) -> NameLookup {
    let keep = policy == Ancestors::Mark && mode == NameMatch::Exact;
    let mut lookup = NameLookup::default();
    for mut process in matches {
        if !ancestors.contains(&process.pid) {
            lookup.processes.push(process);
        } else if keep {
            process.runs_proc = true;
            lookup.processes.push(process);
        } else {
            lookup.left_out.push(Ancestor::from(&process));
        }
    }
    lookup
}

/// The PIDs of the processes proc runs under (not proc itself), up to 10
/// levels, read once per run: they don't change while proc runs
pub(crate) fn proc_ancestors() -> &'static std::collections::HashSet<u32> {
    static ANCESTORS: OnceLock<std::collections::HashSet<u32>> = OnceLock::new();
    ANCESTORS.get_or_init(|| {
        let me = std::process::id();
        let mut ancestors = std::collections::HashSet::new();
        let mut current = Pid::from_u32(me);
        let mut sys = System::new();
        for _ in 0..10 {
            sysinfo_locked(|| {
                sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[current]), true)
            });
            match sys.process(current).and_then(|p| p.parent()) {
                Some(parent) if parent.as_u32() != me && ancestors.insert(parent.as_u32()) => {
                    current = parent;
                }
                _ => break,
            }
        }
        ancestors
    })
}

/// The fields of a [`Process`] that the cut-down JSON objects of `tree`, `why`,
/// `wait` and `unstick` do not carry under their own keys. Each of those
/// objects flattens this in after its own keys, so the names, types and
/// absent-when-unknown rule are the same in all four.
#[derive(Serialize, Clone)]
pub(crate) struct ProcessDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exe_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_time_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_files: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child_count: Option<u32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub runs_proc: bool,
}

impl ProcessDetails {
    /// The fields of `p` that a cut-down object does not carry
    pub(crate) fn of(p: &Process) -> Self {
        Self {
            exe_path: p.exe_path.clone(),
            user: p.user.clone(),
            parent_pid: p.parent_pid,
            start_time: p.start_time,
            parent_name: p.parent_name.clone(),
            cpu_time_secs: p.cpu_time_secs,
            open_files: p.open_files,
            child_count: p.child_count,
            runs_proc: p.runs_proc,
        }
    }
}

/// This process and its ancestors, as far up as ten levels (proc → shell →
/// shell wrapper → IDE → ...). A name lookup skips them all: their command
/// lines hold proc's own arguments (`zsh -c 'proc by node'`).
pub(crate) fn self_and_ancestors(sys: &System) -> std::collections::HashSet<Pid> {
    let self_pid = Pid::from_u32(std::process::id());
    let mut pids = std::collections::HashSet::new();
    pids.insert(self_pid);
    let mut current = self_pid;
    for _ in 0..10 {
        if let Some(parent) = sys.process(current).and_then(|p| p.parent()) {
            pids.insert(parent);
            current = parent;
        } else {
            break;
        }
    }
    pids
}

/// The text of a process that name matching reads, as the OS gave it: control
/// characters unescaped, unlike the fields a process is displayed from
#[derive(Debug, Clone)]
pub struct RawText {
    /// The process name
    pub name: String,
    /// Path of the executable
    pub exe_path: Option<String>,
    /// The full command line
    pub command: Option<String>,
    /// The working directory
    pub cwd: Option<String>,
}

/// The name proc gives a process, as the OS gave it (not escaped): sysinfo's,
/// except on the BSDs, where sysinfo takes it from argv[0] and a daemon can
/// have rewritten that into a title (sshd's `sshd: /usr/sbin/sshd [listener] 0
/// of 10-100 startups` read `sshd [listener] 0 of 10-100 startups`); there the
/// kernel's name stands in (see [`kernel_name_if_retitled`]). Every view and
/// every name match reads it here, so they agree
fn os_name(proc: &sysinfo::Process) -> std::borrow::Cow<'_, str> {
    let name = proc.name().to_string_lossy();
    #[cfg(any(target_os = "freebsd", target_os = "netbsd"))]
    let name = kernel_name_if_retitled(name.into_owned(), || bsd_comm(proc.pid().as_u32())).into();
    name
}

/// How a name relates to a process sysinfo listed, under `mode`. Reads the
/// name, executable and command line straight from sysinfo, so no record is
/// built for the many processes that don't match.
pub(crate) fn classify_sysinfo(pattern: &str, proc: &sysinfo::Process, mode: NameMatch) -> Hit {
    let name = os_name(proc);
    let exe = proc.exe().map(|exe| exe.to_string_lossy());
    let argv0 = proc.cmd().first().map(|arg| arg.to_string_lossy());
    // the joined command line is only needed to search inside it
    let command = (!proc.cmd().is_empty()).then(|| {
        proc.cmd()
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    });
    classify(
        pattern,
        &Names {
            name: &name,
            exe: exe.as_deref(),
            argv0: argv0.as_deref(),
            command: command.as_deref(),
        },
        mode,
    )
}

impl Process {
    /// Whether the OS let proc read this process's CPU and memory. On macOS and Windows
    /// sysinfo leaves the CPU time absent and the memory at 0 for a process it was not
    /// allowed to read (see [`cpu_time_secs_read`]), and the 0.0% and 0 are no reading.
    /// Elsewhere (Linux, the BSDs) the files are world-readable, so a 0 is a reading: a
    /// kernel thread or a zombie has no memory. The one test every view uses.
    pub(crate) fn read_by_os(&self) -> bool {
        !(cfg!(any(target_os = "macos", windows))
            && self.memory_mb <= 0.0
            && self.cpu_time_secs.is_none())
    }

    /// Whether proc can see this process's sockets, so that an empty list of ports means
    /// it listens on nothing. On unix the socket sources (lsof, `/proc/<pid>/fd`) map the
    /// sockets of the user's own processes, and of every process for root, and show
    /// nothing at all for another user's process (`lsof -p 1` as a user prints nothing).
    /// Windows reads every PID's sockets from netstat.
    #[cfg(unix)]
    pub(crate) fn sockets_visible(&self) -> bool {
        let effective = nix::unistd::Uid::effective();
        effective.is_root()
            || Uid::try_from(effective.as_raw() as usize)
                .is_ok_and(|uid| self.user.as_deref() == Some(user_name(&uid).as_str()))
    }

    /// Windows reads every PID's sockets from netstat.
    #[cfg(not(unix))]
    pub(crate) fn sockets_visible(&self) -> bool {
        true
    }

    /// The names a name target is compared with
    pub fn names(&self) -> Names<'_> {
        match &self.raw {
            Some(raw) => Names {
                name: &raw.name,
                exe: raw.exe_path.as_deref(),
                argv0: self.argv0.as_deref(),
                command: raw.command.as_deref(),
            },
            None => Names {
                name: &self.name,
                exe: self.exe_path.as_deref(),
                argv0: self.argv0.as_deref(),
                command: self.command.as_deref(),
            },
        }
    }

    /// The working directory exactly as the OS gave it, for matching `--in`.
    /// `cwd` is what is shown, with control characters escaped, so a directory
    /// named `a<newline>b` and one named `a\nb` (backslash, `n`) both read
    /// `a\nb` there; this one tells them apart.
    pub fn raw_cwd(&self) -> Option<&str> {
        match &self.raw {
            Some(raw) => raw.cwd.as_deref(),
            None => self.cwd.as_deref(),
        }
    }

    /// Find all processes a name matches (see [`NameMatch`]): by default the
    /// whole name, with `--cmd` any part of the name or command line.
    ///
    /// Proc itself and its ancestor shells never match. When nothing matches by
    /// name but processes have the text in their command line, the error says
    /// so: it costs nothing, the same pass over the processes counts them.
    ///
    /// The processes proc runs under are left out, without saying so; a
    /// command uses [`Process::lookup_name`], which says which they were.
    pub fn find_by_name(pattern: &str, mode: NameMatch) -> Result<Vec<Process>> {
        Self::lookup_name(pattern, mode, Ancestors::LeaveOut)
            .map(|lookup| lookup.processes)
            .map_err(|error| as_before(error, pattern))
    }

    /// The processes a name matches, PID ascending, as
    /// [`find_by_name`](Self::find_by_name) finds them, and what `policy` does
    /// with a match proc runs under (see [`Ancestors`]). proc itself is never
    /// a match. With every match left out, the error is
    /// [`ProcError::AncestorsOnly`], which names them
    pub fn lookup_name(pattern: &str, mode: NameMatch, policy: Ancestors) -> Result<NameLookup> {
        let sys = refreshed_system();

        // proc and the processes it runs under: their command lines hold proc's
        // own arguments (e.g. "zsh -c proc by node"), so a match in the command
        // line never counts toward the hint, and `policy` decides the rest
        let ancestor_pids = self_and_ancestors(&sys);
        let me = Pid::from_u32(std::process::id());
        let mut in_command_line_only = 0;
        let mut matches: Vec<Process> = sys
            .processes()
            .iter()
            .filter_map(|(pid, proc)| {
                // Exclude proc itself and Linux threads
                if *pid == me || is_thread(proc) {
                    return None;
                }
                match classify_sysinfo(pattern, proc, mode) {
                    // a process proc runs under matched by --cmd only in its
                    // command line, which holds proc's own arguments: no match
                    Hit::Match if ancestor_pids.contains(pid) && mode != NameMatch::Exact => {
                        let process = Process::from_sysinfo(*pid, proc);
                        matches_outside_command_line(&process, pattern, mode).then_some(process)
                    }
                    Hit::Match => Some(Process::from_sysinfo(*pid, proc)),
                    Hit::CommandLineOnly if !ancestor_pids.contains(pid) => {
                        in_command_line_only += 1;
                        None
                    }
                    Hit::CommandLineOnly | Hit::No => None,
                }
            })
            .collect();
        // sysinfo keeps its processes in a randomly ordered map: PID ascending
        // makes every list that starts here the same on every run
        matches.sort_by_key(|p| p.pid);

        let ancestors: std::collections::HashSet<u32> = ancestor_pids
            .iter()
            .map(|pid| pid.as_u32())
            .filter(|&pid| pid != me.as_u32())
            .collect();
        let mut lookup = split_ancestors(matches, &ancestors, mode, policy);
        if lookup.processes.is_empty() {
            if !lookup.left_out.is_empty() {
                return Err(ProcError::AncestorsOnly {
                    name: pattern.to_string(),
                    ancestors: lookup.left_out,
                });
            }
            return Err(ProcError::name_not_found(pattern, in_command_line_only));
        }
        fill_family(&mut lookup.processes, &sys);

        Ok(lookup)
    }

    /// Find a specific process by PID
    pub fn find_by_pid(pid: u32) -> Result<Option<Process>> {
        let sys = refreshed_system();

        let sysinfo_pid = Pid::from_u32(pid);

        // `refreshed_system` holds every process, so the parent and the
        // children are known at the cost of one pass over the table
        let found = sys
            .processes()
            .get(&sysinfo_pid)
            .map(|proc| Process::from_sysinfo(sysinfo_pid, proc));
        // macOS: sysinfo leaves a zombie out, which `list` still shows; ask
        // the kernel before saying there is no such process
        #[cfg(target_os = "macos")]
        let found = found.or_else(|| macos_zombie(pid));
        Ok(found.map(|found| {
            let mut found = [found];
            fill_family(&mut found, &sys);
            let [found] = found;
            found
        }))
    }

    /// The running processes among `pids`, PID ascending, each read in full
    /// (name, command line, user, memory, start time, executable) by one
    /// refresh of exactly those PIDs rather than of the whole process table. A
    /// PID that is gone is missing from the answer. The parent's name and the
    /// number of children are not filled: they need the whole table (see
    /// [`Process::find_all`]).
    pub fn find_by_pids(pids: &[u32]) -> Vec<Process> {
        let wanted = each_once(pids);
        let mut sys = System::new();
        sysinfo_locked(|| {
            sys.refresh_processes_specifics(
                sysinfo::ProcessesToUpdate::Some(&wanted),
                true,
                sysinfo::ProcessRefreshKind::everything(),
            )
        });
        let mut found: Vec<Process> = sys
            .processes()
            .iter()
            .map(|(pid, proc)| Process::from_sysinfo(*pid, proc))
            .collect();
        found.sort_by_key(|p| p.pid);
        found
    }

    /// Get all running processes (excludes proc's own process)
    pub fn find_all() -> Result<Vec<Process>> {
        let sys = refreshed_system();

        let self_pid = sysinfo::Pid::from_u32(std::process::id());
        #[allow(unused_mut)]
        let mut processes: Vec<Process> = sys
            .processes()
            .iter()
            .filter(|(pid, proc)| **pid != self_pid && !is_thread(proc))
            .map(|(pid, proc)| Process::from_sysinfo(*pid, proc))
            .collect();

        #[cfg(target_os = "macos")]
        {
            let known: std::collections::HashSet<u32> = processes.iter().map(|p| p.pid).collect();
            processes.extend(macos_zombies(&known));
        }

        // sysinfo keeps its processes in a randomly ordered map (and the
        // zombies are appended after them): PID ascending makes every list
        // that starts here the same on every run
        processes.sort_by_key(|p| p.pid);
        fill_family(&mut processes, &sys);

        Ok(processes)
    }

    /// Find processes running a specific executable path
    pub fn find_by_exe_path(path: &std::path::Path) -> Result<Vec<Process>> {
        let all = Self::find_all()?;
        let path_str = path.to_string_lossy();

        Ok(all
            .into_iter()
            .filter(|p| {
                if let Some(ref exe) = p.exe_path {
                    // Exact match (compare as strings to avoid PathBuf allocation)
                    exe == &*path_str || std::path::Path::new(exe) == path
                } else {
                    false
                }
            })
            .collect())
    }

    /// Find processes that have a file open (Unix only), proc itself left out.
    /// On Linux proc reads `/proc` itself (`linux_open_file_holders`), so a
    /// system without lsof, such as the Docker image, still gets an answer.
    /// macOS asks `lsof`, and FreeBSD and NetBSD `fstat`, which their base
    /// system has: pkgsrc's lsof 4.91 found no holder at all on NetBSD 11
    /// x86_64, and fstat is built with the kernel it reads
    #[cfg(unix)]
    pub fn find_by_open_file(path: &std::path::Path) -> Result<Vec<Process>> {
        #[cfg(target_os = "linux")]
        let pids = linux_open_file_holders(std::path::Path::new("/proc"), path)?;
        #[cfg(not(target_os = "linux"))]
        let pids = open_file_pids_with(OPEN_FILE_PROGRAM, path)?;

        let self_pid = std::process::id();
        let mut processes = Vec::new();
        for pid in pids.into_iter().filter(|&pid| pid != self_pid) {
            if let Ok(Some(proc)) = Self::find_by_pid(pid) {
                processes.push(proc);
            }
        }

        Ok(processes)
    }

    /// Find processes that have a file open (Windows stub)
    #[cfg(not(unix))]
    pub fn find_by_open_file(_path: &std::path::Path) -> Result<Vec<Process>> {
        // Windows: Could use handle.exe from Sysinternals, but skip for now
        Ok(vec![])
    }

    /// Find processes that look stuck: running longer than `timeout`, and in each
    /// of two readings a second apart either using over half a core or blocked in
    /// uninterruptible sleep. One reading
    /// used to be enough, so a momentary spike counted. A long build at full CPU
    /// still looks the same as a loop that never ends; only the user can tell.
    pub fn find_stuck(timeout: Duration) -> Result<Vec<Process>> {
        let timeout_secs = timeout.as_secs();
        // each read is timed from its start, as in `measure_cpu`
        let mut read_at = std::time::Instant::now();
        let mut sys = sysinfo_locked(System::new_all);
        let mut used = cpu_used(&sys);
        let mut cpu: std::collections::HashMap<Pid, f32> = std::collections::HashMap::new();
        let mut stuck: Option<std::collections::HashSet<Pid>> = None;

        for _ in 0..STUCK_READINGS {
            std::thread::sleep(STUCK_READING_GAP);
            let started = std::time::Instant::now();
            sysinfo_locked(|| sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true));
            let wall = started - read_at;
            read_at = started;
            // a process first seen in this read has no CPU figure yet
            cpu = sys
                .processes()
                .iter()
                .filter_map(|(pid, p)| {
                    let before = used.get(pid)?;
                    Some((
                        *pid,
                        cpu_percent_between(*before, cpu_time_ms(*pid, p), wall),
                    ))
                })
                .collect();
            used = cpu_used(&sys);
            let now: std::collections::HashSet<Pid> = sys
                .processes()
                .iter()
                .filter(|(pid, p)| {
                    let busy = cpu.get(pid).copied().unwrap_or(0.0);
                    counts_as_stuck(
                        is_thread(p),
                        p.run_time(),
                        timeout_secs,
                        looks_stuck(busy, p.status()),
                    )
                })
                .map(|(pid, _)| *pid)
                .collect();
            stuck = Some(match stuck {
                None => now,
                Some(before) => before.intersection(&now).copied().collect(),
            });
        }

        let stuck = stuck.unwrap_or_default();
        let mut found: Vec<Process> = sys
            .processes()
            .iter()
            .filter(|(pid, _)| stuck.contains(pid))
            .map(|(pid, p)| {
                let mut process = Process::from_sysinfo(*pid, p);
                // the figure the last read measured, not sysinfo's own
                if let Some(&busy) = cpu.get(pid) {
                    process.cpu_percent = busy;
                }
                process
            })
            .collect();
        // sysinfo keeps its processes in a randomly ordered map: PID ascending
        found.sort_by_key(|p| p.pid);
        fill_family(&mut found, &sys);
        Ok(found)
    }

    /// Whether this process looks stuck now, measured the way `find_stuck` does
    /// (stuck in two readings a second apart), for a process named as a target.
    pub fn looks_stuck_now(&self) -> bool {
        let pid = Pid::from_u32(self.pid);
        let mut sys = System::new();
        let mut read_at = std::time::Instant::now();
        self.refresh_for_check(&mut sys);
        let mut used = sys
            .process(pid)
            .filter(|p| self.is_same(p))
            .map(|p| cpu_time_ms(pid, p));
        (0..STUCK_READINGS).all(|_| {
            std::thread::sleep(STUCK_READING_GAP);
            let started = std::time::Instant::now();
            self.refresh_for_check(&mut sys);
            let wall = started - read_at;
            read_at = started;
            let (Some(before), Some(p)) = (used, sys.process(pid).filter(|p| self.is_same(p)))
            else {
                return false;
            };
            let after = cpu_time_ms(pid, p);
            used = Some(after);
            looks_stuck(cpu_percent_between(before, after, wall), p.status())
        })
    }

    /// CPU use over `window` (at least sysinfo's minimum), in percent of one core;
    /// None if the process is gone. A reading over a few milliseconds is ~0 for
    /// any process, so a real window is needed.
    ///
    /// Unix only (the method does not exist on Windows): its one caller is the gentle recovery `unstick` tries there;
    /// on Windows `unstick` only terminates.
    #[cfg(unix)]
    pub fn cpu_over(&self, window: Duration) -> Option<f32> {
        let pid = Pid::from_u32(self.pid);
        let mut sys = System::new();
        let started = std::time::Instant::now();
        self.refresh_for_check(&mut sys);
        let before = sys
            .process(pid)
            .filter(|p| self.is_same(p))
            .map(|p| cpu_time_ms(pid, p))?;
        std::thread::sleep(window.max(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL));
        let wall = started.elapsed();
        self.refresh_for_check(&mut sys);
        sys.process(pid)
            .filter(|p| self.is_same(p))
            .map(|p| cpu_percent_between(before, cpu_time_ms(pid, p), wall))
    }

    /// Measure the CPU use of these processes over 0.2 s and set `cpu_percent`,
    /// in percent of one core. A lookup reads CPU a few milliseconds after its
    /// first refresh, which is ~0% even for a process using a whole core, so
    /// commands call this before they show, filter or sort by CPU. One 0.2 s wait
    /// covers the whole batch; commands that never show CPU skip it.
    ///
    /// CPU is the CPU time each process used between two reads, over the wall
    /// time we measure between them. sysinfo's own `cpu_usage` divides by its
    /// system-wide tick interval instead, which read about 2% for a process
    /// spinning at 100% in some runs.
    pub fn measure_cpu(processes: &mut [Process]) {
        if processes.is_empty() {
            return;
        }
        let pids: Vec<Pid> = processes.iter().map(|p| Pid::from_u32(p.pid)).collect();
        let which = sysinfo::ProcessesToUpdate::Some(&pids);
        let cpu_only = sysinfo::ProcessRefreshKind::nothing().with_cpu();
        let mut sys = System::new();

        let first = std::time::Instant::now();
        sysinfo_locked(|| sys.refresh_processes_specifics(which, true, cpu_only));
        let before = cpu_used(&sys);
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        // each refresh reads the processes in the same order, so timing both
        // from their start gives every process the same window
        let wall = first.elapsed();
        sysinfo_locked(|| sys.refresh_processes_specifics(which, true, cpu_only));

        for process in processes.iter_mut() {
            let pid = Pid::from_u32(process.pid);
            let (Some(current), Some(&cpu_before)) = (sys.process(pid), before.get(&pid)) else {
                continue;
            };
            if process.is_same(current) {
                process.cpu_percent =
                    cpu_percent_between(cpu_before, cpu_time_ms(pid, current), wall);
            }
        }
    }

    /// Force kill the process (SIGKILL on Unix, taskkill /F on Windows)
    pub fn kill(&self) -> Result<()> {
        let mut sys = System::new();
        self.refresh_for_check(&mut sys);

        match sys.process(Pid::from_u32(self.pid)) {
            Some(proc) if self.is_same(proc) => kill_outcome(proc.kill(), self.pid),
            // gone, or the PID now belongs to a new process, or (macOS) a zombie
            _ => self.kill_unlisted(),
        }
    }

    /// `kill` for a PID the refresh did not list. macOS: sysinfo leaves a
    /// zombie out, so the kernel is asked; a zombie takes the signal as it does
    /// on Linux (it changes nothing, the process has exited).
    #[cfg(target_os = "macos")]
    fn kill_unlisted(&self) -> Result<()> {
        self.same_but_unlisted()?;
        let pid = nix::unistd::Pid::from_raw(self.pid as i32);
        let delivered = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL).is_ok();
        kill_outcome(delivered, self.pid)
    }

    /// `kill` for a PID the refresh did not list: gone, or a new process has it
    #[cfg(not(target_os = "macos"))]
    fn kill_unlisted(&self) -> Result<()> {
        Err(ProcError::ProcessNotFound(self.pid.to_string()))
    }

    /// Refreshes this record's PID in `sys` for a check with [`is_same`](Self::is_same),
    /// reading again while the process reports no start time ([`read_until_started`])
    fn refresh_for_check(&self, sys: &mut System) {
        let pid = Pid::from_u32(self.pid);
        read_until_started(self.start_time.is_some(), || {
            sysinfo_locked(|| {
                sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true)
            });
            sys.process(pid).map(|p| p.start_time())
        });
    }

    /// Whether `current`, the process now holding this PID, is the one this record
    /// describes. A different start time means the PID was reused by a new process.
    fn is_same(&self, current: &sysinfo::Process) -> bool {
        same_start(self.start_time, current.start_time())
    }

    /// Checked right before each signal, so a delayed one (stop's SIGKILL comes up
    /// to --timeout seconds after the target was resolved) can't reach a new
    /// process that reused the PID.
    fn ensure_same_process(&self) -> Result<()> {
        let mut sys = System::new();
        let pid = Pid::from_u32(self.pid);
        self.refresh_for_check(&mut sys);
        match sys.process(pid) {
            Some(current) if self.is_same(current) => Ok(()),
            _ => self.same_but_unlisted(),
        }
    }

    /// For a PID the refresh did not list. macOS: sysinfo leaves a zombie out,
    /// so the kernel is asked, and this record's own zombie counts as the same
    /// process (a signal to it succeeds and changes nothing, as on Linux).
    #[cfg(target_os = "macos")]
    fn same_but_unlisted(&self) -> Result<()> {
        match macos_zombie(self.pid) {
            Some(zombie) if same_start(self.start_time, zombie.start_time.unwrap_or(0)) => Ok(()),
            _ => Err(ProcError::ProcessNotFound(self.pid.to_string())),
        }
    }

    /// For a PID the refresh did not list: gone, or a new process has it
    #[cfg(not(target_os = "macos"))]
    fn same_but_unlisted(&self) -> Result<()> {
        Err(ProcError::ProcessNotFound(self.pid.to_string()))
    }

    /// Force kill and wait up to 5 seconds for the process to be gone.
    ///
    /// Never blocks forever (sysinfo's own kill_and_wait polls with no limit and
    /// never ends for a zombie). A process still alive after 5 seconds, such as
    /// one stuck in the kernel, is an error.
    pub fn kill_and_wait(&self) -> Result<()> {
        self.kill_and_wait_within(Duration::from_secs(5))
    }

    /// `kill_and_wait` with the time to wait for the process to be gone
    fn kill_and_wait_within(&self, wait: Duration) -> Result<()> {
        self.kill_then_wait(wait, Process::kill)
    }

    /// `kill_and_wait_within` with the kill to send, so a test can send one that
    /// leaves the process running (a real kill is over before it returns on
    /// Windows, though the process can stay listed for a moment after)
    fn kill_then_wait(&self, wait: Duration, kill: impl Fn(&Process) -> Result<()>) -> Result<()> {
        match kill(self) {
            // it exited on its own just before the kill
            Err(ProcError::ProcessNotFound(_)) => return Ok(()),
            other => other?,
        }
        let deadline = std::time::Instant::now() + wait;
        while std::time::Instant::now() < deadline {
            if !self.is_running() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(ProcError::SignalError(format!(
            "Process {} is still running {} seconds after SIGKILL",
            self.pid,
            wait.as_secs()
        )))
    }

    /// Send SIGTERM for graceful termination (Unix) or taskkill (Windows)
    #[cfg(unix)]
    pub fn terminate(&self) -> Result<()> {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid as NixPid;

        self.ensure_same_process()?;
        kill(NixPid::from_raw(self.pid as i32), Signal::SIGTERM)
            .map_err(|e| ProcError::SignalError(e.to_string()))
    }

    /// Graceful termination (Windows): `taskkill` without `/F` asks the
    /// process to close its windows, so it fails for a console program, which
    /// has none. That failure is returned, with taskkill's reason, rather than
    /// reported as success.
    #[cfg(windows)]
    pub fn terminate(&self) -> Result<()> {
        use std::process::Command;

        self.ensure_same_process()?;
        let output = Command::new("taskkill")
            .args(["/PID", &self.pid.to_string()])
            .output()
            .map_err(|e| ProcError::SystemError(e.to_string()))?;

        taskkill_outcome(&output)
    }

    /// Send an arbitrary signal to the process (Unix only)
    #[cfg(unix)]
    pub fn send_signal(&self, signal: nix::sys::signal::Signal) -> Result<()> {
        use nix::sys::signal::kill;
        use nix::unistd::Pid as NixPid;
        self.ensure_same_process()?;
        kill(NixPid::from_raw(self.pid as i32), signal)
            .map_err(|e| ProcError::SignalError(format!("{}: {}", signal, e)))
    }

    // Note: On Windows, send_signal is not available. Commands that use it
    // (freeze, thaw) have their own #[cfg(not(unix))] stubs that return NotSupported.

    /// Find orphaned processes (parent is PID 1 / init / launchd, excluding system daemons)
    ///
    /// Unix only: on Windows this returns `ProcError::NotSupported`, because
    /// Windows has no PID 1 and does not re-parent a process whose parent exited,
    /// so an empty list would claim "no orphans" where nothing could be checked.
    #[cfg(unix)]
    pub fn find_orphans() -> Result<Vec<Process>> {
        Ok(Self::orphans_among(Self::find_all()?))
    }

    /// Windows: orphans cannot be told, so this is an error, not an empty list
    #[cfg(not(unix))]
    pub fn find_orphans() -> Result<Vec<Process>> {
        Err(orphans_not_supported())
    }

    /// The orphans among `all`
    #[cfg(unix)]
    fn orphans_among(all: Vec<Process>) -> Vec<Process> {
        all.into_iter()
            .filter(|p| {
                if let Some(ppid) = p.parent_pid {
                    ppid == 1 && p.pid != 1 && !Self::is_system_process(p)
                } else {
                    false
                }
            })
            .collect()
    }

    /// Check if a process is a system daemon (naturally has PPID 1)
    ///
    /// Filters out processes that legitimately have PPID 1 — system daemons,
    /// kernel threads, and services managed by init/systemd/launchd.
    /// Heuristic: no cwd or cwd is "/" with exe in system paths.
    #[cfg(unix)]
    fn is_system_process(p: &Process) -> bool {
        if p.cwd.is_none() || p.cwd.as_deref() == Some("/") {
            if let Some(ref exe) = p.exe_path {
                // macOS system paths
                if exe.starts_with("/System/") || exe.starts_with("/usr/libexec/") {
                    return true;
                }
                // Shared Unix system paths (macOS + Linux)
                if exe.starts_with("/usr/sbin/")
                    || exe.starts_with("/sbin/")
                    || exe.starts_with("/usr/bin/")
                    || exe.starts_with("/usr/lib/")
                    || exe.starts_with("/usr/lib64/")
                    || exe.starts_with("/lib/")
                    || exe.starts_with("/lib64/")
                    || exe.starts_with("/opt/")
                    || exe.starts_with("/snap/")
                {
                    return true;
                }
            }
            return true; // No exe path = likely kernel thread
        }
        false
    }

    /// Check if the process is still alive. A zombie (exited, not yet reaped by
    /// its parent) has exited, and a process with a different start time is a new
    /// one that reused the PID, so neither counts.
    pub fn exists(&self) -> bool {
        let mut sys = System::new();
        let pid = Pid::from_u32(self.pid);
        self.refresh_for_check(&mut sys);
        match sys.process(pid) {
            None => false,
            Some(current) if current.status() == SysProcessStatus::Zombie => false,
            Some(current) => self.is_same(current),
        }
    }

    /// Check if the process is still running (alias for exists for compatibility)
    pub fn is_running(&self) -> bool {
        self.exists()
    }

    /// Convert from sysinfo Process
    pub(crate) fn from_sysinfo(pid: Pid, proc: &sysinfo::Process) -> Self {
        let cmd_vec = proc.cmd();
        // Every text field is set by whoever runs the process: escape control
        // characters before anything prints them. Name matching reads the raw
        // text (`raw`, kept only when escaping changed something).
        let raw_command = (!cmd_vec.is_empty()).then(|| {
            cmd_vec
                .iter()
                .map(|s| s.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ")
        });
        let raw_name = os_name(proc);
        let raw_exe = proc.exe().map(|p| p.to_string_lossy());
        let raw_cwd = proc
            .cwd()
            .map(|p| strip_deleted_marker(p.to_string_lossy().to_string()));
        let changed = |text: &str| matches!(escape_control(text), std::borrow::Cow::Owned(_));
        let raw = (changed(&raw_name)
            || raw_exe.as_deref().is_some_and(changed)
            || raw_command.as_deref().is_some_and(changed)
            || raw_cwd.as_deref().is_some_and(changed))
        .then(|| {
            Box::new(RawText {
                name: raw_name.to_string(),
                exe_path: raw_exe.as_ref().map(|exe| exe.to_string()),
                command: raw_command.clone(),
                cwd: raw_cwd.clone(),
            })
        });
        let command = raw_command
            .as_deref()
            .map(|joined| escape_control(joined).into_owned());

        let exe_path = raw_exe
            .as_deref()
            .map(|exe| escape_control(exe).into_owned());
        let cwd = raw_cwd
            .as_deref()
            .map(|path| escape_control(path).into_owned());

        Process {
            pid: pid.as_u32(),
            name: escape_control(&raw_name).into_owned(),
            exe_path,
            cwd,
            command,
            argv0: cmd_vec
                .first()
                .map(|arg| arg.to_string_lossy().into_owned()),
            raw,
            cpu_percent: proc.cpu_usage(),
            memory_mb: proc.memory() as f64 / 1024.0 / 1024.0,
            status: ProcessStatus::from(proc.status()),
            user: proc
                .user_id()
                .map(|uid| escape_control(&user_name(uid)).into_owned()),
            parent_pid: proc.parent().map(|p| p.as_u32()),
            // sysinfo reports 0 when it can't read the start time (on macOS, any
            // other user's process), which would make the uptime about 56 years
            start_time: (proc.start_time() > 0).then(|| proc.start_time()),
            uptime_secs: (proc.start_time() > 0).then(|| proc.run_time()),
            // the two that need other processes are filled by `fill_family`
            parent_name: None,
            cpu_time_secs: cpu_time_secs_read(cpu_time_ms(pid, proc), proc.memory()),
            open_files: open_files_read(proc.open_files()),
            child_count: None,
            // read once per run; never build a record inside `sysinfo_locked`,
            // whose FreeBSD lock this first read takes
            runs_proc: proc_ancestors().contains(&pid.as_u32()),
        }
    }
}

/// The whole seconds of CPU time a process has used, from the `cpu_ms` read
/// and its `memory` in bytes. On macOS and Windows sysinfo leaves both at 0 for
/// a process it was not allowed to read (any other user's on macOS: measured,
/// 232 of 1169 processes), and a process that is read has memory in use. So a
/// 0 with no memory is "not read", absent from the record, not "no CPU used".
fn cpu_time_secs_read(cpu_ms: u64, memory: u64) -> Option<u64> {
    let unread = cfg!(any(target_os = "macos", windows)) && cpu_ms == 0 && memory == 0;
    (!unread).then_some(cpu_ms / 1000)
}

/// How many files a process has open, from what sysinfo returned. On macOS
/// `proc_pidinfo` answers 0 (with errno EPERM, checked on PID 1) for a process
/// that is not the user's, and a live process has files open, so there a 0 is
/// "not read". On Windows sysinfo counts every kernel handle (events, mutexes
/// and the rest, not only files), which is not "open files", so there it is
/// not given. Elsewhere sysinfo returns `None` when it can't read (NetBSD reads
/// `/proc/<pid>/fd`, so it needs procfs mounted).
fn open_files_read(count: Option<usize>) -> Option<u64> {
    count
        .filter(|&count| !cfg!(target_os = "macos") || count > 0)
        .filter(|_| !cfg!(windows))
        .map(|count| count as u64)
}

/// Fill `parent_name` and `child_count` of `processes` from the full process
/// table `sys`, which holds their parents and children whether or not the list
/// being built kept them. Counts the table once, so the cost is one pass over
/// it however long `processes` is. Linux threads are no children.
pub(crate) fn fill_family(processes: &mut [Process], sys: &System) {
    let mut children: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    // proc itself is the child of whatever ran it, but it is no part of what
    // `tree` lists, so it is left out of the count as it is left out of the list
    let own = std::process::id();
    for proc in sys
        .processes()
        .values()
        .filter(|p| !is_thread(p) && p.pid().as_u32() != own)
    {
        if let Some(parent) = proc.parent() {
            *children.entry(parent.as_u32()).or_insert(0) += 1;
        }
    }
    for process in processes {
        process.child_count = Some(children.get(&process.pid).copied().unwrap_or(0));
        process.parent_name = process
            .parent_pid
            .and_then(|parent| sys.process(Pid::from_u32(parent)))
            .map(|parent| escape_control(&os_name(parent)).into_owned());
    }
}

/// Parse a signal name to a nix Signal (Unix only)
///
/// Accepts: "HUP", "SIGHUP", "hup" (signal names only, not numbers —
/// numeric signal values differ between macOS and Linux)
#[cfg(unix)]
pub fn parse_signal_name(name: &str) -> Result<nix::sys::signal::Signal> {
    use nix::sys::signal::Signal;

    let upper = name.to_uppercase();
    let upper = upper.trim_start_matches("SIG");
    match upper {
        "HUP" => Ok(Signal::SIGHUP),
        "INT" => Ok(Signal::SIGINT),
        "QUIT" => Ok(Signal::SIGQUIT),
        "ABRT" => Ok(Signal::SIGABRT),
        "KILL" => Ok(Signal::SIGKILL),
        "TERM" => Ok(Signal::SIGTERM),
        "STOP" => Ok(Signal::SIGSTOP),
        "CONT" => Ok(Signal::SIGCONT),
        "USR1" => Ok(Signal::SIGUSR1),
        "USR2" => Ok(Signal::SIGUSR2),
        _ => Err(ProcError::InvalidInput(format!(
            "Unknown signal: '{}'. Valid signals: HUP, INT, QUIT, ABRT, KILL, TERM, STOP, CONT, USR1, USR2",
            name
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_by_pid_names_a_process_as_the_process_list_does() {
        let me = std::process::id();
        let names = names_by_pid(&[me, 4_000_000]);
        let record = Process::find_by_pid(me).unwrap().unwrap();
        assert_eq!(names.get(&me), Some(&record.name));
        assert!(!names.contains_key(&4_000_000), "a PID nobody holds");
    }

    #[test]
    fn a_name_rewritten_into_a_title_gives_way_to_the_kernels() {
        let kernel = || Some("sshd".to_string());
        // a title, and a login shell's `-bash`: the kernel's name
        assert_eq!(
            kernel_name_if_retitled("sshd [listener] 0 of 10-100 startups".into(), kernel),
            "sshd"
        );
        let bash = || Some("bash".to_string());
        assert_eq!(kernel_name_if_retitled("-bash".into(), bash), "bash");
        // a plain name stays, and the kernel isn't asked
        let unasked = || -> Option<String> { panic!("asked the kernel") };
        assert_eq!(
            kernel_name_if_retitled("python3".into(), unasked),
            "python3"
        );
        // no kernel name to use: the name sysinfo gave
        let none = || None;
        assert_eq!(kernel_name_if_retitled("a b".into(), none), "a b");
        let empty = || Some(String::new());
        assert_eq!(kernel_name_if_retitled("a b".into(), empty), "a b");
    }

    /// A process record with only a PID and a name, as a lookup returns it
    fn named(pid: u32, name: &str) -> Process {
        Process {
            pid,
            name: escape_control(name).into_owned(),
            exe_path: None,
            cwd: None,
            command: None,
            argv0: None,
            raw: None,
            cpu_percent: 0.0,
            memory_mb: 0.0,
            status: ProcessStatus::Running,
            user: None,
            parent_pid: None,
            start_time: None,
            uptime_secs: None,
            parent_name: None,
            cpu_time_secs: None,
            open_files: None,
            child_count: None,
            runs_proc: false,
        }
    }

    #[test]
    fn split_keeps_and_marks_an_ancestor_only_for_an_exact_read() {
        let ancestors: std::collections::HashSet<u32> = [7].into_iter().collect();
        let zsh = Ancestor {
            pid: 7,
            name: "zsh".to_string(),
        };
        for (mode, policy, kept) in [
            (NameMatch::Exact, Ancestors::Mark, true),
            (NameMatch::Command, Ancestors::Mark, false),
            (NameMatch::Exact, Ancestors::LeaveOut, false),
            (NameMatch::Command, Ancestors::LeaveOut, false),
        ] {
            let split = split_ancestors(
                vec![named(7, "zsh"), named(9, "zsh")],
                &ancestors,
                mode,
                policy,
            );
            let pids: Vec<u32> = split.processes.iter().map(|p| p.pid).collect();
            let marked: Vec<bool> = split.processes.iter().map(|p| p.runs_proc).collect();
            if kept {
                assert_eq!(pids, vec![7, 9], "{mode:?} {policy:?}");
                assert_eq!(marked, vec![true, false], "{mode:?} {policy:?}");
                assert!(split.left_out.is_empty(), "{mode:?} {policy:?}");
            } else {
                // the other process stays, unmarked, whatever the policy
                assert_eq!(pids, vec![9], "{mode:?} {policy:?}");
                assert_eq!(marked, vec![false], "{mode:?} {policy:?}");
                assert_eq!(split.left_out, vec![zsh.clone()], "{mode:?} {policy:?}");
            }
        }
    }

    #[test]
    fn two_ancestors_with_one_name_are_both_left_out_in_pid_order() {
        let ancestors: std::collections::HashSet<u32> = [81340, 11849].into_iter().collect();
        let split = split_ancestors(
            vec![named(11849, "zsh"), named(81340, "zsh")],
            &ancestors,
            NameMatch::Exact,
            Ancestors::LeaveOut,
        );
        assert!(split.processes.is_empty());
        let pids: Vec<u32> = split.left_out.iter().map(|a| a.pid).collect();
        assert_eq!(pids, vec![11849, 81340]);
    }

    #[test]
    fn a_left_out_name_is_the_escaped_one() {
        let ancestors: std::collections::HashSet<u32> = [3].into_iter().collect();
        let split = split_ancestors(
            vec![named(3, "a\x1b[2Jb")],
            &ancestors,
            NameMatch::Command,
            Ancestors::Mark,
        );
        assert_eq!(split.left_out[0].name, "a\\x1b[2Jb");
    }

    #[test]
    fn find_by_name_still_leaves_ancestors_out() {
        // this test's own process is an ancestor of nothing proc runs here, but
        // proc's lookup runs inside it: its own name is proc's own (self), which
        // a lookup never returns, through the old API as through the new
        let me = Process::find_by_pid(std::process::id()).unwrap().unwrap();
        let found = Process::find_by_name(&me.name, NameMatch::Exact).unwrap_or_default();
        assert!(found.iter().all(|p| p.pid != me.pid), "{found:?}");
        let looked = Process::lookup_name(&me.name, NameMatch::Exact, Ancestors::Mark);
        let kept = looked.map(|l| l.processes).unwrap_or_default();
        assert!(kept.iter().all(|p| p.pid != me.pid), "{kept:?}");
    }

    #[test]
    fn find_by_name_answers_as_before_when_only_ancestors_matched() {
        let only_ancestors = ProcError::AncestorsOnly {
            name: "zsh".to_string(),
            ancestors: vec![Ancestor::from(&named(7, "zsh"))],
        };
        assert!(matches!(
            as_before(only_ancestors, "zsh"),
            ProcError::ProcessNotFound(ref n) if n == "zsh"
        ));
        assert!(matches!(
            as_before(ProcError::PortNotFound(1), "zsh"),
            ProcError::PortNotFound(1)
        ));
    }

    #[test]
    fn a_command_line_that_holds_procs_arguments_is_not_a_match() {
        // `sh -c 'proc by --cmd X'`: the shell's command line holds X
        let mut shell = named(7, "sh");
        shell.command = Some("sh -c proc by --cmd qqx".to_string());
        assert!(!matches_outside_command_line(
            &shell,
            "qqx",
            NameMatch::Command
        ));
        // its own name still matches, under --cmd as exactly
        assert!(matches_outside_command_line(
            &shell,
            "sh",
            NameMatch::Command
        ));
        assert!(matches_outside_command_line(&shell, "sh", NameMatch::Exact));
        // and so does the argv[0] it was started under (`exec -a devshell sh`)
        let mut renamed = named(8, "bash");
        renamed.argv0 = Some("devshell".to_string());
        renamed.command = Some("devshell -c proc by --cmd devshell".to_string());
        assert!(matches_outside_command_line(
            &renamed,
            "devshell",
            NameMatch::Command
        ));
    }

    #[test]
    fn runs_proc_is_in_the_json_only_when_true() {
        let mut process = named(1, "zsh");
        let json = serde_json::to_value(&process).unwrap();
        assert!(json.get("runs_proc").is_none(), "{json}");
        process.runs_proc = true;
        let json = serde_json::to_value(&process).unwrap();
        assert_eq!(json["runs_proc"], true);
        assert_eq!(
            serde_json::to_value(ProcessDetails::of(&process)).unwrap()["runs_proc"],
            true
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_c_array_reads_up_to_its_nul() {
        let chars: Vec<libc::c_char> = b"sshd\0junk".iter().map(|&b| b as libc::c_char).collect();
        assert_eq!(c_array_text(&chars), "sshd");
        let full: Vec<libc::c_char> = b"sshd".iter().map(|&b| b as libc::c_char).collect();
        assert_eq!(c_array_text(&full), "sshd");
    }

    #[test]
    fn names_by_pid_names_a_process_asked_for_twice() {
        // a process listening on IPv4 and IPv6 is one PID per socket, and
        // sysinfo took the second refresh of one PID as the process having
        // ended: Windows' `proc ports` named such holders "unknown"
        let me = std::process::id();
        let names = names_by_pid(&[me, me]);
        let record = Process::find_by_pid(me).unwrap().unwrap();
        assert_eq!(names.get(&me), Some(&record.name));
    }

    #[test]
    fn find_by_pids_reads_the_processes_asked_for_in_full() {
        let me = std::process::id();
        // asked twice and out of order, with a PID nobody holds
        let found = Process::find_by_pids(&[4_000_000, me, me]);
        assert_eq!(found.len(), 1, "{found:?}");
        let record = Process::find_by_pid(me).unwrap().unwrap();
        let mine = &found[0];
        assert_eq!(mine.pid, me);
        assert_eq!(mine.name, record.name);
        assert_eq!(mine.command, record.command);
        assert_eq!(mine.user, record.user);
        assert_eq!(mine.exe_path, record.exe_path);
        assert_eq!(mine.start_time, record.start_time);
        assert!(mine.memory_mb > 0.0);
        assert!(Process::find_by_pids(&[]).is_empty());
    }

    #[test]
    fn a_signal_needs_the_start_time_the_record_saw() {
        // the same start time, or within the second the clock rounds to
        assert!(same_start(Some(1_000), 1_000));
        assert!(same_start(Some(1_000), 1_001));
        // a newer process on the PID
        assert!(!same_start(Some(1_000), 4_600));
        // the holder's start time can't be read: not proof it is the same one
        assert!(!same_start(Some(1_000), 0));
        // a record without a start time has nothing to compare
        assert!(same_start(None, 0));
        assert!(same_start(None, 1_000));
    }

    #[test]
    fn a_start_time_that_reads_0_is_read_again_until_it_is_there() {
        // how many reads each sequence of answers gets
        let reads = |wants_start: bool, answers: &[Option<u64>]| {
            let mut left = answers.iter();
            let mut count = 0;
            read_until_started(wants_start, || {
                count += 1;
                // past the given answers, the start time stays unread
                left.next().copied().unwrap_or(Some(0))
            });
            count
        };
        // NetBSD, while the process takes a signal: there a moment later
        assert_eq!(reads(true, &[Some(0), Some(0), Some(1_000)]), 3);
        // never readable: gives up after START_TIME_READS reads
        assert_eq!(reads(true, &[]), START_TIME_READS);
        // read at once, or the process is gone: one read
        assert_eq!(reads(true, &[Some(1_000)]), 1);
        assert_eq!(reads(true, &[None]), 1);
        // a record without a start time compares none, so 0 is not read again
        assert_eq!(reads(false, &[Some(0)]), 1);
    }

    /// A service account (shell `/bin/false` or `/usr/bin/false`) is named like
    /// any other: sysinfo's user list leaves such accounts out, so proc showed
    /// their user ID instead. macOS's `daemon` account has that shell; a Linux
    /// machine need not have one, so the test runs on macOS. It asks the
    /// directory service, as proc does: Nix's macOS sandbox can't read /etc/passwd
    #[cfg(target_os = "macos")]
    #[test]
    fn an_account_whose_shell_is_false_is_named() {
        let daemon = nix::unistd::User::from_name("daemon")
            .unwrap()
            .expect("macOS has a daemon account");
        assert!(
            daemon.shell.to_string_lossy().ends_with("/false"),
            "{:?}",
            daemon.shell
        );
        let uid = Uid::try_from(daemon.uid.as_raw() as usize).unwrap();
        assert_eq!(user_name(&uid), "daemon");
    }

    #[cfg(unix)]
    #[test]
    fn a_user_id_nobody_has_is_shown_as_the_number() {
        let uid = Uid::try_from(4_000_000_000usize).unwrap();
        assert_eq!(user_name(&uid), "4000000000");
    }

    #[test]
    fn cpu_is_the_cpu_time_used_over_the_wall_time() {
        let second = Duration::from_secs(1);
        // a full core for a second, half a core, and nothing
        assert_eq!(cpu_percent_between(5_000, 6_000, second), 100.0);
        assert_eq!(cpu_percent_between(5_000, 5_500, second), 50.0);
        assert_eq!(cpu_percent_between(5_000, 5_000, second), 0.0);
        // two cores' worth reads 200%, in percent of one core
        assert_eq!(cpu_percent_between(0, 2_000, second), 200.0);
        // a counter that went backwards (a reused PID) or no time passing is 0
        assert_eq!(cpu_percent_between(6_000, 5_000, second), 0.0);
        assert_eq!(cpu_percent_between(0, 1_000, Duration::ZERO), 0.0);
    }

    #[test]
    fn a_failed_command_is_explained_in_its_own_words() {
        let reason = "ERROR: The process with PID 7 could not be terminated.";
        assert_eq!(
            command_failure(b"", reason.as_bytes(), "exit code: 1"),
            reason
        );
        // stdout when stderr is empty, trimmed
        assert_eq!(
            command_failure(b"  not here\r\n", b"", "exit code: 1"),
            "not here"
        );
        // taskkill wraps its reason over several lines; the error keeps them on one
        assert_eq!(
            command_failure(
                b"",
                b"ERROR: first part\r\n  second part.\r\n\r\n",
                "exit code: 1"
            ),
            "ERROR: first part second part."
        );
        assert_eq!(
            command_failure(b"", b"  \n", "exit code: 128"),
            "exited with exit code: 128"
        );
    }

    #[test]
    fn busy_means_over_half_a_core_or_blocked_in_the_kernel() {
        assert!(looks_stuck(50.1, SysProcessStatus::Run));
        assert!(!looks_stuck(50.0, SysProcessStatus::Run));
        assert!(!looks_stuck(0.0, SysProcessStatus::Sleep));
        assert!(looks_stuck(0.0, SysProcessStatus::UninterruptibleDiskSleep));
    }

    #[test]
    fn a_reading_counts_as_stuck_from_the_timeout_on() {
        assert!(counts_as_stuck(false, 0, 0, true));
        assert!(!counts_as_stuck(false, 59, 60, true));
        assert!(counts_as_stuck(false, 60, 60, true));
        assert!(counts_as_stuck(false, 61, 60, true));
        // and only a busy process, never a thread
        assert!(!counts_as_stuck(false, 60, 60, false));
        assert!(!counts_as_stuck(true, 60, 60, true));
    }

    #[test]
    fn a_process_that_ran_the_timeout_has_run_long_enough() {
        // --timeout 0 finds a process that reads 0 seconds old
        assert!(ran_long_enough(0, 0));
        assert!(ran_long_enough(5, 5));
        assert!(ran_long_enough(6, 5));
        assert!(!ran_long_enough(4, 5));
    }

    #[test]
    fn test_find_all_processes() {
        let processes = Process::find_all().unwrap();
        assert!(!processes.is_empty(), "Should find at least one process");
    }

    #[test]
    fn every_process_source_lists_pid_ascending() {
        use crate::test_support::{end, idle_child, record_of};

        let mut children: Vec<_> = (0..3).map(|_| idle_child()).collect();
        let mine: Vec<u32> = children.iter().map(|c| record_of(c).pid).collect();
        let name = record_of(&children[0]).name;

        let ascending = |what: &str, list: &[Process]| {
            let pids: Vec<u32> = list.iter().map(|p| p.pid).collect();
            let mut sorted = pids.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(pids, sorted, "{what} is not PID ascending");
            pids
        };
        let all = ascending("find_all", &Process::find_all().unwrap());
        let named = ascending(
            "find_by_name",
            &Process::find_by_name(&name, NameMatch::Exact).unwrap(),
        );
        ascending("find_stuck", &Process::find_stuck(Duration::ZERO).unwrap());
        for pid in &mine {
            assert!(all.contains(pid), "find_all has {pid}");
            assert!(named.contains(pid), "find_by_name has {pid}");
        }
        children.iter_mut().for_each(end);
    }

    #[test]
    fn test_find_by_pid_self() {
        let pid = std::process::id();
        let process = Process::find_by_pid(pid).unwrap();
        assert!(process.is_some(), "Should find own process");
    }

    #[test]
    fn test_is_running_tells_a_reused_pid_apart() {
        let mut me = Process::find_by_pid(std::process::id()).unwrap().unwrap();
        assert!(me.is_running());
        // Same PID, different start time: a new process that reused the PID
        let start = me.start_time.expect("a running process has a start time");
        me.start_time = Some(start - 3600);
        assert!(!me.is_running(), "a reused PID counted as the same process");
    }

    #[test]
    fn test_escape_control_makes_terminal_input_harmless() {
        // Ordinary text, including non-ASCII, is untouched
        assert_eq!(escape_control("node server.js"), "node server.js");
        assert_eq!(escape_control("café ✓ 日本"), "café ✓ 日本");
        // ESC sequences: colours, window title (OSC), clipboard (OSC 52)
        assert_eq!(escape_control("a\x1b[31mb"), "a\\x1b[31mb");
        assert_eq!(escape_control("\x1b]0;pwned\x07"), "\\x1b]0;pwned\\x07");
        // Line breaks and tabs would split or shift a table row
        assert_eq!(escape_control("one\ntwo\tthree\r"), "one\\ntwo\\tthree\\r");
        // DEL, and C1 controls such as the 8-bit CSI (U+009B)
        assert_eq!(escape_control("x\x7fy\u{9b}2J"), "x\\x7fy\\x9b2J");
        // Bidirectional overrides can reorder text on screen ("Trojan Source")
        assert_eq!(escape_control("ab\u{202e}cd"), "ab\\u{202e}cd");
    }

    #[cfg(unix)]
    #[test]
    fn test_looks_stuck_needs_sustained_cpu() {
        let spawn = |script: &str| {
            std::process::Command::new("sh")
                .args(["-c", script])
                .spawn()
                .unwrap()
        };
        let mut spinning = spawn("while :; do :; done");
        let mut idle = spawn("exec sleep 300");
        std::thread::sleep(Duration::from_millis(300));
        let record = |pid: u32| Process::find_by_pid(pid).unwrap().unwrap();

        // On a loaded machine (a CI runner running other tests) a spinner can get less
        // than half a core for two seconds running, so it gets a few tries to be seen
        // at its real speed; an idle process must never count as stuck, on any try.
        let spinning_stuck = (0..4).any(|_| record(spinning.id()).looks_stuck_now());
        let idle_stuck = record(idle.id()).looks_stuck_now();
        for child in [&mut spinning, &mut idle] {
            let _ = child.kill();
            let _ = child.wait();
        }
        assert!(
            spinning_stuck,
            "a process spinning at 100% CPU wasn't stuck"
        );
        assert!(!idle_stuck, "an idle process counted as stuck");
    }

    #[test]
    fn test_signals_never_reach_a_process_that_reused_the_pid() {
        let mut child = crate::test_support::idle_child();
        let mut stale = crate::test_support::record_of(&child);
        // A record from before the PID was reused: same PID, older start time
        stale.start_time = stale.start_time.map(|t| t - 3600);

        let kill = stale.kill();
        let term = stale.terminate();
        #[cfg(unix)]
        let sig = stale.send_signal(nix::sys::signal::Signal::SIGKILL);
        let alive = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let _ = child.wait();

        assert!(
            alive,
            "a stale record signalled the process now using its PID"
        );
        assert!(
            matches!(kill, Err(ProcError::ProcessNotFound(_))),
            "{kill:?}"
        );
        assert!(
            matches!(term, Err(ProcError::ProcessNotFound(_))),
            "{term:?}"
        );
        #[cfg(unix)]
        assert!(matches!(sig, Err(ProcError::ProcessNotFound(_))), "{sig:?}");
    }

    /// A control character in a process's name, executable or command line is
    /// shown escaped, but name matching reads what the OS gave: a process found
    /// by `find_by_name` is also kept by `--by`, and the other way round
    #[cfg(unix)]
    #[test]
    fn text_with_control_characters_matches_as_typed_and_displays_escaped() {
        use crate::core::matches_by_filter;
        use std::os::unix::process::CommandExt;

        let word = format!("zz\u{7}ctl{}x", std::process::id());
        // A shell waiting on its stdin, not `sleep`: where `sleep` is one name
        // of a multi-call binary (Nix's coreutils), it refuses to run under
        // another first argument. The shell ends when the pipe closes.
        let mut child = std::process::Command::new("sh")
            .arg0(&word)
            .args(["-c", "read line"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();
        // `spawn` returns once the exec has begun, not once it has finished:
        // measured on Linux, /proc/<pid>/cmdline is still empty for a moment
        // after it (name and exe are already the new program's). A record taken
        // in that moment has no first argument, so wait for the command line.
        let record = std::iter::repeat_with(|| {
            std::thread::sleep(Duration::from_millis(5));
            Process::find_by_pid(pid).unwrap().unwrap()
        })
        .take(1000)
        .find(|record| record.argv0.is_some());
        let part = format!("\u{7}ctl{}", std::process::id());

        let found = |pattern: &str, mode| {
            Process::find_by_name(pattern, mode)
                .map(|found| found.iter().any(|p| p.pid == pid))
                .unwrap_or(false)
        };
        let named = found(&word, NameMatch::Exact);
        let named_in_command = found(&part, NameMatch::Command);
        let _ = child.kill();
        let _ = child.wait();

        let record = record.expect("the child's command line never appeared");
        assert!(named, "find_by_name read the escaped first argument");
        assert!(
            named_in_command,
            "find_by_name read the escaped command line"
        );
        assert!(
            matches_by_filter(&record, &word, NameMatch::Exact),
            "--by read the escaped first argument"
        );
        assert!(
            matches_by_filter(&record, &part, NameMatch::Command),
            "--by --cmd read the escaped command line"
        );
        // what is shown is escaped
        let shown = record.command.as_deref().unwrap();
        assert!(shown.contains("\\x07ctl"), "{shown}");
        assert!(!shown.contains('\u{7}'), "{shown:?}");
    }

    /// The same, on Windows, where a process has no `arg0` to set: a control
    /// character in the command line (the listener example ignores extra
    /// arguments) is shown escaped and still matches as typed with `--cmd`
    #[cfg(windows)]
    #[test]
    fn a_control_character_in_a_windows_command_line_matches_as_typed_and_displays_escaped() {
        use crate::core::matches_by_filter;

        let mut exe = std::env::current_exe().unwrap();
        exe.pop();
        exe.pop();
        exe.push("examples");
        exe.push("listener.exe");
        assert!(exe.exists(), "build the example first: {}", exe.display());
        let part = format!("\u{7}ctl{}", std::process::id());
        let mut child = std::process::Command::new(&exe)
            .arg(&part)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let record = std::iter::repeat_with(|| {
            std::thread::sleep(Duration::from_millis(20));
            Process::find_by_pid(pid).unwrap()
        })
        .take(500)
        .flatten()
        .find(|record| record.command.is_some())
        .expect("the child's command line never appeared");
        let found = Process::find_by_name(&part, NameMatch::Command)
            .map(|found| found.iter().any(|p| p.pid == pid))
            .unwrap_or(false);
        let kept = matches_by_filter(&record, &part, NameMatch::Command);
        crate::test_support::end(&mut child);

        assert!(found, "find_by_name read the escaped command line");
        assert!(kept, "--by --cmd read the escaped command line");
        let shown = record.command.as_deref().unwrap();
        assert!(shown.contains("\\x07ctl"), "{shown}");
        assert!(!shown.contains('\u{7}'), "{shown:?}");
    }

    #[test]
    fn a_name_is_read_from_the_raw_text_when_escaping_changed_it() {
        let record = Process {
            name: "a\\x07b".to_string(),
            raw: Some(Box::new(RawText {
                name: "a\u{7}b".to_string(),
                exe_path: Some("c\u{7}d".to_string()),
                command: Some("e\u{7}f".to_string()),
                cwd: None,
            })),
            argv0: Some("g".to_string()),
            ..ghost()
        };
        let names = record.names();
        assert_eq!(names.name, "a\u{7}b");
        assert_eq!(names.exe, Some("c\u{7}d"));
        assert_eq!(names.argv0, Some("g"));
        assert_eq!(names.command, Some("e\u{7}f"));
    }

    #[test]
    fn the_ancestors_stop_where_the_process_list_does() {
        // A list that holds only this process: its parent is named but not
        // listed, so the climb ends there instead of going on
        let me = Pid::from_u32(std::process::id());
        let mut sys = System::new();
        sysinfo_locked(|| sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[me]), true));
        let parent = sys.process(me).and_then(|p| p.parent());
        let expected: std::collections::HashSet<Pid> = std::iter::once(me).chain(parent).collect();
        assert_eq!(self_and_ancestors(&sys), expected);
    }

    #[test]
    fn test_strip_deleted_marker() {
        assert_eq!(
            strip_deleted_marker("/nonexistent/proc-gone (deleted)".to_string()),
            "/nonexistent/proc-gone"
        );
        assert_eq!(
            strip_deleted_marker("/tmp/project".to_string()),
            "/tmp/project"
        );

        // A directory really named "x (deleted)" keeps its name
        let real = std::env::temp_dir().join(format!("proc-real-{} (deleted)", std::process::id()));
        std::fs::create_dir_all(&real).unwrap();
        let path = real.to_string_lossy().to_string();
        assert_eq!(strip_deleted_marker(path.clone()), path);
        std::fs::remove_dir(&real).unwrap();
    }

    #[test]
    fn a_name_nothing_has_is_plain_not_found_in_both_modes() {
        for mode in [NameMatch::Exact, NameMatch::Command] {
            let result = Process::find_by_name("nonexistent_process_12345", mode);
            assert!(
                matches!(&result, Err(ProcError::ProcessNotFound(n)) if n == "nonexistent_process_12345"),
                "{result:?}"
            );
        }
    }

    /// A shell waiting for input that never comes, with a marker in its command
    /// line: it is named `sh`, and the marker is only in `sh -c ...`. It runs
    /// nothing else, so there is no child to leave behind.
    #[cfg(unix)]
    fn waiting_shell_with(marker: &str) -> std::process::Child {
        std::process::Command::new("sh")
            .args(["-c", &format!("read line; : {marker}")])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn a_word_only_in_a_command_line_is_found_with_cmd_and_hinted_without() {
        let marker = format!("hintmarker{}", std::process::id());
        let mut child = waiting_shell_with(&marker);
        let pid = child.id();
        let exact = Process::find_by_name(&marker, NameMatch::Exact);
        let cmd = Process::find_by_name(&marker, NameMatch::Command);
        let _ = child.kill();
        let _ = child.wait();

        // by default no process is named that, and the error says one has it
        // in its command line
        assert!(
            matches!(&exact, Err(ProcError::NameNotFound { name, in_command_line })
                if name == &marker && *in_command_line >= 1),
            "{exact:?}"
        );
        // with --cmd that process is the match, and the only one
        let found = cmd.unwrap();
        assert_eq!(found.iter().map(|p| p.pid).collect::<Vec<_>>(), vec![pid]);
    }

    #[cfg(unix)]
    #[test]
    fn test_find_orphans_returns_ok() {
        // Should not error — may or may not find orphans depending on system state
        let result = Process::find_orphans();
        assert!(result.is_ok());
    }

    #[cfg(not(unix))]
    #[test]
    fn find_orphans_is_not_supported_on_windows() {
        let result = Process::find_orphans();
        assert!(
            matches!(&result, Err(ProcError::NotSupported(m)) if m.starts_with("orphans is not supported on Windows")),
            "{result:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_is_system_process_system_paths() {
        let make_proc = |exe: Option<&str>, cwd: Option<&str>| Process {
            pid: 100,
            name: "test".to_string(),
            exe_path: exe.map(String::from),
            cwd: cwd.map(String::from),
            command: None,
            argv0: None,
            raw: None,
            cpu_percent: 0.0,
            memory_mb: 0.0,
            status: ProcessStatus::Running,
            user: None,
            parent_pid: Some(1),
            start_time: None,
            uptime_secs: None,
            parent_name: None,
            cpu_time_secs: None,
            open_files: None,
            child_count: None,
            runs_proc: false,
        };

        // System daemons with cwd "/" and system exe paths
        assert!(Process::is_system_process(&make_proc(
            Some("/usr/sbin/sshd"),
            Some("/")
        )));
        assert!(Process::is_system_process(&make_proc(
            Some("/System/Library/foo"),
            Some("/")
        )));
        assert!(Process::is_system_process(&make_proc(
            Some("/usr/bin/systemd"),
            Some("/")
        )));
        assert!(Process::is_system_process(&make_proc(
            Some("/usr/lib/snapd/snapd"),
            Some("/")
        )));

        // No exe path with cwd "/" = kernel thread
        assert!(Process::is_system_process(&make_proc(None, Some("/"))));

        // No cwd at all = likely kernel thread
        assert!(Process::is_system_process(&make_proc(
            Some("/usr/bin/foo"),
            None
        )));

        // Every system directory counts, each checked on its own
        for exe in [
            "/System/x",
            "/usr/libexec/x",
            "/usr/sbin/x",
            "/sbin/x",
            "/usr/bin/x",
            "/usr/lib/x",
            "/usr/lib64/x",
            "/lib/x",
            "/lib64/x",
            "/opt/x",
            "/snap/x",
        ] {
            assert!(
                Process::is_system_process(&make_proc(Some(exe), Some("/"))),
                "{exe}"
            );
        }
        // a daemon with cwd "/" outside them still counts: nothing says it is a user's
        assert!(Process::is_system_process(&make_proc(
            Some("/srv/x"),
            Some("/")
        )));

        // User process with user cwd = NOT system
        assert!(!Process::is_system_process(&make_proc(
            Some("/usr/bin/node"),
            Some("/home/user/project")
        )));
        assert!(!Process::is_system_process(&make_proc(
            Some("/home/user/.local/bin/app"),
            Some("/home/user")
        )));
    }

    #[cfg(unix)]
    #[test]
    fn test_parse_signal_name_valid() {
        use nix::sys::signal::Signal;

        assert_eq!(parse_signal_name("HUP").unwrap(), Signal::SIGHUP);
        assert_eq!(parse_signal_name("hup").unwrap(), Signal::SIGHUP);
        assert_eq!(parse_signal_name("SIGHUP").unwrap(), Signal::SIGHUP);
        assert_eq!(parse_signal_name("sighup").unwrap(), Signal::SIGHUP);
        assert_eq!(parse_signal_name("INT").unwrap(), Signal::SIGINT);
        assert_eq!(parse_signal_name("QUIT").unwrap(), Signal::SIGQUIT);
        assert_eq!(parse_signal_name("ABRT").unwrap(), Signal::SIGABRT);
        assert_eq!(parse_signal_name("KILL").unwrap(), Signal::SIGKILL);
        assert_eq!(parse_signal_name("TERM").unwrap(), Signal::SIGTERM);
        assert_eq!(parse_signal_name("STOP").unwrap(), Signal::SIGSTOP);
        assert_eq!(parse_signal_name("CONT").unwrap(), Signal::SIGCONT);
        assert_eq!(parse_signal_name("USR1").unwrap(), Signal::SIGUSR1);
        assert_eq!(parse_signal_name("USR2").unwrap(), Signal::SIGUSR2);
    }

    #[cfg(unix)]
    #[test]
    fn test_parse_signal_name_invalid() {
        assert!(parse_signal_name("INVALID").is_err());
        assert!(parse_signal_name("FOO").is_err());
        assert!(parse_signal_name("").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_parse_signal_name_case_insensitive() {
        use nix::sys::signal::Signal;

        assert_eq!(parse_signal_name("term").unwrap(), Signal::SIGTERM);
        assert_eq!(parse_signal_name("Term").unwrap(), Signal::SIGTERM);
        assert_eq!(parse_signal_name("TERM").unwrap(), Signal::SIGTERM);
        assert_eq!(parse_signal_name("sigterm").unwrap(), Signal::SIGTERM);
        assert_eq!(parse_signal_name("SigTerm").unwrap(), Signal::SIGTERM);
    }

    #[cfg(unix)]
    #[test]
    fn test_send_signal_nonexistent_process() {
        use nix::sys::signal::Signal;

        // PID 99999999 almost certainly doesn't exist
        let proc = Process {
            pid: 99999999,
            name: "ghost".to_string(),
            exe_path: None,
            cwd: None,
            command: None,
            argv0: None,
            raw: None,
            cpu_percent: 0.0,
            memory_mb: 0.0,
            status: ProcessStatus::Running,
            user: None,
            parent_pid: None,
            start_time: None,
            uptime_secs: None,
            parent_name: None,
            cpu_time_secs: None,
            open_files: None,
            child_count: None,
            runs_proc: false,
        };

        let result = proc.send_signal(Signal::SIGCONT);
        assert!(result.is_err());
    }

    /// `sh -c 'sleep 30 & wait'`: a shell with exactly one child, a `sleep`.
    /// Returns the shell, which the caller kills and reaps (the sleep ends with
    /// its own 30 s; the shell's death leaves it to init).
    #[cfg(unix)]
    fn shell_with_one_child() -> std::process::Child {
        std::process::Command::new("sh")
            .args(["-c", "sleep 30 & wait"])
            .spawn()
            .unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn a_process_records_its_parents_name_and_its_children() {
        let mut shell = shell_with_one_child();
        let shell_pid = shell.id();
        // the shell starts its sleep after the exec: wait for the child to show.
        // The sleep's parent name is checked against the shell in the same read:
        // macOS's sh is a launcher that turns into bash, and one read of the
        // whole table takes long enough under load to see the shell as sh and
        // its sleep, started by bash, both (the unit tests on a Mac failed 4 of 8
        // runs on it, 2026-11-11)
        let child_of_shell = || {
            let all = Process::find_all().unwrap();
            let sleeper = all.iter().find(|p| {
                p.parent_pid == Some(shell_pid) && p.pid != shell_pid && p.name == "sleep"
            })?;
            let shell = all.iter().find(|p| p.pid == shell_pid)?;
            Some((sleeper.clone(), shell.name.clone()))
        };
        let seen = std::iter::repeat_with(|| {
            std::thread::sleep(Duration::from_millis(10));
            child_of_shell()
        })
        .take(500)
        .find(Option::is_some)
        .flatten();
        let all = Process::find_all().unwrap();
        let by_pid = Process::find_by_pid(shell_pid).unwrap();
        let _ = shell.kill();
        let _ = shell.wait();

        let (sleeper, shell_name) = seen.expect("the shell never started its sleep");
        let shell_record = all.iter().find(|p| p.pid == shell_pid).unwrap();
        assert_eq!(shell_record.child_count, Some(1), "{shell_record:?}");
        assert_eq!(sleeper.parent_name.as_deref(), Some(shell_name.as_str()));
        // a leaf has no children, which is a count of 0, not an unknown
        assert_eq!(sleeper.child_count, Some(0), "{sleeper:?}");
        // a lookup by PID reads the whole table too, and fills the same facts
        let by_pid = by_pid.unwrap();
        assert_eq!(by_pid.child_count, Some(1), "{by_pid:?}");
        assert_eq!(by_pid.parent_name, shell_record.parent_name);
    }

    #[test]
    fn proc_does_not_count_itself_among_the_children_of_its_parent() {
        // one snapshot of the table gives the record and the expected count
        let sys = refreshed_system();
        let own = Pid::from_u32(std::process::id());
        let parent = sys
            .process(own)
            .and_then(|p| p.parent())
            .expect("the test process has a parent");
        let mut record = [Process::from_sysinfo(parent, sys.process(parent).unwrap())];
        fill_family(&mut record, &sys);
        let others = sys
            .processes()
            .values()
            .filter(|p| !is_thread(p) && p.parent() == Some(parent) && p.pid() != own)
            .count();
        assert_eq!(record[0].child_count, Some(others as u32), "{record:?}");
    }

    #[test]
    fn cpu_time_and_open_files_are_read_for_own_processes() {
        let me = Process::find_by_pid(std::process::id()).unwrap().unwrap();
        assert!(me.cpu_time_secs.is_some(), "{me:?}");
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        assert!(me.open_files >= Some(3), "stdin, stdout, stderr: {me:?}");
        // the full list carries them too (it leaves proc's own process out)
        let others = Process::find_all().unwrap();
        assert!(others.iter().any(|p| p.cpu_time_secs.is_some()));
    }

    #[test]
    fn cpu_time_is_whole_seconds_of_the_cpu_time_read() {
        // The same reading `cpu_time_ms` gives, divided by 1000
        let sys = refreshed_system();
        let me = Pid::from_u32(std::process::id());
        let record = Process::from_sysinfo(me, sys.process(me).unwrap());
        let ms = cpu_time_ms(me, sys.process(me).unwrap());
        let secs = record.cpu_time_secs.unwrap();
        // proc reads the CPU time again for the record, a moment later
        assert!(secs >= ms / 1000 && secs <= ms / 1000 + 1, "{secs} vs {ms}");
    }

    #[test]
    fn a_reading_of_zero_from_an_unread_process_is_unknown() {
        // a process that used 1.5 s of CPU, and one that used none but is read
        assert_eq!(cpu_time_secs_read(1500, 10 << 20), Some(1));
        assert_eq!(cpu_time_secs_read(0, 10 << 20), Some(0));
        // used nothing and has no memory: macOS and Windows did not read it
        assert_eq!(
            cpu_time_secs_read(0, 0),
            if cfg!(any(target_os = "macos", windows)) {
                None
            } else {
                Some(0)
            }
        );
        // Windows counts handles, not files
        assert_eq!(
            open_files_read(Some(7)),
            if cfg!(windows) { None } else { Some(7) }
        );
        assert_eq!(open_files_read(None), None);
        // macOS answers 0 for a process that is not the user's
        assert_eq!(
            open_files_read(Some(0)),
            if cfg!(any(target_os = "macos", windows)) {
                None
            } else {
                Some(0)
            }
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn another_users_process_has_no_cpu_time_or_open_files_on_macos() {
        // PID 1 (launchd) is root's: sysinfo can read it only when the test
        // itself runs as root
        let readable = nix::unistd::geteuid().is_root();
        let launchd = Process::find_by_pid(1).unwrap().unwrap();
        assert_eq!(launchd.cpu_time_secs.is_some(), readable, "{launchd:?}");
        assert_eq!(launchd.open_files.is_some(), readable, "{launchd:?}");
        // the family comes from the table, not from reading the process
        assert!(launchd.child_count > Some(0), "{launchd:?}");
    }

    #[test]
    fn unknown_facts_are_absent_from_json() {
        let json = serde_json::to_value(ghost()).unwrap();
        for key in ["parent_name", "cpu_time_secs", "open_files", "child_count"] {
            assert!(json.get(key).is_none(), "{key} must be absent: {json}");
        }
        // known facts are present, under the spec's names
        let mut known = ghost();
        known.parent_name = Some("zsh".to_string());
        known.cpu_time_secs = Some(7);
        known.open_files = Some(9);
        known.child_count = Some(2);
        let json = serde_json::to_value(known).unwrap();
        assert_eq!(json["parent_name"], "zsh");
        assert_eq!(json["cpu_time_secs"], 7);
        assert_eq!(json["open_files"], 9);
        assert_eq!(json["child_count"], 2);
    }

    #[test]
    fn a_zero_is_a_reading_only_with_something_beside_it() {
        // no memory and no CPU time: where the OS refuses (macOS, Windows) that is
        // "not read"; on Linux and the BSDs the files are readable, so it is a reading
        let mut p = ghost();
        assert_eq!(p.read_by_os(), !cfg!(any(target_os = "macos", windows)));
        // memory in use means the process was read
        p.memory_mb = 0.5;
        assert!(p.read_by_os());
        // CPU time without memory (a zombie, a kernel thread): read, on every OS
        p.memory_mb = 0.0;
        p.cpu_time_secs = Some(0);
        assert!(p.read_by_os());
    }

    /// A child that has exited and is not reaped: a zombie, which `ps` shows
    /// once it is (polled with a deadline, not slept for). Past the deadline
    /// the child comes back as it is, and the test's own checks fail.
    #[cfg(unix)]
    fn unreaped_zombie() -> std::process::Child {
        let child = std::process::Command::new("sh")
            .args(["-c", "exit 0"])
            .stdin(std::process::Stdio::null())
            .spawn()
            .expect("spawn a shell");
        let is_zombie = || {
            let ps = std::process::Command::new("ps")
                .args(["-o", "stat=", "-p", &child.id().to_string()])
                .output()
                .expect("run ps");
            String::from_utf8_lossy(&ps.stdout).trim().starts_with('Z')
        };
        // Wait, then look, until `ps` shows the zombie, for up to 30 s (1500
        // looks 20 ms apart). No loop: a loop has lines that run only on some
        // timings (its body, or the code after a check), and the coverage gate
        // failed on each of them in turn on 2026-11-04. The closure runs in
        // full at least once whatever the timing
        let _zombie = (0..1500).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            is_zombie()
        });
        child
    }

    #[cfg(unix)]
    #[test]
    fn a_zombie_is_found_by_pid_as_a_zombie_on_every_os() {
        let mut child = unreaped_zombie();
        let found = Process::find_by_pid(child.id()).unwrap();
        let _ = child.wait();
        let found = found.expect("a zombie is found by its PID, as the list shows it");
        assert_eq!(found.pid, child.id());
        assert_eq!(found.status, ProcessStatus::Zombie);
        assert_eq!(found.parent_pid, Some(std::process::id()));
        // it has exited: not running, as `wait` counts it
        assert!(!found.is_running());
    }

    /// A signal to a zombie is delivered and changes nothing, on every OS: no
    /// command reports "not found" for a process that `list` shows
    #[cfg(unix)]
    #[test]
    fn a_zombie_takes_every_signal_and_counts_as_exited() {
        let mut child = unreaped_zombie();
        let record = Process::find_by_pid(child.id()).unwrap().unwrap();
        let results = [
            record.send_signal(nix::sys::signal::Signal::SIGSTOP),
            record.send_signal(nix::sys::signal::Signal::SIGCONT),
            record.terminate(),
            record.kill(),
            record.kill_and_wait(),
        ];
        let _ = child.wait();
        for result in results {
            assert!(result.is_ok(), "{result:?}");
        }
    }

    /// A zombie's PID reused by another process is not the record's zombie
    #[cfg(target_os = "macos")]
    #[test]
    fn a_record_of_another_start_time_is_not_the_zombie() {
        let mut child = unreaped_zombie();
        let mut record = Process::find_by_pid(child.id()).unwrap().unwrap();
        record.start_time = record.start_time.map(|t| t + 100);
        let results = [record.terminate(), record.kill()];
        let _ = child.wait();
        for result in results {
            assert!(
                matches!(result, Err(ProcError::ProcessNotFound(_))),
                "{result:?}"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_kernel_is_asked_for_a_zombie_by_pid_and_only_for_a_zombie() {
        let mut child = unreaped_zombie();
        let zombie = macos_zombie(child.id());
        let _ = child.wait();
        assert_eq!(zombie.expect("the zombie").pid, child.id());
        // alive, and no process at all
        assert!(macos_zombie(std::process::id()).is_none());
        assert!(macos_zombie(4_000_000).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn sockets_are_visible_for_ones_own_processes_and_for_root() {
        let me = nix::unistd::Uid::effective();
        let mut p = ghost();
        // an owner nobody has: only root sees its sockets
        p.user = Some("no-such-user-for-proc-tests".to_string());
        assert_eq!(p.sockets_visible(), me.is_root());
        p.user = None;
        assert_eq!(p.sockets_visible(), me.is_root());
        // the user running proc
        let name = nix::unistd::User::from_uid(me).unwrap().unwrap().name;
        p.user = Some(name);
        assert!(p.sockets_visible());
    }

    #[cfg(windows)]
    #[test]
    fn windows_sees_every_pids_sockets() {
        assert!(ghost().sockets_visible());
    }

    /// A record of a process that doesn't exist
    fn ghost() -> Process {
        Process {
            pid: 99999999,
            name: "ghost".to_string(),
            exe_path: None,
            cwd: None,
            command: None,
            argv0: None,
            raw: None,
            cpu_percent: 0.0,
            memory_mb: 0.0,
            status: ProcessStatus::Running,
            user: None,
            parent_pid: None,
            start_time: None,
            uptime_secs: None,
            parent_name: None,
            cpu_time_secs: None,
            open_files: None,
            child_count: None,
            runs_proc: false,
        }
    }

    #[test]
    fn every_sysinfo_status_maps_to_ours() {
        use ProcessStatus::{DiskWait, Idle, Starting, Unknown};
        // What sysinfo's Idle means, per OS: SIDL (a process being created) on macOS and
        // the BSDs, an idle kernel thread on Linux, nothing elsewhere
        for (os, idle) in [
            ("linux", Idle),
            ("android", Idle),
            ("macos", Starting),
            ("freebsd", Starting),
            ("netbsd", Starting),
            ("windows", Unknown),
            ("openbsd", Unknown),
        ] {
            assert_eq!(
                ProcessStatus::from_sysinfo_on(SysProcessStatus::Idle, os),
                idle,
                "{os}"
            );
        }
        // Everything else is the same on every OS
        for os in ["linux", "macos", "freebsd", "netbsd", "windows"] {
            for (sys, ours) in [
                (SysProcessStatus::Run, ProcessStatus::Running),
                (SysProcessStatus::Sleep, ProcessStatus::Sleeping),
                (SysProcessStatus::Stop, ProcessStatus::Stopped),
                (SysProcessStatus::Zombie, ProcessStatus::Zombie),
                (SysProcessStatus::Dead, ProcessStatus::Dead),
                (SysProcessStatus::UninterruptibleDiskSleep, DiskWait),
                (SysProcessStatus::LockBlocked, Unknown),
                (SysProcessStatus::Suspended, Unknown),
                (SysProcessStatus::Tracing, Unknown),
                (SysProcessStatus::Parked, Unknown),
                (SysProcessStatus::Wakekill, Unknown),
                (SysProcessStatus::Waking, Unknown),
                (SysProcessStatus::Unknown(7), Unknown),
            ] {
                assert_eq!(
                    ProcessStatus::from_sysinfo_on(sys, os),
                    ours,
                    "{os} {sys:?}"
                );
            }
        }
        // ... and the conversion proc uses is the one for this OS
        assert_eq!(
            ProcessStatus::from(SysProcessStatus::Idle),
            ProcessStatus::of_sysinfo_idle(std::env::consts::OS)
        );
        assert_eq!(
            ProcessStatus::from(SysProcessStatus::Run),
            ProcessStatus::Running
        );
    }

    #[test]
    fn a_status_is_a_lowercase_word_in_json_and_the_new_ones_round_trip() {
        for (status, json, word) in [
            (ProcessStatus::Running, "running", "running"),
            (ProcessStatus::Sleeping, "sleeping", "sleeping"),
            (ProcessStatus::Stopped, "stopped", "stopped"),
            (ProcessStatus::Zombie, "zombie", "zombie"),
            (ProcessStatus::Dead, "dead", "dead"),
            (ProcessStatus::Idle, "idle", "idle"),
            (ProcessStatus::Starting, "starting", "starting"),
            (ProcessStatus::DiskWait, "disk_wait", "disk wait"),
            (ProcessStatus::Unknown, "unknown", "unknown"),
        ] {
            let text = serde_json::to_string(&status).unwrap();
            assert_eq!(text, format!("\"{json}\""));
            assert_eq!(
                serde_json::from_str::<ProcessStatus>(&text).unwrap(),
                status
            );
            assert_eq!(status.word(), word);
        }
    }

    #[test]
    fn a_process_that_is_gone_does_not_look_stuck() {
        assert!(!ghost().looks_stuck_now());
    }

    #[test]
    fn killing_a_process_that_is_already_gone_is_success() {
        assert!(matches!(ghost().kill(), Err(ProcError::ProcessNotFound(_))));
        ghost().kill_and_wait().unwrap();
    }

    #[test]
    fn a_kill_that_was_not_delivered_is_an_error() {
        kill_outcome(true, 7).unwrap();
        let err = kill_outcome(false, 7).unwrap_err();
        assert!(
            matches!(&err, ProcError::SignalError(m) if m == "Failed to kill process 7"),
            "{err:?}"
        );
    }

    #[test]
    fn taskkill_without_force_reports_what_it_said() {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt;
        use std::process::{ExitStatus, Output};

        let output = |raw, stdout: &str, stderr: &str| Output {
            status: ExitStatus::from_raw(raw),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        };
        taskkill_outcome(&output(0, "SUCCESS: sent termination signal", "")).unwrap();

        // exit code 1 on Windows; the same wait status on Unix
        #[cfg(unix)]
        let failed = 1 << 8;
        #[cfg(windows)]
        let failed = 1;
        let err =
            taskkill_outcome(&output(failed, "", "ERROR: could not be terminated")).unwrap_err();
        assert!(
            matches!(&err, ProcError::SignalError(m) if m == "ERROR: could not be terminated"),
            "{err:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn orphans_are_the_children_of_init_that_are_not_system_daemons() {
        let proc = |pid: u32, ppid: Option<u32>, exe: Option<&str>, cwd: Option<&str>| Process {
            pid,
            name: format!("p{pid}"),
            exe_path: exe.map(String::from),
            cwd: cwd.map(String::from),
            parent_pid: ppid,
            ..ghost()
        };
        let orphans = Process::orphans_among(vec![
            // a user's process whose parent died
            proc(10, Some(1), Some("/home/z/app"), Some("/home/z")),
            // a system daemon: cwd "/" and an exe in a system directory
            proc(11, Some(1), Some("/usr/sbin/sshd"), Some("/")),
            // init itself, and a process with a live parent
            proc(1, Some(1), Some("/sbin/init"), Some("/home/z")),
            proc(12, Some(500), Some("/home/z/app"), Some("/home/z")),
            // no parent known
            proc(13, None, Some("/home/z/app"), Some("/home/z")),
        ]);
        let pids: Vec<u32> = orphans.iter().map(|p| p.pid).collect();
        assert_eq!(pids, vec![10]);
    }

    // A missing lsof or fstat used to read as "no process holds the file"
    #[cfg(unix)]
    #[test]
    fn a_missing_open_file_program_is_an_error_naming_it() {
        let found = open_file_pids_with("proc-no-such-lsof", std::path::Path::new("/"));
        assert!(
            matches!(&found, Err(ProcError::NotSupported(why)) if why.contains("proc-no-such-lsof")),
            "{found:?}"
        );
    }

    /// A fake `/proc`: PID entries with the links and maps a holder has
    #[cfg(unix)]
    fn fake_proc(target: &std::path::Path) -> tempfile::TempDir {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let entry = |pid: &str| {
            let dir = root.path().join(pid);
            std::fs::create_dir_all(dir.join("fd")).unwrap();
            dir
        };
        let other = std::path::Path::new("/somewhere/else");
        symlink(target, entry("10").join("fd").join("3")).unwrap();
        symlink(target, entry("11").join("cwd")).unwrap();
        std::fs::write(
            entry("12").join("maps"),
            format!(
                "55d0c0a00000-55d0c0a21000 rw-p 00000000 00:00 0                          [heap]\n\
                 7f1c2a000000-7f1c2a021000 r-xp 00000000 08:01 1234                       {}\n",
                target.display()
            ),
        )
        .unwrap();
        symlink(other, entry("13").join("exe")).unwrap();
        symlink(other, entry("13").join("fd").join("0")).unwrap();
        symlink(target, entry("14").join("root")).unwrap();
        symlink(target, entry("15").join("exe")).unwrap();
        // not PID entries: a file, and a name that isn't a number
        std::fs::write(root.path().join("uptime"), "1.0 1.0\n").unwrap();
        symlink(target, entry("self").join("cwd")).unwrap();
        root
    }

    #[cfg(unix)]
    #[test]
    fn every_way_a_process_uses_a_file_is_found_in_proc() {
        let target = std::path::Path::new("/srv/app/data file.db");
        let root = fake_proc(target);
        let pids = linux_open_file_holders(root.path(), target).unwrap();
        // 10 an open descriptor, 11 the working directory, 12 a mapping,
        // 14 the root directory, 15 the executable; 13 uses another file
        assert_eq!(pids, vec![10, 11, 12, 14, 15]);
    }

    #[cfg(unix)]
    #[test]
    fn a_proc_that_cannot_be_read_is_an_error_not_an_empty_answer() {
        let missing = std::path::Path::new("/proc-no-such-dir");
        let found = linux_open_file_holders(missing, std::path::Path::new("/"));
        assert!(
            matches!(&found, Err(ProcError::SystemError(why)) if why.contains("/proc-no-such-dir")),
            "{found:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_maps_line_gives_the_file_it_maps() {
        let line =
            |rest: &str| format!("7f1c2a000000-7f1c2a021000 r-xp 00000000 08:01 1234   {rest}");
        let mapped = line("/usr/lib/libc.so.6");
        assert_eq!(
            mapped_path(&mapped),
            Some(std::path::Path::new("/usr/lib/libc.so.6"))
        );
        // a path keeps its spaces
        let spaced = line("/srv/my app/lib.so");
        assert_eq!(
            mapped_path(&spaced),
            Some(std::path::Path::new("/srv/my app/lib.so"))
        );
        // anonymous memory and named regions map no file
        assert_eq!(
            mapped_path("7f1c2a000000-7f1c2a021000 rw-p 00000000 00:00 0"),
            None
        );
        assert_eq!(mapped_path(&line("[stack]")), None);
        assert_eq!(mapped_path("garbage"), None);
    }

    #[test]
    fn lsof_prints_one_pid_a_line() {
        assert_eq!(lsof_pids("1851\n2614\n"), vec![1851, 2614]);
        assert!(lsof_pids("").is_empty());
    }

    #[test]
    fn fstat_rows_give_the_pids_that_hold_the_file() {
        // as FreeBSD 15.1 and NetBSD 11.0 printed them
        let freebsd = "USER     CMD          PID   FD MOUNT      INUM MODE         SZ|DV R/W NAME\n\
            proc     sleep       2614    3 /        1445173 -rw-r--r--       5  r  /home/proc/held.txt\n";
        let netbsd = "USER     CMD          PID   FD  MOUNT         INUM MODE         SZ|DV R/W NAME\n\
            proc     sleep       1851    3  /           425799 -rw-r--r--       5 r   /home/proc/held.txt\n";
        assert_eq!(fstat_pids(freebsd), vec![2614]);
        assert_eq!(fstat_pids(netbsd), vec![1851]);
        // nobody holds it: the header alone
        assert!(fstat_pids(
            "USER     CMD          PID   FD MOUNT      INUM MODE         SZ|DV R/W NAME\n"
        )
        .is_empty());
        // a command name with a space, a path with digits and a space, and one
        // process listed for two descriptors
        let rows =
            "proc     Web Content  4021   12 /  77 -rw-r--r--  5 r  /home/proc/2026 notes.txt\n\
            proc     Web Content  4021   13 /  77 -rw-r--r--  5 r  /home/proc/2026 notes.txt\n\
            root     sshd           90  text /  12 -r-xr-xr-x  9 r  /home/proc/2026 notes.txt\n";
        assert_eq!(fstat_pids(rows), vec![90, 4021]);
        // command names made of numbers are not taken for the PID: an
        // executable named "4021", and one named "Web 4021"
        let numeric =
            "proc     4021          777    3 /home  77 -rw-r--r--  5 r  /home/proc/held.txt\n\
            proc     Web 4021      778    3 /home  77 -rw-r--r--  5 r  /home/proc/held.txt\n";
        assert_eq!(fstat_pids(numeric), vec![777, 778]);
        // a row with no mount point, or no command name before the PID, is left out
        assert!(
            fstat_pids("proc     sleep  2614    3 -  77 -rw-r--r--  5 r  held.txt\n").is_empty()
        );
        assert!(fstat_pids("proc  2614    3 /  77 -rw-r--r--  5 r  /h\n").is_empty());
    }

    #[test]
    fn a_wait_for_a_process_that_stays_is_a_wait_and_then_an_error() {
        let mut child = crate::test_support::idle_child();
        let record = crate::test_support::record_of(&child);
        let started = std::time::Instant::now();
        // a kill that does nothing: the process is still there, however long
        // it is looked at
        let result = record.kill_then_wait(Duration::from_millis(300), |_| Ok(()));
        let waited = started.elapsed();
        let still_there = child.try_wait().unwrap().is_none();
        crate::test_support::end(&mut child);
        assert!(
            matches!(&result, Err(ProcError::SignalError(_))),
            "{result:?}"
        );
        assert!(waited >= Duration::from_millis(300), "{waited:?}");
        assert!(still_there);
    }

    #[test]
    fn a_process_still_alive_when_the_wait_ends_is_an_error() {
        let mut child = crate::test_support::idle_child();
        let record = crate::test_support::record_of(&child);
        // no time to wait: the SIGKILL is sent, but the process isn't seen gone
        let result = record.kill_and_wait_within(Duration::ZERO);
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            matches!(&result, Err(ProcError::SignalError(m))
                if m == &format!("Process {} is still running 0 seconds after SIGKILL", record.pid)),
            "{result:?}"
        );
    }

    /// One kinfo_proc record, with the fields `zombies_in` reads
    #[cfg(target_os = "macos")]
    fn kinfo(pid: u32, stat: u8, comm: &str, ruid: u32, ppid: u32, start: u64) -> Vec<u8> {
        let mut rec = vec![0u8; KINFO_PROC_SIZE];
        rec[0..8].copy_from_slice(&start.to_ne_bytes());
        rec[36] = stat;
        rec[40..44].copy_from_slice(&pid.to_ne_bytes());
        rec[243..243 + comm.len()].copy_from_slice(comm.as_bytes());
        rec[392..396].copy_from_slice(&ruid.to_ne_bytes());
        rec[560..564].copy_from_slice(&ppid.to_ne_bytes());
        rec
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn zombies_are_read_from_the_kernels_table() {
        let known = std::collections::HashSet::from([30]);
        let table: Vec<u8> = [
            kinfo(10, 5, "defunct-app", 0, 99, 1_700_000_000),
            // no name left, and no start time
            kinfo(11, 5, "", 0, 1, 0),
            // alive, not a zombie
            kinfo(20, 2, "alive", 0, 1, 1_700_000_000),
            // already listed by sysinfo
            kinfo(30, 5, "known", 0, 1, 1_700_000_000),
            // the kernel's own slot
            kinfo(0, 5, "kernel", 0, 0, 0),
        ]
        .concat();

        let zombies = zombies_in(&table, &known);
        let pids: Vec<u32> = zombies.iter().map(|z| z.pid).collect();
        assert_eq!(pids, vec![10, 11]);

        let named = &zombies[0];
        assert_eq!(named.name, "defunct-app");
        assert_eq!(named.status, ProcessStatus::Zombie);
        assert_eq!(named.parent_pid, Some(99));
        assert_eq!(named.start_time, Some(1_700_000_000));
        assert!(named.uptime_secs.unwrap() > 0);
        assert_eq!(named.user.as_deref(), Some("root"));

        let unnamed = &zombies[1];
        assert_eq!(unnamed.name, "<defunct>");
        assert_eq!(unnamed.start_time, None);
        assert_eq!(unnamed.uptime_secs, None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_table_of_an_unknown_layout_shows_no_zombies() {
        let known = std::collections::HashSet::new();
        let mut table = kinfo(10, 5, "z", 0, 1, 0);
        table.push(0);
        assert!(zombies_in(&table, &known).is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_refused_sysctl_gives_an_empty_table() {
        use std::cell::Cell;

        // refused when asked for the size
        assert!(read_proc_table(|_, _| -1).is_empty());

        // refused when asked for the table
        let calls = Cell::new(0);
        let table = read_proc_table(|_, size| {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                *size = 2 * KINFO_PROC_SIZE;
                0
            } else {
                -1
            }
        });
        assert_eq!(calls.get(), 2);
        assert!(table.is_empty());

        // answered: the table is cut to the size the kernel reports
        let calls = Cell::new(0);
        let table = read_proc_table(|buf, size| {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                *size = 2 * KINFO_PROC_SIZE;
            } else {
                assert_eq!(*size, 66 * KINFO_PROC_SIZE, "room for 64 newcomers");
                // SAFETY: `buf` has `*size` writable bytes
                unsafe { buf.cast::<u8>().write(7) };
                *size = KINFO_PROC_SIZE;
            }
            0
        });
        assert_eq!(table.len(), KINFO_PROC_SIZE);
        assert_eq!(table[0], 7);
    }
}
