//! Shared filter utilities used across commands
//!
//! Provides common filter resolution logic to avoid duplication.

use crate::core::{
    flatten_targets, matches_name, parse_target, NameMatch, PortInfo, Process, ProcessStatus,
    TargetType,
};
use crate::error::{ProcError, Result};
use clap::ValueEnum;
use std::path::{Path, PathBuf};

/// Values accepted by `--status`. An unknown value is a usage error (exit 4),
/// not a filter that silently matches everything.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum StatusFilter {
    /// Running on a CPU
    Running,
    /// Waiting (idle, blocked on I/O)
    #[value(alias = "sleep")]
    Sleeping,
    /// Paused (e.g. by `proc freeze` or Ctrl+Z)
    #[value(alias = "stop")]
    Stopped,
    /// Exited, but not yet reaped by its parent
    Zombie,
}

impl StatusFilter {
    /// Whether a process with this status passes the filter
    pub fn matches(self, status: ProcessStatus) -> bool {
        matches!(
            (self, status),
            (StatusFilter::Running, ProcessStatus::Running)
                | (StatusFilter::Sleeping, ProcessStatus::Sleeping)
                | (StatusFilter::Stopped, ProcessStatus::Stopped)
                | (StatusFilter::Zombie, ProcessStatus::Zombie)
        )
    }
}

/// Sort key for process commands (list, by, in, for)
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SortKey {
    /// Sort by CPU usage (descending)
    Cpu,
    /// Sort by memory usage (descending)
    Mem,
    /// Sort by process ID (ascending)
    Pid,
    /// Sort by process name (ascending)
    Name,
}

/// Sort key for port commands
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PortSortKey {
    /// Sort by port number (ascending)
    Port,
    /// Sort by process ID (ascending)
    Pid,
    /// Sort by process name (ascending)
    Name,
}

/// Sort a list of processes by the given key; processes that tie on the key
/// come in PID ascending order, so the same machine gives the same list (and
/// the same rows under `--limit`) however the input was ordered.
pub fn sort_processes(processes: &mut [Process], key: SortKey) {
    match key {
        SortKey::Cpu => processes.sort_by(|a, b| {
            b.cpu_percent
                .partial_cmp(&a.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.pid.cmp(&b.pid))
        }),
        SortKey::Mem => processes.sort_by(|a, b| {
            b.memory_mb
                .partial_cmp(&a.memory_mb)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.pid.cmp(&b.pid))
        }),
        SortKey::Pid => processes.sort_by_key(|p| p.pid),
        SortKey::Name => processes.sort_by_cached_key(|p| (p.name.to_lowercase(), p.pid)),
    }
}

/// Sort a list of listening ports by the given key. Ports that tie on the key
/// come by port ascending, then PID ascending: a process's own ports read in
/// ascending order under `--sort pid` and `--sort name`.
pub fn sort_ports(ports: &mut [PortInfo], key: PortSortKey) {
    match key {
        PortSortKey::Port => ports.sort_by_key(|p| (p.port, p.pid)),
        PortSortKey::Pid => ports.sort_by_key(|p| (p.pid, p.port)),
        PortSortKey::Name => {
            ports.sort_by_cached_key(|p| (p.process_name.to_lowercase(), p.port, p.pid))
        }
    }
}

/// Check if a process matches a `--by` name filter.
///
/// The same comparison as a positional name target (see [`NameMatch`]): the
/// whole name by default, any part of the name or command line with `--cmd`.
pub fn matches_by_filter(process: &Process, pattern: &str, mode: NameMatch) -> bool {
    matches_name(pattern, &process.names(), mode)
}

/// Check if a process matches an `--in` directory filter. Compares the real
/// working directory, not the escaped text shown for it (see [`Process::raw_cwd`]).
pub fn matches_in_filter(process: &Process, dir_path: &std::path::Path) -> bool {
    process
        .raw_cwd()
        .is_some_and(|cwd| cwd_within(cwd, dir_path))
}

