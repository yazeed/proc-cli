//! Target resolution - Convert user input to processes
//!
//! Targets can be:
//! - `:port` - Process listening on this port
//! - `pid` - Process with this PID (numeric)
//! - `name` - Processes matching this name

use crate::core::port::{parse_port, PortInfo, UnseenPort};
use crate::core::{Ancestor, Ancestors, NameLookup, NameMatch, Process};
use crate::error::{command_line_hint, ProcError, Result};

/// Resolved target type
#[derive(Debug, Clone)]
pub enum TargetType {
    /// Target a process by the port it listens on (e.g., `:3000`)
    Port(u16),
    /// Target a process by its process ID (e.g., `1234`)
    Pid(u32),
    /// Target processes by name (e.g., `node`): the whole name, or with `--cmd`
    /// any part of the name or command line
    Name(String),
}

/// clap value parser for target arguments: a target that starts with ':' must be
/// a port from 1 to 65535. Without this, ":70000" fell through to a name search
/// and reported "not found" instead of a typo.
pub fn validate_target_arg(s: &str) -> std::result::Result<String, String> {
    let targets = parse_targets(s);
    // "" or "," names nothing; without this, `watch ""` watched every process
    if targets.is_empty() {
        return Err("a target can't be empty: give a PID, :port or name".to_string());
    }
    for target in targets {
        if let Some(port) = target.strip_prefix(':') {
            if !matches!(port.parse::<u16>(), Ok(p) if p > 0) {
                return Err(format!(
                    "'{target}' is not a port; ports go from :1 to :65535"
                ));
            }
        }
    }
    Ok(s.to_string())
}

/// Parse a target string and determine its type
pub fn parse_target(target: &str) -> TargetType {
    let target = target.trim();

    // Explicit port prefix
    if target.starts_with(':') {
        if let Ok(port) = parse_port(target) {
            return TargetType::Port(port);
        }
    }

    // Pure number - treat as PID
    if let Ok(pid) = target.parse::<u32>() {
        return TargetType::Pid(pid);
    }

    // Otherwise it's a name
    TargetType::Name(target.to_string())
}

/// Resolve a target to processes; `mode` says how a name is compared
pub fn resolve_target(target: &str, mode: NameMatch) -> Result<Vec<Process>> {
    match parse_target(target) {
        TargetType::Port(port) => resolve_port(port),
        TargetType::Pid(pid) => resolve_pid(pid),
        TargetType::Name(name) => Process::find_by_name(&name, mode),
    }
}

/// [`resolve_target`], with what `policy` does to a name's matches that proc
/// runs under (see [`Ancestors`]) and the ones it left out. A PID or port
/// target leaves nothing out: it names its process
pub fn lookup_target(target: &str, mode: NameMatch, policy: Ancestors) -> Result<NameLookup> {
    match parse_target(target) {
        TargetType::Name(name) => Process::lookup_name(&name, mode, policy),
        TargetType::Port(port) => resolve_port(port).map(NameLookup::of),
        TargetType::Pid(pid) => resolve_pid(pid).map(NameLookup::of),
    }
}

/// [`resolve_target`] for a port, for a command that has read the listening
/// table already: the holders of `port` come from `listening`, which is not read again
pub(crate) fn resolve_port_in(port: u16, listening: &[PortInfo]) -> Result<Vec<Process>> {
    processes_on_port(
        port,
        crate::core::port::visible_holders(
            port,
            crate::core::port::holders_in(listening.to_vec(), port),
            PortInfo::unseen,
        )?,
        Process::find_by_pid,
    )
}

/// Resolve a port to every process listening on it
fn resolve_port(port: u16) -> Result<Vec<Process>> {
    processes_on_port(port, PortInfo::holders_seen(port)?, Process::find_by_pid)
}

/// The processes behind `listeners`, what was found listening on `port`, in
/// that order; `find` looks a process up by PID. A holder can end between the
/// two lookups and is left out, unless that leaves nobody.
fn processes_on_port(
    port: u16,
    listeners: Vec<PortInfo>,
    mut find: impl FnMut(u32) -> Result<Option<Process>>,
) -> Result<Vec<Process>> {
    let Some(first) = listeners.first().map(|l| l.pid) else {
        return Err(ProcError::PortNotFound(port));
    };
    let mut found = Vec::new();
    for listener in &listeners {
        if let Some(proc) = find(listener.pid)? {
            found.push(proc);
        }
    }
    if found.is_empty() {
        Err(ProcError::ProcessGone(first))
    } else {
        Ok(found)
    }
}

