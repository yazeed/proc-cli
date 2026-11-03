//! Target resolution - Convert user input to processes
//!
//! Targets can be:
//! - `:port` - Process listening on this port
//! - `pid` - Process with this PID (numeric)
//! - `name` - Processes matching this name

use crate::core::port::{parse_port, PortInfo};
use crate::core::{NameMatch, Process};
use crate::error::{ProcError, Result};

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

/// Resolve a port to every process listening on it
fn resolve_port(port: u16) -> Result<Vec<Process>> {
    processes_on_port(port, PortInfo::holders_of(port)?, Process::find_by_pid)
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
    /// For a name that matched nothing, but is in some command lines: what
    /// `--cmd` would find
    pub hints: Vec<String>,
}

impl Resolved {
    /// The warning for the targets that matched nothing, if there were any
    pub fn warning(&self) -> Option<String> {
        if self.not_found.is_empty() {
            return None;
        }
        let mut warning = format!("Not found: {}", self.not_found.join(", "));
        for hint in &self.hints {
            warning.push_str("\n  ");
            warning.push_str(hint);
        }
        Some(warning)
    }
}

/// Resolve multiple targets, deduplicating by PID
pub fn resolve_targets(targets: &[String], mode: NameMatch) -> Resolved {
    resolve_targets_impl(targets, false, mode)
}

/// Resolve multiple targets, excluding the current process (self)
///
/// Use this for destructive commands (kill, stop) to avoid proc killing itself
/// when the target pattern matches proc's own command line arguments.
pub fn resolve_targets_excluding_self(targets: &[String], mode: NameMatch) -> Resolved {
    resolve_targets_impl(targets, true, mode)
}

fn resolve_targets_impl(targets: &[String], exclude_self: bool, mode: NameMatch) -> Resolved {
    use std::collections::HashSet;

    let mut resolved = Resolved::default();
    let mut seen_pids = HashSet::new();
    let self_pid = std::process::id();

    for target in targets {
        match resolve_target(target, mode) {
            Ok(processes) => {
                for proc in processes {
                    // Skip self if requested (for destructive commands)
                    if exclude_self && proc.pid == self_pid {
                        continue;
                    }
                    if seen_pids.insert(proc.pid) {
                        resolved.processes.push(proc);
                    }
                }
            }
            Err(error) => {
                resolved.not_found.push(target.clone());
                resolved.hints.extend(error.command_line_hint());
            }
        }
    }

    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