/// Whether a process's cwd is `dir` or under it.
///
/// `dir` is already in the OS's own form (see [`resolve_dir`]). A Windows cwd
/// is not always: it comes back in whatever form the process was started
/// with. Measured on a GitHub runner, a process spawned with cwd
/// `C:\Users\RUNNER~1\AppData\Local\Temp\.tmpX` reports
/// `"cwd": "C:\\Users\\RUNNER~1\\AppData\\Local\\Temp\\.tmpX\\"` (short name) while its
/// `exe_path` is `C:\Users\runneradmin\...` (long). So when the plain
/// comparison fails, on Windows the cwd is canonicalized too and compared again.
pub fn cwd_within(cwd: &str, dir: &Path) -> bool {
    #[cfg(windows)]
    let canonicalize = |p: &Path| p.canonicalize().ok();
    #[cfg(not(windows))]
    let canonicalize = |_: &Path| None;
    cwd_within_with(cwd, dir, cfg!(windows), canonicalize)
}

/// [`cwd_within`] with the canonicalizer and the platform switch as inputs,
/// so every OS can test the Windows path with a fake canonicalizer.
fn cwd_within_with(
    cwd: &str,
    dir: &Path,
    retry_canonical: bool,
    canonicalize: impl Fn(&Path) -> Option<PathBuf>,
) -> bool {
    if Path::new(cwd).starts_with(dir) {
        return true;
    }
    if !retry_canonical {
        return false;
    }
    match canonicalize(Path::new(cwd)) {
        Some(canonical) => {
            PathBuf::from(strip_verbatim_prefix(&canonical.to_string_lossy())).starts_with(dir)
        }
        None => false,
    }
}

/// Apply `--in` and `--by` filters to a list of processes (in a single pass).
///
/// This replaces the 14-line retain block duplicated across commands.
pub fn apply_filters(
    processes: &mut Vec<Process>,
    in_dir: &Option<String>,
    by_name: &Option<String>,
    mode: NameMatch,
) -> Result<()> {
    let in_dir_filter = resolve_in_dir(in_dir)?;
    let in_dir_has_any = in_dir_filter
        .as_ref()
        .is_some_and(|dir_path| processes.iter().any(|p| matches_in_filter(p, dir_path)));
    require_in_dir(in_dir, &in_dir_filter, in_dir_has_any)?;
    processes.retain(|p| {
        if let Some(ref dir_path) = in_dir_filter {
            if !matches_in_filter(p, dir_path) {
                return false;
            }
        }
        if let Some(ref name) = by_name {
            if !matches_by_filter(p, name, mode) {
                return false;
            }
        }
        true
    });
    Ok(())
}

/// The guards a command that acts on targets takes: what the caller saw of the
/// process it means
#[derive(Debug, Clone, Default)]
pub struct Guards {
    /// `--in`: the directory it runs in
    pub in_dir: Option<String>,
    /// `--by`: its name
    pub by_name: Option<String>,
    /// `--start-time`: when it started, Unix seconds (`start_time` in its JSON)
    pub start_time: Option<u64>,
}

impl Guards {
    /// The guards from a command's `--in`, `--by` and `--start-time`
    pub fn new(in_dir: &Option<String>, by_name: &Option<String>, start_time: Option<u64>) -> Self {
        Guards {
            in_dir: in_dir.clone(),
            by_name: by_name.clone(),
            start_time,
        }
    }
}