/// Resolve PID to process
fn resolve_pid(pid: u32) -> Result<Vec<Process>> {
    match Process::find_by_pid(pid)? {
        Some(proc) => Ok(vec![proc]),
        None => Err(ProcError::ProcessNotFound(pid.to_string())),
    }
}

/// Find all ports a process is listening on
pub fn find_ports_for_pid(pid: u32) -> Result<Vec<PortInfo>> {
    let all_ports = PortInfo::get_all_listening()?;
    Ok(all_ports.into_iter().filter(|p| p.pid == pid).collect())
}

/// Each process with the ports it listens on, from one reading of the port
/// table. A reading runs programs on Windows (two `netstat`s, and a
/// `tasklist` for each socket), so reading it once per process made `proc
/// for` on a few processes run past a minute there.
pub fn with_ports(processes: Vec<Process>) -> Result<Vec<(Process, Vec<PortInfo>)>> {
    let mut listening = None;
    let mut results = Vec::with_capacity(processes.len());
    for proc in processes {
        let all = match &mut listening {
            Some(all) => all,
            unread => unread.insert(PortInfo::get_all_listening()?),
        };
        let ports = all.iter().filter(|p| p.pid == proc.pid).cloned().collect();
        results.push((proc, ports));
    }
    Ok(results)
}