/// [`apply_filters`] for a command that acts on targets. `--by`, `--in` and
/// `--start-time` are guards there (act on this PID only if it is still this
/// process), so when they rule out everything the targets found, that is an
/// error that names them: it used to read like a plain not-found, so an agent
/// took a mistyped guard for a recycled PID and left the process running.
///
/// `--start-time` is the exact guard: the OS gives a PID to a new process once
/// the old one exits (a busy Mac cycles through every PID in under an hour), and
/// a new process can have the same name and directory. It describes one
/// process, so it takes one PID target, and a process whose start time can't
/// be read (another user's, on macOS) can't be confirmed, so it is ruled out.
pub fn guard_targets(
    processes: &mut Vec<Process>,
    targets: &[String],
    guards: &Guards,
    mode: NameMatch,
) -> Result<()> {
    if guards.start_time.is_some() {
        let all = flatten_targets(targets);
        if !matches!(all.as_slice(), [one] if matches!(parse_target(one), TargetType::Pid(_))) {
            return Err(ProcError::InvalidInput(
                "--start-time describes one process: give it a single PID target".to_string(),
            ));
        }
    }
    let found = processes.clone();
    apply_filters(processes, &guards.in_dir, &guards.by_name, mode)?;
    if let Some(seen) = guards.start_time {
        processes.retain(|p| started_at(p, seen));
    }
    if found.is_empty() || !processes.is_empty() {
        return Ok(());
    }
    Err(ProcError::FilteredOut {
        target: targets.join(" "),
        ruled_out_by: ruled_out_by(&found, guards, mode)?,
    })
}

/// Whether the process started at `seen` (Unix seconds), to within a second.
/// A start time that can't be read doesn't match: nothing confirms it.
fn started_at(process: &Process, seen: u64) -> bool {
    process
        .start_time
        .is_some_and(|now| now.abs_diff(seen) <= 1)
}

/// The guards that rule out a process in `found`, each as its flag and the
/// value typed, for [`ProcError::FilteredOut`]. Only those that fail count:
/// `--by sleep --start-time 1` on a `sleep` names the start time alone.
pub fn ruled_out_by(
    found: &[Process],
    guards: &Guards,
    mode: NameMatch,
) -> Result<Vec<(String, String)>> {
    let mut ruled_out = Vec::new();
    if let Some(name) = &guards.by_name {
        if found.iter().any(|p| !matches_by_filter(p, name, mode)) {
            ruled_out.push(("--by".to_string(), name.clone()));
        }
    }
    if let (Some(typed), Some(dir)) = (&guards.in_dir, resolve_in_dir(&guards.in_dir)?) {
        if found.iter().any(|p| !matches_in_filter(p, &dir)) {
            ruled_out.push(("--in".to_string(), typed.clone()));
        }
    }
    if let Some(seen) = guards.start_time {
        if found.iter().any(|p| !started_at(p, seen)) {
            ruled_out.push(("--start-time".to_string(), seen.to_string()));
        }
    }
    Ok(ruled_out)
}

/// Resolve an `--in` directory filter to the form processes report their cwd in.
///
/// Handles "." (current directory), relative paths, and absolute paths, then
/// resolves symlinks (see [`resolve_dir`]).
///
/// Every command that takes `--in` resolves it here, so they all refuse a
/// relative path when the current directory is gone (exit 4, `invalid_input`):
/// there is nothing to resolve it against. A directory that does not exist is
/// not refused here, because processes can still run in a deleted directory;
/// see [`require_in_dir`].
pub fn resolve_in_dir(in_dir: &Option<String>) -> Result<Option<PathBuf>> {
    let Some(typed) = in_dir else {
        return Ok(None);
    };
    Ok(Some(resolve_dir(PathBuf::from(typed))?))
}

/// Refuse an `--in` directory that does not exist when nothing was found in
/// it (exit 4, `invalid_input`, `Directory not found: <path as typed>`): that
/// is a typo, not an empty directory. A deleted directory that processes still
/// run in is kept, so its leftovers can be found, as `proc in` does.
pub fn require_in_dir(
    in_dir: &Option<String>,
    dir: &Option<PathBuf>,
    found_any: bool,
) -> Result<()> {
    match (in_dir, dir) {
        (Some(typed), Some(dir)) if !found_any && !dir.is_dir() => {
            Err(ProcError::InvalidInput(if dir.exists() {
                format!("Not a directory: {typed}")
            } else {
                format!("Directory not found: {typed}")
            }))
        }
        _ => Ok(()),
    }
}

/// [`require_in_dir`] for a command that has not listed every process: only
/// when the resolved directory is missing does it look whether a process
/// still runs there
pub fn require_in_dir_live(in_dir: &Option<String>, dir: &Option<PathBuf>) -> Result<()> {
    if let Some(missing) = dir.as_ref().filter(|dir| !dir.is_dir()) {
        let any = Process::find_all()?
            .iter()
            .any(|p| matches_in_filter(p, missing));
        require_in_dir(in_dir, dir, any)?;
    }
    Ok(())
}

/// The current directory, for resolving the relative `path` the user typed.
/// When the directory proc runs in was deleted there is nothing to resolve
/// against; say that, rather than let the lookup fail as an unrelated error.
pub fn current_dir_for(path: &Path) -> Result<PathBuf> {
    std::env::current_dir().map_err(|err| {
        ProcError::InvalidInput(format!(
            "Cannot resolve '{}': the current directory no longer exists or can't be read ({err})",
            path.display()
        ))
    })
}

/// Make a directory path comparable with the cwds the OS reports.
///
/// A relative path is joined onto the current directory. If the directory
/// exists it is canonicalized: the OS reports a process's cwd with symlinks
/// resolved (`/tmp/x` is `/private/tmp/x` on macOS) and, on Windows, with the
/// real case, long names and plain `D:\` form, so the path the user typed
/// (symlinked, `RUNNER~1`, forward slashes) must be put in that form before
/// the two are compared. A directory that does not exist (it may still be the
/// cwd of a process that outlived it) has the part that does exist put in that
/// form and the rest kept as typed, so a deleted `/tmp/x` still reads
/// `/private/tmp/x`. A relative path needs the current directory: when that is
/// gone it is an error.
pub fn resolve_dir(path: PathBuf) -> Result<PathBuf> {
    let absolute = if path.is_relative() {
        current_dir_for(&path)?.join(path)
    } else {
        path
    };
    let found = absolute
        .ancestors()
        .find_map(|part| part.canonicalize().ok().map(|real| (part, real)));
    Ok(found.map_or(absolute.clone(), |(part, real)| {
        let mut real = as_reported(real);
        real.extend(
            absolute
                .strip_prefix(part)
                .unwrap_or(Path::new(""))
                .components(),
        );
        real
    }))
}

/// A canonical path in the form processes report: Windows returns
/// `\\?\D:\...`, but processes report `D:\...`
#[cfg(windows)]
fn as_reported(canonical: PathBuf) -> PathBuf {
    PathBuf::from(strip_verbatim_prefix(&canonical.to_string_lossy()))
}

#[cfg(not(windows))]
fn as_reported(canonical: PathBuf) -> PathBuf {
    canonical
}

/// Turn a Windows verbatim path into the plain form processes report.
///
/// `canonicalize` on Windows returns `\\?\D:\app\x.exe` (or
/// `\\?\UNC\server\share\x.exe`), while a process's executable path is
/// `D:\app\x.exe` (`\\server\share\x.exe`), so the two never compare equal.
/// Any other form comes back unchanged. Pure string work, so it is tested on
/// every OS; only Windows calls it.
pub fn strip_verbatim_prefix(path: &str) -> String {
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{unc}");
    }
    if let Some(disk) = path.strip_prefix(r"\\?\") {
        let mut chars = disk.chars();
        let is_disk = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.next() == Some(':')
            && matches!(chars.next(), Some('\\') | None);
        if is_disk {
            return disk.to_string();
        }
    }
    path.to_string()
}

#[cfg(test)]
mod verbatim_tests {
    use super::strip_verbatim_prefix;