/// Split comma-separated targets into individual target strings
///
/// Examples:
///   ":3000,:8080" -> [":3000", ":8080"]
///   "node,python" -> ["node", "python"]
///   ":3000, 1234, node" -> [":3000", "1234", "node"]
pub fn parse_targets(targets_str: &str) -> Vec<String> {
    targets_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Split every target argument the user typed into individual targets
///
/// A command takes its targets as separate arguments, and a value may still be
/// a comma-separated list, so each value goes through [`parse_targets`].
///
/// Examples:
///   ["123", ":3000,:8080", "node"] -> ["123", ":3000", ":8080", "node"]
pub fn flatten_targets(values: &[String]) -> Vec<String> {
    values.iter().flat_map(|v| parse_targets(v)).collect()
}

/// The commands whose positional arguments are targets that [`flatten_targets`]
/// splits on commas, with the clap id of that argument. `by`, `tree`, `in` and
/// `for` take one value that commas never split, so they are not here.
const COMMA_SPLIT_ARGS: &[(&str, &str)] = &[
    ("on", "target"),
    ("info", "targets"),
    ("list", "name"),
    ("kill", "target"),
    ("stop", "target"),
    ("watch", "target"),
    ("unstick", "target"),
    ("freeze", "target"),
    ("thaw", "target"),
    ("why", "target"),
    ("free", "target"),
    ("wait", "target"),
];

/// A word as a shell would need it typed: quoted when it holds anything but
/// plain characters, so the notice's example can be pasted as it stands.
fn shell_word(word: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || ":._-/@%+=".contains(c);
    if !word.is_empty() && word.chars().all(plain) {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// The deprecation notice for comma-separated targets, if the command line
/// used any, or `None`.
///
/// Only the target arguments count (see `COMMA_SPLIT_ARGS`): a comma in a
/// `--by` or `--in` value, or in the one value of `by`, `in` or `for`, is just
/// text. Nothing is returned under `--json`, whose output must not change.
/// The notice is one string however many commas there are, and its example is
/// the user's own targets, split and joined by spaces.
pub fn comma_notice(matches: &clap::ArgMatches) -> Option<String> {
    let (command, sub) = matches.subcommand()?;
    let (_, id) = COMMA_SPLIT_ARGS.iter().find(|(name, _)| *name == command)?;
    if sub.get_flag("json") {
        return None;
    }
    let typed: Vec<String> = sub.get_many::<String>(id)?.cloned().collect();
    if !typed.iter().any(|value| value.contains(',')) {
        return None;
    }
    let targets: Vec<String> = flatten_targets(&typed)
        .iter()
        .map(|target| shell_word(target))
        .collect();
    Some(format!(
        "comma-separated targets are deprecated and will be removed in a future major \
         release; pass them as separate arguments: proc {command} {}",
        targets.join(" ")
    ))
}

/// What resolving several targets came to
#[derive(Debug, Default)]
pub struct Resolved {
    /// The processes found, each once however many targets name it
    pub processes: Vec<Process>,
    /// The targets that matched nothing
    pub not_found: Vec<String>,
    /// The names that matched nothing but are in some command lines, each
    /// with how many: what `--cmd` would find
    pub command_line: Vec<(String, usize)>,
    /// The targets that are ports whose holder proc can't see (another
    /// user's process, or a system that hides other users' sockets), each with
    /// its port and why. They are in `not_found` too, since they resolved to no
    /// process, but nobody holding them would be untrue
    pub unseen: Vec<(String, UnseenPort)>,
    /// The processes proc runs under that a target's name matched and that
    /// were left out, each with its target. A target whose matches were all
    /// left out is in `not_found` too, and the warning names the processes
    pub left_out: Vec<(String, Ancestor)>,
    /// The processes proc runs under that a name matched and that were kept,
    /// marked `runs_proc` (commands that only read)
    pub included: Vec<Ancestor>,
}

impl Resolved {
    /// The warning for the targets that matched nothing, if there were any
    pub fn warning(&self) -> Option<String> {
        let missing: Vec<&str> = self
            .not_found
            .iter()
            .filter(|target| !self.unseen.iter().any(|(unseen, _)| unseen == *target))
            .filter(|target| !self.left_out.iter().any(|(left, _)| left == *target))
            .map(String::as_str)
            .collect();
        let mut lines = Vec::new();
        if !missing.is_empty() {
            let mut warning = format!("Not found: {}", missing.join(", "));
            for (name, count) in &self.command_line {
                warning.push_str("\n  ");
                warning.push_str(&command_line_hint(name, *count));
            }
            lines.push(warning);
        }
        for (_, port) in &self.unseen {
            lines.push(port.to_string());
        }
        lines.extend(left_out_warning(&self.left_out_ancestors()));
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    /// Whether nothing was found and every target simply matched nothing (no
    /// port proc can't see, nothing left out): then the error for it says all
    /// the warning would, and a command prints only the error
    pub fn only_not_found(&self) -> bool {
        self.processes.is_empty() && self.unseen.is_empty() && self.left_out.is_empty()
    }

    /// The line for the processes proc runs under that a name matched and that
    /// were kept, if there were any: `Includes zsh (PID 81340), which runs this proc`
    pub fn note(&self) -> Option<String> {
        (!self.included.is_empty()).then(|| format!("Includes {}", runs_this_proc(&self.included)))
    }

    /// The processes left out, each once, PID ascending, as the JSON lists them
    pub fn left_out_ancestors(&self) -> Vec<Ancestor> {
        let mut ancestors: Vec<Ancestor> = self.left_out.iter().map(|(_, a)| a.clone()).collect();
        ancestors.sort_by_key(|a| a.pid);
        ancestors.dedup_by_key(|a| a.pid);
        ancestors
    }
}

/// The warning for processes proc runs under that a name matched and that
/// were left out: `Left out: zsh (PID 81340), which runs this proc`
pub(crate) fn left_out_warning(left_out: &[Ancestor]) -> Option<String> {
    (!left_out.is_empty()).then(|| format!("Left out: {}", runs_this_proc(left_out)))
}

/// The note for the processes shown that proc runs under, for a list a name
/// chose: `Includes zsh (PID 81340), which runs this proc`
pub(crate) fn includes_note(processes: &[Process]) -> Option<String> {
    let included: Vec<Ancestor> = processes
        .iter()
        .filter(|p| p.runs_proc)
        .map(Ancestor::from)
        .collect();
    (!included.is_empty()).then(|| format!("Includes {}", runs_this_proc(&included)))
}

/// `zsh (PID 81340), which runs this proc`, or `…, which run this proc` for several
pub(crate) fn runs_this_proc(ancestors: &[Ancestor]) -> String {
    let runs = if ancestors.len() == 1 { "runs" } else { "run" };
    format!(
        "{}, which {runs} this proc",
        crate::core::process::ancestor_list(ancestors)
    )
}

/// The error for targets (as the user wrote them) that resolved to no
/// process: a port whose holder proc can't see when one of them was
/// (`unseen`, from [`Resolved`]), since "not found" would be untrue; else the
/// processes proc runs under that matched and were left out (`left_out`), when
/// there were any; otherwise not found, with every target listed and the
/// names among them that are in command lines (`command_line`)
/// ([`ProcError::NoneFound`])
pub fn nothing_resolved(
    targets: &[String],
    unseen: &[(String, UnseenPort)],
    left_out: &[(String, Ancestor)],
    command_line: &[(String, usize)],
) -> ProcError {
    if let Some(&(_, port)) = unseen.first() {
        return ProcError::PortUnseen(port);
    }
    if left_out.is_empty() {
        return ProcError::NoneFound {
            error: Box::new(ProcError::ProcessNotFound(targets.join(" "))),
            not_found: flatten_targets(targets),
            command_line: command_line.to_vec(),
        };
    }
    let resolved = Resolved {
        left_out: left_out.to_vec(),
        ..Resolved::default()
    };
    // the targets that matched them, each once, as typed: `kill zsh ghost`
    // says what zsh matched, not what "zsh ghost" did
    let mut targets: Vec<&str> = Vec::new();
    for (target, _) in left_out {
        if !targets.contains(&target.as_str()) {
            targets.push(target);
        }
    }
    ProcError::AncestorsOnly {
        name: targets.join(" "),
        ancestors: resolved.left_out_ancestors(),
    }
}

/// Resolve multiple targets, deduplicating by PID, for a command that only
/// reads: a name's match that proc runs under is kept and marked (see
/// [`Ancestors::Mark`])
pub fn resolve_targets(targets: &[String], mode: NameMatch) -> Resolved {
    resolve_targets_impl(targets, false, mode, Ancestors::Mark)
}

/// Resolve multiple targets for a command that acts on processes, or waits
/// on them: proc itself is never one, and a name's match that proc runs under
/// is left out (see [`Ancestors::LeaveOut`]) and listed in `left_out`, so
/// `proc kill zsh` can't take down the shell proc runs in
pub fn resolve_targets_excluding_self(targets: &[String], mode: NameMatch) -> Resolved {
    resolve_targets_impl(targets, true, mode, Ancestors::LeaveOut)
}

fn resolve_targets_impl(
    targets: &[String],
    exclude_self: bool,
    mode: NameMatch,
    policy: Ancestors,
) -> Resolved {
    use std::collections::HashSet;

    let mut resolved = Resolved::default();
    let mut seen_pids = HashSet::new();
    let self_pid = std::process::id();

    for target in targets {
        match lookup_target(target, mode, policy) {
            Ok(lookup) => {
                let by_name = matches!(parse_target(target), TargetType::Name(_));
                for proc in lookup.processes {
                    // Skip self if requested (for destructive commands)
                    if exclude_self && proc.pid == self_pid {
                        continue;
                    }
                    if seen_pids.insert(proc.pid) {
                        if by_name && proc.runs_proc {
                            resolved.included.push(Ancestor::from(&proc));
                        }
                        resolved.processes.push(proc);
                    }
                }
                for ancestor in lookup.left_out {
                    resolved.left_out.push((target.clone(), ancestor));
                }
            }
            Err(error) => {
                match error {
                    ProcError::PortUnseen(port) => resolved.unseen.push((target.clone(), port)),
                    ProcError::AncestorsOnly { ref ancestors, .. } => {
                        for ancestor in ancestors {
                            resolved.left_out.push((target.clone(), ancestor.clone()));
                        }
                    }
                    _ => {}
                }
                resolved.not_found.push(target.clone());
                resolved.command_line.extend(error.command_line_match());
            }
        }
    }

    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Targets as a command gets them, one per argument
    fn typed(targets: &[&str]) -> Vec<String> {
        targets.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn with_ports_gives_each_process_its_own_ports() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let me = Process::find_by_pid(std::process::id()).unwrap().unwrap();
        let mut child = crate::test_support::idle_child();
        let other = crate::test_support::record_of(&child);
        // the first process reads the table, the second one uses that reading
        let results = with_ports(vec![me, other]).unwrap();
        crate::test_support::end(&mut child);
        assert!(results[0].1.iter().any(|p| p.port == port), "{results:?}");
        assert!(results[1].1.is_empty(), "{results:?}");
    }

    #[test]
    fn find_ports_for_pid_gives_the_ports_a_process_listens_on() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let ports = find_ports_for_pid(std::process::id()).unwrap();
        assert!(ports.iter().any(|p| p.port == port), "{ports:?}");
        assert!(ports.iter().all(|p| p.pid == std::process::id()));
    }

    #[test]
    fn test_parse_targets_single() {
        assert_eq!(parse_targets(":3000"), vec![":3000"]);
        assert_eq!(parse_targets("node"), vec!["node"]);
        assert_eq!(parse_targets("1234"), vec!["1234"]);
    }

    #[test]
    fn test_parse_targets_multiple() {
        assert_eq!(parse_targets(":3000,:8080"), vec![":3000", ":8080"]);
        assert_eq!(parse_targets("node,python"), vec!["node", "python"]);
        assert_eq!(
            parse_targets(":3000,1234,node"),
            vec![":3000", "1234", "node"]
        );
    }

    #[test]
    fn test_parse_targets_with_whitespace() {
        assert_eq!(
            parse_targets(":3000, :8080, :9000"),
            vec![":3000", ":8080", ":9000"]
        );
        assert_eq!(parse_targets(" node , python "), vec!["node", "python"]);
    }

    #[test]
    fn separate_arguments_and_comma_lists_flatten_to_one_list_in_order() {
        let typed = |values: &[&str]| -> Vec<String> {
            flatten_targets(&values.iter().map(|v| v.to_string()).collect::<Vec<_>>())
        };
        assert_eq!(typed(&["123", "456"]), vec!["123", "456"]);
        assert_eq!(
            typed(&["123", ":3000,:8080", "node"]),
            vec!["123", ":3000", ":8080", "node"]
        );
        assert_eq!(typed(&["a,b", ",", " c "]), vec!["a", "b", "c"]);
        // a repeated target stays repeated: the PID dedupe happens when resolving
        assert_eq!(typed(&["1", "1,1"]), vec!["1", "1", "1"]);
        assert!(typed(&[]).is_empty());
    }

    #[test]
    fn the_commands_that_split_on_commas_are_the_ones_with_a_target_list() {
        use clap::CommandFactory;
        let cli = crate::cli::Cli::command();
        for sub in cli.get_subcommands() {
            let name = sub.get_name();
            let listed = COMMA_SPLIT_ARGS.iter().find(|(n, _)| *n == name);
            // a list of targets: a positional that takes any number of values
            let lists: Vec<&str> = sub
                .get_positionals()
                .filter(|a| a.get_num_args().is_some_and(|n| n.max_values() > 1))
                .map(|a| a.get_id().as_str())
                .collect();
            match listed {
                Some((_, id)) => assert_eq!(lists, vec![*id], "{name}"),
                None => assert!(lists.is_empty(), "{name} takes a list but isn't listed"),
            }
        }
    }

    #[test]
    fn shell_words_are_quoted_only_when_they_need_to_be() {
        assert_eq!(shell_word(":3000"), ":3000");
        assert_eq!(shell_word("node"), "node");
        assert_eq!(shell_word("my app"), "'my app'");
        assert_eq!(shell_word("it's"), r"'it'\''s'");
        assert_eq!(shell_word(""), "''");
    }

    #[test]
    fn test_parse_targets_empty_entries() {
        assert_eq!(parse_targets(":3000,,,:8080"), vec![":3000", ":8080"]);
        assert_eq!(parse_targets(",,node,,"), vec!["node"]);
    }

    #[test]
    fn test_parse_target_port() {
        assert!(matches!(parse_target(":3000"), TargetType::Port(3000)));
        assert!(matches!(parse_target(":8080"), TargetType::Port(8080)));
    }

    #[test]
    fn test_parse_target_pid() {
        assert!(matches!(parse_target("1234"), TargetType::Pid(1234)));
        assert!(matches!(parse_target("99999"), TargetType::Pid(99999)));
    }

    #[test]
    fn a_colon_target_that_is_not_a_port_is_a_name() {
        for target in [":abc", ":70000", ":"] {
            assert!(
                matches!(parse_target(target), TargetType::Name(n) if n == target),
                "{target}"
            );
        }
    }

    #[test]
    fn test_parse_target_name() {
        assert!(matches!(parse_target("node"), TargetType::Name(_)));
        assert!(matches!(parse_target("my-process"), TargetType::Name(_)));
    }

    #[test]
    fn only_a_colon_target_has_to_be_a_port() {
        // names and PIDs are left alone, alone or in a list with ports
        assert_eq!(validate_target_arg("node").unwrap(), "node");
        assert_eq!(validate_target_arg("1234").unwrap(), "1234");
        assert_eq!(validate_target_arg(":3000, node").unwrap(), ":3000, node");
        for bad in [":70000", ":0", ":abc", "node,:70000"] {
            let err = validate_target_arg(bad).unwrap_err();
            assert!(err.contains("is not a port"), "{bad}: {err}");
        }
    }

    fn listener(pid: u32) -> PortInfo {
        PortInfo {
            port: 4000,
            protocol: crate::core::Protocol::Tcp,
            pid,
            process_name: "x".to_string(),
            address: None,
        }
    }

    fn own_process() -> Process {
        Process::find_by_pid(std::process::id()).unwrap().unwrap()
    }

    #[test]
    fn a_port_resolves_to_the_process_listening_on_it() {
        let found = processes_on_port(4000, vec![listener(7)], |pid| {
            assert_eq!(pid, 7);
            Ok(Some(own_process()))
        })
        .unwrap();
        assert_eq!(found.len(), 1);

        let none = processes_on_port(4000, Vec::new(), |_| panic!("no listener, no lookup"));
        assert!(
            matches!(none, Err(ProcError::PortNotFound(4000))),
            "{none:?}"
        );

        // the process ended after the port was listed
        let gone = processes_on_port(4000, vec![listener(7)], |_| Ok(None));
        assert!(matches!(gone, Err(ProcError::ProcessGone(7))), "{gone:?}");

        let failed = processes_on_port(4000, vec![listener(7)], |_| {
            Err(ProcError::SystemError("no".to_string()))
        });
        assert!(
            matches!(failed, Err(ProcError::SystemError(_))),
            "{failed:?}"
        );
    }

    #[test]
    fn a_port_resolves_to_every_process_listening_on_it() {
        let mut asked = Vec::new();
        let found = processes_on_port(4000, vec![listener(7), listener(8), listener(9)], |pid| {
            asked.push(pid);
            // 8 ended after the port was listed; the others are there
            Ok((pid != 8).then(own_process))
        })
        .unwrap();
        assert_eq!(asked, vec![7, 8, 9]);
        assert_eq!(found.len(), 2, "the holder that ended is left out");

        // every holder gone: the first one is named
        let gone = processes_on_port(4000, vec![listener(7), listener(8)], |_| Ok(None));
        assert!(matches!(gone, Err(ProcError::ProcessGone(7))), "{gone:?}");
    }

    #[test]
    fn targets_resolve_once_each_and_never_to_ourselves_when_asked() {
        let me = std::process::id().to_string();
        let targets = vec![me.clone(), me.clone(), "99999999".to_string()];

        // listed twice, found once; the made-up PID is not found
        let resolved = resolve_targets(&targets, NameMatch::Exact);
        assert_eq!(resolved.processes.len(), 1);
        assert_eq!(resolved.processes[0].pid, std::process::id());
        assert_eq!(resolved.not_found, vec!["99999999".to_string()]);
        assert_eq!(
            resolved.warning().unwrap(),
            "Not found: 99999999",
            "a PID has no --cmd hint"
        );

        let resolved = resolve_targets_excluding_self(&targets, NameMatch::Exact);
        assert!(resolved.processes.is_empty());
        assert_eq!(resolved.not_found, vec!["99999999".to_string()]);
    }

    #[test]
    fn nothing_missing_means_no_warning() {
        let me = std::process::id().to_string();
        let resolved = resolve_targets(&[me], NameMatch::Exact);
        assert!(resolved.not_found.is_empty());
        assert_eq!(resolved.warning(), None);
    }

    fn zsh(pid: u32) -> crate::core::Ancestor {
        crate::core::Ancestor {
            pid,
            name: "zsh".to_string(),
        }
    }

    #[test]
    fn a_resolution_says_what_it_left_out() {
        // one target matched only the shell proc runs in: left out, not "not found"
        let one = Resolved {
            not_found: vec!["zsh".to_string()],
            left_out: vec![("zsh".to_string(), zsh(81340))],
            ..Resolved::default()
        };
        assert_eq!(
            one.warning().unwrap(),
            "Left out: zsh (PID 81340), which runs this proc"
        );
        // two, beside a target that matched nothing; each said once, PID order
        let two = Resolved {
            not_found: vec!["ghost".to_string(), "zsh".to_string()],
            left_out: vec![
                ("zsh".to_string(), zsh(81340)),
                ("zsh".to_string(), zsh(11849)),
                (":1".to_string(), zsh(11849)),
            ],
            ..Resolved::default()
        };
        assert_eq!(
            two.warning().unwrap(),
            "Not found: ghost\nLeft out: zsh (PID 11849), zsh (PID 81340), which run this proc"
        );
        assert_eq!(two.left_out_ancestors(), vec![zsh(11849), zsh(81340)]);
        assert!(Resolved::default().left_out_ancestors().is_empty());
    }

    #[test]
    fn a_resolution_notes_the_ancestors_it_kept() {
        assert_eq!(Resolved::default().note(), None);
        let one = Resolved {
            included: vec![zsh(81340)],
            ..Resolved::default()
        };
        assert_eq!(
            one.note().unwrap(),
            "Includes zsh (PID 81340), which runs this proc"
        );
        let two = Resolved {
            included: vec![zsh(11849), zsh(81340)],
            ..Resolved::default()
        };
        assert_eq!(
            two.note().unwrap(),
            "Includes zsh (PID 11849), zsh (PID 81340), which run this proc"
        );
    }

    #[test]
    fn the_left_out_error_quotes_only_the_targets_that_matched_them() {
        // `kill zsh ghost`: ghost matched nothing, zsh only the shell
        let left_out = vec![("zsh".to_string(), zsh(81340))];
        assert!(matches!(
            nothing_resolved(&typed(&["zsh", "ghost"]), &[], &left_out, &[]),
            ProcError::AncestorsOnly { ref name, .. } if name == "zsh"
        ));
        // two targets that each matched only the shell
        let both = vec![
            ("zsh".to_string(), zsh(81340)),
            ("-zsh".to_string(), zsh(81340)),
        ];
        assert!(matches!(
            nothing_resolved(&typed(&["zsh", "-zsh"]), &[], &both, &[]),
            ProcError::AncestorsOnly { ref name, ref ancestors } if name == "zsh -zsh" && ancestors.len() == 1
        ));
        // one target that matched two shells proc runs under (a login zsh and
        // the zsh inside it) is quoted once
        let nested = vec![
            ("zsh".to_string(), zsh(11849)),
            ("zsh".to_string(), zsh(81340)),
        ];
        assert!(matches!(
            nothing_resolved(&typed(&["zsh"]), &[], &nested, &[]),
            ProcError::AncestorsOnly { ref name, ref ancestors } if name == "zsh" && ancestors.len() == 2
        ));
    }

    #[test]
    fn nothing_resolved_names_the_ancestors_when_all_were_left_out() {
        let left_out = vec![("zsh".to_string(), zsh(81340))];
        assert!(matches!(
            nothing_resolved(&typed(&["zsh"]), &[], &left_out, &[]),
            ProcError::AncestorsOnly { ref name, ref ancestors }
                if name == "zsh" && ancestors == &vec![zsh(81340)]
        ));
        // a port proc can't see still wins: saying nobody holds it would be untrue
        let unseen = vec![(
            ":80".to_string(),
            crate::core::UnseenPort {
                port: 80,
                why: crate::core::Unseen::OtherUser,
            },
        )];
        assert!(matches!(
            nothing_resolved(&typed(&[":80", "zsh"]), &unseen, &left_out, &[]),
            ProcError::PortUnseen(_)
        ));
    }

    #[test]
    fn resolve_target_finds_a_name_for_a_library_caller() {
        // no command resolves names through it any more (they use
        // `lookup_target`), but the library still offers it
        let me = Process::find_by_pid(std::process::id()).unwrap().unwrap();
        let found = resolve_target(&me.name, NameMatch::Exact);
        // proc itself is never a match, so only other processes of its name
        assert!(found.iter().flatten().all(|p| p.pid != me.pid), "{found:?}");
        assert!(matches!(
            resolve_target("no-such-process-q9z", NameMatch::Exact),
            Err(ProcError::ProcessNotFound(_))
        ));
    }

    #[test]
    fn resolving_by_name_for_a_read_notes_the_process_proc_runs_under() {
        // this test runs under its parent (cargo, or a shell): its exact name
        // finds it, kept and marked, and the resolution notes it
        let me = Process::find_by_pid(std::process::id()).unwrap().unwrap();
        let parent = Process::find_by_pid(me.parent_pid.unwrap())
            .unwrap()
            .unwrap();
        let resolved = resolve_targets(std::slice::from_ref(&parent.name), NameMatch::Exact);
        assert!(
            resolved.included.iter().any(|a| a.pid == parent.pid),
            "{:?}",
            resolved.included
        );
        assert!(resolved.note().is_some());
        // the same name for a command that acts: left out, named
        let acting =
            resolve_targets_excluding_self(std::slice::from_ref(&parent.name), NameMatch::Exact);
        assert!(acting.left_out.iter().any(|(_, a)| a.pid == parent.pid));
    }

    #[test]
    fn a_name_lookup_through_a_target_keeps_or_leaves_out_by_policy() {
        // proc itself is never a target: its own PID and name resolve to nothing
        // as a name, and a PID target keeps working as before
        let me = std::process::id().to_string();
        let by_pid = lookup_target(&me, NameMatch::Exact, crate::core::Ancestors::Mark).unwrap();
        assert_eq!(by_pid.processes.len(), 1);
        assert!(by_pid.left_out.is_empty());
        let resolved = resolve_targets(&[me], NameMatch::Exact);
        assert!(resolved.left_out.is_empty() && resolved.included.is_empty());
    }

    // A port another user's process holds resolved to nothing proc can act
    // on, but saying nobody holds it would be untrue
    #[test]
    fn a_port_held_by_a_process_proc_cannot_see_is_not_called_not_found() {
        use crate::core::Unseen;
        let other = UnseenPort {
            port: 80,
            why: Unseen::OtherUser,
        };
        let unseen = Resolved {
            not_found: vec![":80".to_string()],
            unseen: vec![(":80".to_string(), other)],
            ..Resolved::default()
        };
        assert_eq!(
            unseen.warning().unwrap(),
            "Port 80 is in use, but proc can't see which process holds it: it belongs to another user"
        );
        assert!(matches!(
            nothing_resolved(&typed(&[":80"]), &unseen.unseen, &[], &[]),
            ProcError::PortUnseen(port) if port == other
        ));

        // beside a target that matched nothing, each is said for what it is
        let both = Resolved {
            not_found: vec!["ghost".to_string(), ":80".to_string()],
            unseen: vec![(":80".to_string(), other)],
            ..Resolved::default()
        };
        assert_eq!(
            both.warning().unwrap(),
            "Not found: ghost\nPort 80 is in use, but proc can't see which process holds it: it belongs to another user"
        );

        // no port of another user's: not found, as before, with every target
        // listed for the JSON (a comma list split)
        let none = nothing_resolved(&typed(&["ghost", "1,:3000"]), &[], &[], &[]);
        assert_eq!(
            none.to_string(),
            "No process found matching 'ghost 1,:3000'\n  Try: proc list to list all processes"
        );
        assert_eq!(none.not_found(), Some(&typed(&["ghost", "1", ":3000"])[..]));
        assert_eq!(none.reason(), Some(crate::error::REASON_NOT_FOUND));
        assert_eq!(
            crate::error::ExitCode::from(&none),
            crate::error::ExitCode::NotFound
        );

        // a system that hides other users' sockets: proc can't tell
        let hidden = UnseenPort {
            port: 22,
            why: Unseen::Hidden("security.curtain=1"),
        };
        let resolved = Resolved {
            not_found: vec![":22".to_string()],
            unseen: vec![(":22".to_string(), hidden)],
            ..Resolved::default()
        };
        assert_eq!(
            resolved.warning().unwrap(),
            "proc can't tell whether port 22 is in use: this system hides other users' sockets (security.curtain=1)"
        );
        assert!(matches!(
            nothing_resolved(&typed(&[":22"]), &resolved.unseen, &[], &[]),
            ProcError::PortUnseen(port) if port == hidden
        ));
    }
}