    #[test]
    fn a_verbatim_disk_path_loses_its_prefix() {
        assert_eq!(
            strip_verbatim_prefix(r"\\?\D:\a\proc-cli\listener.exe"),
            r"D:\a\proc-cli\listener.exe"
        );
    }

    #[test]
    fn a_verbatim_unc_path_becomes_a_plain_unc_path() {
        assert_eq!(
            strip_verbatim_prefix(r"\\?\UNC\server\share\listener.exe"),
            r"\\server\share\listener.exe"
        );
    }

    #[test]
    fn other_forms_are_unchanged() {
        for path in [
            r"D:\a\proc-cli\listener.exe",
            r"\\server\share\listener.exe",
            r"\\?\Volume{0123}\listener.exe",
            "/usr/bin/node",
            "",
        ] {
            assert_eq!(strip_verbatim_prefix(path), path);
        }
    }
}

#[cfg(test)]
mod resolve_tests {
    use super::{resolve_dir, resolve_in_dir};
    use std::path::PathBuf;

    #[test]
    fn no_filter_resolves_to_nothing() {
        assert_eq!(resolve_in_dir(&None).unwrap(), None);
    }

    #[test]
    fn a_dot_is_the_current_directory() {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let resolved = resolve_in_dir(&Some(".".to_string())).unwrap().unwrap();
        assert_eq!(resolved.canonicalize().unwrap(), cwd);
        assert!(resolved.is_absolute());
    }

    #[test]
    fn a_relative_path_is_taken_from_the_current_directory() {
        // cargo runs unit tests with the package root as the current directory
        let expected = std::env::current_dir()
            .unwrap()
            .join("src")
            .canonicalize()
            .unwrap();
        let resolved = resolve_in_dir(&Some("src".to_string())).unwrap().unwrap();
        assert_eq!(resolved.canonicalize().unwrap(), expected);
        assert!(resolved.is_absolute());
    }

    #[test]
    fn a_missing_relative_path_is_only_made_absolute() {
        let expected = std::env::current_dir().unwrap().join("no-such-dir-8f3a");
        assert_eq!(
            resolve_in_dir(&Some("no-such-dir-8f3a".to_string())).unwrap(),
            Some(expected)
        );
    }

    #[test]
    fn a_missing_absolute_path_keeps_its_missing_part_as_written() {
        // the part that exists in canonical form (macOS: /var is
        // /private/var), the rest as typed, so a deleted directory still
        // reads the way its processes report it
        let tmp = tempfile::tempdir().unwrap();
        let gone = tmp.path().join("gone").join("deeper");
        let expected = PathBuf::from(super::strip_verbatim_prefix(
            &tmp.path().canonicalize().unwrap().to_string_lossy(),
        ))
        .join("gone")
        .join("deeper");
        assert_eq!(resolve_dir(gone).unwrap(), expected);
    }

    #[test]
    fn an_existing_directory_comes_back_in_canonical_form() {
        let tmp = tempfile::tempdir().unwrap();
        let expected = PathBuf::from(super::strip_verbatim_prefix(
            &tmp.path().canonicalize().unwrap().to_string_lossy(),
        ));
        assert_eq!(resolve_dir(tmp.path().to_path_buf()).unwrap(), expected);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_resolved_to_its_target() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("real");
        std::fs::create_dir(&target).unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(resolve_dir(link).unwrap(), target.canonicalize().unwrap());
    }
}

#[cfg(test)]
mod match_tests {
    use super::{apply_filters, matches_by_filter, matches_in_filter};
    use crate::core::{NameMatch, Process, ProcessStatus};
    use std::path::Path;

    /// A process record with no working directory and no command line, which
    /// some processes really are (a zombie has neither on Linux)
    fn bare_process() -> Process {
        Process {
            pid: 1,
            name: "Worker".to_string(),
            exe_path: None,
            cwd: None,
            command: None,
            argv0: None,
            raw: None,
            cpu_percent: 0.0,
            memory_mb: 0.0,
            status: ProcessStatus::Sleeping,
            user: None,
            parent_pid: None,
            start_time: None,
            uptime_secs: None,
            parent_name: None,
            cpu_time_secs: None,
            open_files: None,
            child_count: None,
        }
    }

    #[test]
    fn a_process_with_no_working_directory_is_not_in_any_directory() {
        assert!(!matches_in_filter(&bare_process(), Path::new("/")));
    }

    /// macOS hides other users' start times: such a process can't be confirmed,
    /// so `--start-time` rules it out instead of letting it through
    #[test]
    fn a_start_time_guard_rules_out_a_process_whose_start_time_is_unknown() {
        let guards = super::Guards::new(&None, &None, Some(1_700_000_000));
        let mut processes = vec![bare_process()];
        let error = super::guard_targets(
            &mut processes,
            &["1".to_string()],
            &guards,
            NameMatch::Exact,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("(ruled out by --start-time 1700000000)"),
            "{error}"
        );

        let mut known = bare_process();
        known.start_time = Some(1_700_000_000);
        let mut processes = vec![known];
        super::guard_targets(
            &mut processes,
            &["1".to_string()],
            &guards,
            NameMatch::Exact,
        )
        .unwrap();
        assert_eq!(processes.len(), 1);
    }

    #[test]
    fn a_working_directory_matches_itself_and_what_is_under_it() {
        let mut process = bare_process();
        process.cwd = Some("/srv/app/api".to_string());
        assert!(matches_in_filter(&process, Path::new("/srv/app")));
        assert!(matches_in_filter(&process, Path::new("/srv/app/api")));
        assert!(!matches_in_filter(&process, Path::new("/srv/ap")));
        assert!(!matches_in_filter(&process, Path::new("/srv/app/web")));
    }

    const EXACT: NameMatch = NameMatch::Exact;
    const CMD: NameMatch = NameMatch::Command;

    #[test]
    fn by_matches_the_whole_name_ignoring_case_and_not_a_part_of_it() {
        let process = bare_process(); // named "Worker"
        assert!(matches_by_filter(&process, "worker", EXACT));
        assert!(matches_by_filter(&process, "WORKER", EXACT));
        assert!(!matches_by_filter(&process, "work", EXACT));
        assert!(!matches_by_filter(&process, "node", EXACT));
    }

    #[test]
    fn by_matches_the_executable_or_first_argument_file_name() {
        let mut process = bare_process();
        process.exe_path = Some("/opt/tools/serve".to_string());
        assert!(matches_by_filter(&process, "serve", EXACT));
        process.exe_path = None;
        process.argv0 = Some("./bin/runner".to_string());
        assert!(matches_by_filter(&process, "runner", EXACT));
        assert!(!matches_by_filter(&process, "bin", EXACT));
    }

    #[test]
    fn by_looks_in_the_command_line_only_with_cmd() {
        let mut process = bare_process();
        assert!(matches_by_filter(&process, "work", CMD));
        assert!(!matches_by_filter(&process, "node", CMD));
        process.command = Some("/usr/bin/Node server.js".to_string());
        assert!(matches_by_filter(&process, "node", CMD));
        assert!(!matches_by_filter(&process, "node", EXACT));
        assert!(!matches_by_filter(&process, "python", CMD));
    }

    #[test]
    fn apply_filters_keeps_what_by_matches_under_the_given_mode() {
        let mut running_node = bare_process();
        running_node.pid = 2;
        running_node.name = "helper".to_string();
        running_node.command = Some("helper --engine node".to_string());
        let mut named_node = bare_process();
        named_node.pid = 3;
        named_node.name = "node".to_string();
        let both = vec![running_node, named_node];
        let by = Some("node".to_string());

        let mut exact = both.clone();
        apply_filters(&mut exact, &None, &by, EXACT).unwrap();
        assert_eq!(exact.iter().map(|p| p.pid).collect::<Vec<_>>(), vec![3]);

        let mut cmd = both.clone();
        apply_filters(&mut cmd, &None, &by, CMD).unwrap();
        assert_eq!(cmd.iter().map(|p| p.pid).collect::<Vec<_>>(), vec![2, 3]);

        let mut unfiltered = both;
        apply_filters(&mut unfiltered, &None, &None, EXACT).unwrap();
        assert_eq!(unfiltered.len(), 2);
    }
}

#[cfg(test)]
mod cwd_within_tests {
    use super::cwd_within_with;
    use std::path::{Path, PathBuf};

    /// Maps the short name to the long one, as Windows canonicalize does
    /// (forward slashes, so the paths mean the same on every OS)
    fn fake(p: &Path) -> Option<PathBuf> {
        let s = p.to_string_lossy().replace("RUNNER~1", "runneradmin");
        s.starts_with("C:").then(|| PathBuf::from(s))
    }

    #[test]
    fn a_plain_match_needs_no_canonicalizing() {
        assert!(cwd_within_with(
            "/srv/app/api",
            Path::new("/srv/app"),
            false,
            |_| None
        ));
        assert!(!cwd_within_with(
            "/srv/other",
            Path::new("/srv/app"),
            false,
            |_| None
        ));
    }

    #[test]
    fn a_short_name_cwd_matches_the_long_name_dir_when_retrying() {
        let cwd = "C:/Users/RUNNER~1/Temp/.tmpX/";
        let dir = Path::new("C:/Users/runneradmin/Temp/.tmpX");
        assert!(cwd_within_with(cwd, dir, true, fake));
        assert!(!cwd_within_with(cwd, dir, false, fake));
    }

    #[test]
    fn a_cwd_that_cannot_be_canonicalized_does_not_match() {
        let dir = Path::new("C:/Users/runneradmin/Temp");
        assert!(!cwd_within_with("D:/gone", dir, true, fake));
    }

    #[test]
    fn a_canonical_cwd_outside_the_dir_does_not_match() {
        let dir = Path::new("C:/Users/runneradmin/Temp");
        assert!(!cwd_within_with("C:/Users/RUNNER~1/Other", dir, true, fake));
    }
}

#[cfg(test)]
mod sort_tests {
    use super::{sort_ports, sort_processes, PortSortKey, SortKey};
    use crate::core::port::{PortInfo, Protocol};
    use crate::core::{Process, ProcessStatus};

    fn process(pid: u32, name: &str, cpu: f32, mem: f64) -> Process {
        Process {
            pid,
            name: name.to_string(),
            exe_path: None,
            cwd: None,
            command: None,
            argv0: None,
            raw: None,
            cpu_percent: cpu,
            memory_mb: mem,
            status: ProcessStatus::Sleeping,
            user: None,
            parent_pid: None,
            start_time: None,
            uptime_secs: None,
            parent_name: None,
            cpu_time_secs: None,
            open_files: None,
            child_count: None,
        }
    }

    fn pids(processes: &[Process]) -> Vec<u32> {
        processes.iter().map(|p| p.pid).collect()
    }

    /// The same records in every order the tie-break has to undo
    fn shuffles(processes: &[Process]) -> Vec<Vec<Process>> {
        let mut forward = processes.to_vec();
        let mut backward = processes.to_vec();
        backward.reverse();
        forward.rotate_left(1);
        vec![forward, backward]
    }

    #[test]
    fn processes_that_tie_on_cpu_come_by_pid_whatever_order_they_arrive_in() {
        let all = [
            process(30, "c", 0.0, 1.0),
            process(10, "a", 0.0, 2.0),
            process(20, "b", 5.0, 3.0),
            process(40, "d", 0.0, 4.0),
        ];
        for mut list in shuffles(&all) {
            sort_processes(&mut list, SortKey::Cpu);
            // biggest first, then the zero-CPU ties by PID
            assert_eq!(pids(&list), [20, 10, 30, 40]);
        }
    }

    #[test]
    fn processes_that_tie_on_memory_come_by_pid() {
        let all = [
            process(7, "a", 0.0, 8.0),
            process(3, "b", 0.0, 8.0),
            process(9, "c", 0.0, 64.0),
            process(5, "d", 0.0, 8.0),
        ];
        for mut list in shuffles(&all) {
            sort_processes(&mut list, SortKey::Mem);
            assert_eq!(pids(&list), [9, 3, 5, 7]);
        }
    }

    #[test]
    fn processes_that_share_a_name_come_by_pid_and_names_ignore_case() {
        let all = [
            process(8, "zsh", 0.0, 0.0),
            process(2, "ZSH", 0.0, 0.0),
            process(6, "bash", 0.0, 0.0),
            process(4, "zsh", 0.0, 0.0),
        ];
        for mut list in shuffles(&all) {
            sort_processes(&mut list, SortKey::Name);
            assert_eq!(pids(&list), [6, 2, 4, 8]);
        }
    }

    #[test]
    fn sorting_by_pid_is_ascending() {
        let all = [
            process(8, "a", 9.0, 0.0),
            process(2, "b", 1.0, 0.0),
            process(6, "c", 5.0, 0.0),
        ];
        for mut list in shuffles(&all) {
            sort_processes(&mut list, SortKey::Pid);
            assert_eq!(pids(&list), [2, 6, 8]);
        }
    }

    #[test]
    fn a_limit_after_a_tied_sort_keeps_the_same_rows_for_every_input_order() {
        let all: Vec<Process> = [50, 10, 40, 20, 30]
            .iter()
            .map(|&pid| process(pid, "node", 0.0, 0.0))
            .collect();
        for mut list in shuffles(&all) {
            sort_processes(&mut list, SortKey::Cpu);
            list.truncate(2);
            assert_eq!(pids(&list), [10, 20]);
        }
    }

    fn port(port: u16, pid: u32, name: &str) -> PortInfo {
        PortInfo {
            port,
            protocol: Protocol::Tcp,
            pid,
            process_name: name.to_string(),
            address: None,
        }
    }

    fn pairs(ports: &[PortInfo]) -> Vec<(u16, u32)> {
        ports.iter().map(|p| (p.port, p.pid)).collect()
    }

    #[test]
    fn ports_come_by_port_then_pid() {
        let mut ports = vec![
            port(7000, 671, "ControlCenter"),
            port(80, 9, "nginx"),
            port(5000, 671, "ControlCenter"),
            port(80, 3, "nginx"),
        ];
        sort_ports(&mut ports, PortSortKey::Port);
        assert_eq!(pairs(&ports), [(80, 3), (80, 9), (5000, 671), (7000, 671)]);
    }

    #[test]
    fn ports_sorted_by_pid_have_each_processs_ports_ascending() {
        let mut ports = vec![
            port(7000, 671, "ControlCenter"),
            port(80, 9, "nginx"),
            port(5000, 671, "ControlCenter"),
        ];
        sort_ports(&mut ports, PortSortKey::Pid);
        assert_eq!(pairs(&ports), [(80, 9), (5000, 671), (7000, 671)]);
    }

    #[test]
    fn ports_sorted_by_name_break_ties_by_port_then_pid_and_ignore_case() {
        let mut ports = vec![
            port(7000, 671, "ControlCenter"),
            port(80, 9, "nginx"),
            port(5000, 671, "controlcenter"),
            port(80, 3, "Nginx"),
        ];
        sort_ports(&mut ports, PortSortKey::Name);
        assert_eq!(pairs(&ports), [(5000, 671), (7000, 671), (80, 3), (80, 9)]);
    }
}
