//! What `on`, `info`, `for` and `why` share to show a process: the ports it
//! listens on with the connections that came in on them, read once during the
//! CPU measurement, and the card they are drawn on (see `ui::card`).

use crate::core::connections::{connections_for, Connections, Peer};
use crate::core::{PortInfo, Process};
use crate::ui::card::{write_card, write_card_without_ports, CardOptions, CardPort};
use crate::ui::output::terminal_width;
use colored::*;
use serde::{Serialize, Serializer};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::panic::resume_unwind;

/// Whether this OS reads connections; the BSDs read none, and a card there
/// says nothing about them rather than that there are none
pub(crate) const READS_CONNECTIONS: bool =
    cfg!(not(any(target_os = "freebsd", target_os = "netbsd")));

/// A listening port of a process with the connections that came in on the
/// process's own sockets; `None` where the OS reads none or the reading failed
#[derive(Debug, Clone)]
pub(crate) struct HeldPort {
    pub(crate) port: PortInfo,
    pub(crate) connections: Option<Connections>,
}

/// The counts of `Connections`, as a port of `on <pid>` or `info` carries them:
/// no peers
#[derive(Serialize)]
struct Counts<'a> {
    total: usize,
    by_state: &'a BTreeMap<String, usize>,
}

/// A port as JSON: the port, then its connection counts
impl Serialize for HeldPort {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Json<'a> {
            #[serde(flatten)]
            port: &'a PortInfo,
            #[serde(skip_serializing_if = "Option::is_none")]
            connections: Option<Counts<'a>>,
        }
        Json {
            port: &self.port,
            connections: self.connections.as_ref().map(|found| Counts {
                total: found.total,
                by_state: &found.by_state,
            }),
        }
        .serialize(serializer)
    }
}

/// The card's options: the terminal as it is now
pub(crate) fn card_options(verbose: bool) -> CardOptions {
    CardOptions {
        verbose,
        width: terminal_width(),
        now_secs: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs()),
    }
}

/// What a process is: its name, then its card with the ports it listens on
/// (`None`: the port table could not be read, so the card says nothing of them)
pub(crate) fn write_process(
    out: &mut impl Write,
    process: &Process,
    ports: Option<&[HeldPort]>,
    opts: &CardOptions,
) -> std::io::Result<()> {
    writeln!(
        out,
        "{} {} (PID {})",
        "✓".green().bold(),
        process.name.white().bold(),
        process.pid.to_string().cyan().bold()
    )?;
    writeln!(out)?;
    write_process_card(out, process, ports, opts)
}

/// A process's card, then a blank line. The ports on the card are `held`; a
/// port number held on two addresses lists its connections once. `None`: the
/// ports are not known, and the card leaves out `Listening:` and `Connections:`.
pub(crate) fn write_process_card(
    out: &mut impl Write,
    process: &Process,
    held: Option<&[HeldPort]>,
    opts: &CardOptions,
) -> std::io::Result<()> {
    let Some(held) = held else {
        write_card_without_ports(out, process, opts)?;
        return writeln!(out);
    };
    let mut seen = HashSet::new();
    let ports: Vec<CardPort> = held
        .iter()
        .map(|entry| CardPort {
            port: &entry.port,
            connections: if seen.insert(entry.port.port) {
                entry.connections.as_ref()
            } else {
                None
            },
        })
        .collect();
    write_card(out, process, &ports, opts)?;
    writeln!(out)
}

/// The ports each of `pids` listens on, with their connections, read while
/// `work` runs: the listening table and the connections to the ports of all
/// the `pids` are read once, in the background, and the wait for the CPU
/// reading (the usual `work`) hides them. A process that listens on nothing
/// reads no connections. `peer_limit` caps the peers listed per port. The
/// `work` runs whatever the reading did; its error is the table's.
pub(crate) fn held_during<T>(
    pids: &[u32],
    peer_limit: usize,
    work: impl FnOnce() -> T,
) -> (crate::error::Result<Vec<Vec<HeldPort>>>, T) {
    held_during_with(PortInfo::get_all_listening, pids, peer_limit, work)
}

/// [`held_during`] with the listening table read by `table`
fn held_during_with<T>(
    table: impl FnOnce() -> crate::error::Result<Vec<PortInfo>> + Send,
    pids: &[u32],
    peer_limit: usize,
    work: impl FnOnce() -> T,
) -> (crate::error::Result<Vec<Vec<HeldPort>>>, T) {
    if pids.is_empty() {
        return (Ok(Vec::new()), work());
    }
    let (reading, result) = std::thread::scope(|scope| {
        let reading = scope.spawn(|| -> crate::error::Result<ConnectionReader> {
            let mut reader = ConnectionReader::new(table()?, peer_limit, true);
            let ports: Vec<u16> = pids.iter().flat_map(|pid| reader.ports_of(*pid)).collect();
            reader.read_now(&ports);
            Ok(reader)
        });
        let result = work();
        (reading.join().unwrap_or_else(|e| resume_unwind(e)), result)
    });
    let held = reading.map(|mut reader| pids.iter().map(|pid| reader.held_by(*pid)).collect());
    (held, result)
}

/// Takes the real CPU reading of `processes` and returns the ports each
/// listens on with their connections, all in one wait; an unreadable port
/// table is an error
pub(crate) fn measure_with_ports(
    processes: &mut [Process],
    peer_limit: usize,
) -> crate::error::Result<Vec<Vec<HeldPort>>> {
    let pids: Vec<u32> = processes.iter().map(|p| p.pid).collect();
    held_during(&pids, peer_limit, || Process::measure_cpu(processes)).0
}

/// [`measure_with_ports`] for a view that does not need the ports: each
/// process's ports are `None` when the port table could not be read
pub(crate) fn measure_with_known_ports(
    processes: &mut [Process],
    peer_limit: usize,
) -> Vec<Option<Vec<HeldPort>>> {
    let count = processes.len();
    known(measure_with_ports(processes, peer_limit), count)
}

/// The ports of `count` processes as a view shows them: each `Some` when the
/// port table was read, each `None` when it was not (so a view can leave the
/// ports out rather than say there are none)
pub(crate) fn known(
    held: crate::error::Result<Vec<Vec<HeldPort>>>,
    count: usize,
) -> Vec<Option<Vec<HeldPort>>> {
    match held {
        Ok(held) => held.into_iter().map(Some).collect(),
        Err(_) => vec![None; count],
    }
}

/// The connections to each of `jobs` (a port with all its holders), from one
/// reading of the socket table; `None` for each where the OS reads none or the
/// reading failed (the answer is the process, not them)
fn read_connections(
    jobs: &[(u16, Vec<u32>)],
    peer_limit: usize,
) -> Vec<(u16, Option<Connections>)> {
    let ports: Vec<(u16, &[u32])> = jobs
        .iter()
        .map(|(port, holders)| (*port, holders.as_slice()))
        .collect();
    let found = READS_CONNECTIONS
        .then(|| connections_for(&ports, peer_limit).ok())
        .flatten()
        .unwrap_or_default();
    jobs.iter()
        .map(|(port, _)| {
            let own = found.iter().find(|(read, _)| read == port);
            (*port, own.map(|(_, connections)| connections.clone()))
        })
        .collect()
}

/// The connections that came in on the sockets of `pid`, of the port-wide
/// `all` (which lists every peer)
pub(crate) fn held_by(all: &Connections, pid: u32) -> Connections {
    let peers: Vec<Peer> = all
        .peers
        .iter()
        .filter(|peer| peer.holder_pid == pid)
        .cloned()
        .collect();
    let mut by_state = BTreeMap::new();
    for peer in &peers {
        *by_state.entry(peer.state.clone()).or_default() += 1;
    }
    Connections {
        total: peers.len(),
        by_state,
        peers,
        peers_truncated: false,
    }
}

/// Reads the connections to the ports of one lookup, each port once, and gives
/// each holder its own share. `table` is every listening socket of the ports
/// asked about: its PIDs are the holders a port's connections are read with
/// (with a subset, a `0.0.0.0` holder would take another holder's connections).
pub(crate) struct ConnectionReader {
    table: Vec<PortInfo>,
    /// How many peers to list: none for the counts alone, all for a card
    limit: usize,
    /// Each holder gets its own share of a port's connections, which takes
    /// every peer when the port has several holders. Without it a port keeps
    /// the connections of all its holders, `limit` peers of them.
    split: bool,
    read: HashMap<u16, Option<Connections>>,
}

impl ConnectionReader {
    pub(crate) fn new(table: Vec<PortInfo>, limit: usize, split: bool) -> Self {
        ConnectionReader {
            table,
            limit,
            split,
            read: HashMap::new(),
        }
    }

    /// Every process holding `port`, once, PID ascending
    fn holders(&self, port: u16) -> Vec<u32> {
        let mut pids: Vec<u32> = self
            .table
            .iter()
            .filter(|entry| entry.port == port)
            .map(|entry| entry.pid)
            .collect();
        pids.sort_unstable();
        pids.dedup();
        pids
    }

    /// How `port` is read: with every holder, and as many peers as the share
    /// of each holder needs
    fn plan(&self, port: u16) -> (Vec<u32>, usize) {
        let holders = self.holders(port);
        let limit = if self.split && holders.len() > 1 {
            usize::MAX
        } else {
            self.limit
        };
        (holders, limit)
    }

    /// What reading `ports` takes: each port not read yet, with its holders,
    /// and one limit for the reading (a port that wanted fewer peers gets more)
    fn jobs(&self, ports: &[u16]) -> (Vec<(u16, Vec<u32>)>, usize) {
        let mut wanted: Vec<u16> = ports
            .iter()
            .copied()
            .filter(|port| !self.read.contains_key(port))
            .collect();
        wanted.sort_unstable();
        wanted.dedup();
        let plans: Vec<(u16, Vec<u32>, usize)> = wanted
            .into_iter()
            .map(|port| {
                let (holders, limit) = self.plan(port);
                (port, holders, limit)
            })
            .collect();
        let limit = plans.iter().map(|(_, _, limit)| *limit).max().unwrap_or(0);
        let jobs = plans
            .into_iter()
            .map(|(port, holders, _)| (port, holders))
            .collect();
        (jobs, limit)
    }

    /// Reads the connections to `ports` now, all in one reading of the socket
    /// table; nothing to read, nothing run
    pub(crate) fn read_now(&mut self, ports: &[u16]) {
        let (jobs, limit) = self.jobs(ports);
        if !jobs.is_empty() {
            self.read.extend(read_connections(&jobs, limit));
        }
    }

    /// Runs `work` while the connections to `ports` are read, all in one
    /// reading of the socket table, in the background: the wait for the CPU
    /// reading hides it
    pub(crate) fn read_during<T>(&mut self, ports: &[u16], work: impl FnOnce() -> T) -> T {
        let (jobs, limit) = self.jobs(ports);
        let (done, result) = std::thread::scope(|scope| {
            let reading = scope.spawn(|| read_connections(&jobs, limit));
            let result = work();
            let done = reading.join().unwrap_or_else(|e| resume_unwind(e));
            (done, result)
        });
        self.read.extend(done);
        result
    }

    /// The connections to `port` of every holder, read now if they were not
    pub(crate) fn of_port(&mut self, port: u16) -> Option<Connections> {
        if !self.read.contains_key(&port) {
            let (holders, limit) = self.plan(port);
            self.read
                .extend(read_connections(&[(port, holders)], limit));
        }
        self.read[&port].clone()
    }

    /// The connections that came in on `pid`'s sockets of `port`
    pub(crate) fn of_holder(&mut self, port: u16, pid: u32) -> Option<Connections> {
        let all = self.of_port(port)?;
        Some(if self.holders(port).len() > 1 {
            held_by(&all, pid)
        } else {
            all
        })
    }

    /// The ports `pid` listens on, each with its connections
    pub(crate) fn held_by(&mut self, pid: u32) -> Vec<HeldPort> {
        let ports: Vec<PortInfo> = self
            .table
            .iter()
            .filter(|entry| entry.pid == pid)
            .cloned()
            .collect();
        ports
            .into_iter()
            .map(|port| {
                let connections = self.of_holder(port.port, pid);
                HeldPort { port, connections }
            })
            .collect()
    }

    /// The port numbers `pid` listens on
    pub(crate) fn ports_of(&self, pid: u32) -> Vec<u16> {
        self.table
            .iter()
            .filter(|entry| entry.pid == pid)
            .map(|entry| entry.port)
            .collect()
    }
}

/// `n` established connections held by `holder`, for the tests of every view
#[cfg(test)]
pub(crate) fn connections_of(holder: u32, n: u16) -> Connections {
    let peers: Vec<Peer> = (0..n)
        .map(|i| Peer {
            holder_pid: holder,
            state: "established".to_string(),
            remote_address: "127.0.0.1".to_string(),
            remote_port: 50_000 + i,
            peer: None,
        })
        .collect();
    held_by(
        &Connections {
            total: peers.len(),
            by_state: BTreeMap::new(),
            peers,
            peers_truncated: false,
        },
        holder,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{ProcessStatus, Protocol};
    use crate::test_support::{assert_stops_at_a_failed_write, end, idle_child, record_of};

    fn without_cwd() -> Process {
        Process {
            pid: 1,
            name: "x".to_string(),
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

    /// A process with every detail the card can show
    fn detailed() -> Process {
        let mut p = without_cwd();
        p.cwd = Some("/work/app".to_string());
        p.exe_path = Some("/usr/bin/x".to_string());
        p.command = Some("x --serve".to_string());
        p.uptime_secs = Some(90);
        p
    }

    fn listener() -> PortInfo {
        PortInfo {
            port: 4000,
            protocol: Protocol::Tcp,
            pid: 1,
            process_name: "x".to_string(),
            address: None,
        }
    }

    fn bound(pid: u32, address: &str) -> PortInfo {
        let mut port = listener();
        port.pid = pid;
        port.address = Some(address.to_string());
        port
    }

    fn held(port: PortInfo, connections: Option<Connections>) -> HeldPort {
        HeldPort { port, connections }
    }

    /// The card as a terminal 100 columns wide draws it at a fixed time
    fn opts() -> CardOptions {
        CardOptions {
            verbose: false,
            width: 100,
            now_secs: 1_000,
        }
    }

    /// What a print function writes, with colours off
    fn printed(print: impl FnOnce(&mut Vec<u8>) -> std::io::Result<()>) -> String {
        colored::control::set_override(false);
        let mut out = Vec::new();
        print(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn a_process_lists_every_port_and_a_shared_port_number_once() {
        // 4000 on two addresses, then 4001
        let mut other = bound(1, "127.0.0.1");
        other.port = 4001;
        let ports = [
            held(bound(1, "127.0.0.1"), Some(connections_of(1, 1))),
            held(bound(1, "::1"), Some(connections_of(1, 1))),
            held(other, Some(Connections::default())),
        ];
        let out = printed(|w| write_process(w, &detailed(), Some(&ports), &opts()));
        assert!(out.starts_with("✓ x (PID 1)\n\n  Process:"), "{out}");
        assert_eq!(out.matches("Listening:").count(), 1, "{out}");
        assert!(out.contains(":4000 TCP on 127.0.0.1"), "{out}");
        assert!(out.contains(":4000 TCP on ::1"), "{out}");
        assert!(out.contains(":4001 TCP on 127.0.0.1"), "{out}");
        // the second address of 4000 repeats nothing; 4001 has none
        assert_eq!(out.matches("Connections:").count(), 1, "{out}");
        assert!(out.ends_with("\n\n"), "{out}");
    }

    #[test]
    fn a_process_listening_on_nothing_says_none() {
        let out = printed(|w| write_process(w, &without_cwd(), Some(&[]), &opts()));
        assert!(out.contains("Listening:   none\n"), "{out}");
        assert!(!out.contains("is listening on"), "{out}");
        assert!(!out.contains("Connections:"), "{out}");
    }

    #[test]
    fn a_failed_write_ends_the_process_view() {
        let p = detailed();
        let ports = [held(bound(1, "127.0.0.1"), Some(connections_of(1, 2)))];
        assert_stops_at_a_failed_write(|w| write_process(w, &p, Some(&ports), &opts()));
        assert_stops_at_a_failed_write(|w| write_process(w, &p, Some(&[]), &opts()));
        // ports not known
        assert_stops_at_a_failed_write(|w| write_process(w, &p, None, &opts()));
    }

    #[test]
    fn the_connections_of_a_holder_are_the_peers_that_came_in_on_its_sockets() {
        let mut all = connections_of(1, 3);
        all.peers.extend(connections_of(2, 2).peers);
        all.peers[4].state = "close_wait".to_string();
        all.total = 5;

        let first = held_by(&all, 1);
        assert_eq!(first.total, 3);
        assert_eq!(first.by_state.get("established"), Some(&3));
        assert_eq!(first.peers.len(), 3);
        let second = held_by(&all, 2);
        assert_eq!(second.total, 2);
        assert_eq!(second.by_state.get("established"), Some(&1));
        assert_eq!(second.by_state.get("close_wait"), Some(&1));
        assert!(!second.peers_truncated);
        assert_eq!(held_by(&all, 3), Connections::default());
    }

    #[test]
    fn a_reader_gives_each_holder_its_own_share_and_a_lone_holder_the_whole() {
        let table = vec![
            bound(1, "127.0.0.1"),
            bound(2, "::1"),
            bound(2, "127.0.0.1"),
        ];
        let mut reader = ConnectionReader::new(table, 0, true);
        assert_eq!(reader.holders(4000), vec![1, 2], "once each, ascending");
        assert!(reader.holders(4001).is_empty());
        // several holders: every peer, to tell their shares apart
        assert_eq!(reader.plan(4000), (vec![1, 2], usize::MAX));

        let mut all = connections_of(1, 2);
        all.peers.extend(connections_of(2, 1).peers);
        all.total = 3;
        reader.read.insert(4000, Some(all.clone()));
        assert_eq!(reader.of_holder(4000, 1).unwrap().total, 2);
        assert_eq!(reader.of_holder(4000, 2).unwrap().total, 1);
        assert_eq!(reader.of_port(4000), Some(all.clone()));
        let held = reader.held_by(2);
        assert_eq!(held.len(), 2, "both of its addresses");
        assert!(held
            .iter()
            .all(|h| h.connections.as_ref().unwrap().total == 1));
        assert_eq!(reader.ports_of(2), vec![4000, 4000]);

        // a lone holder keeps the port's connections as read, peers cut or not
        let mut lone = ConnectionReader::new(vec![bound(1, "127.0.0.1")], 50, false);
        assert_eq!(lone.plan(4000), (vec![1], 50));
        lone.read.insert(4000, Some(all.clone()));
        assert_eq!(lone.of_holder(4000, 1), Some(all));
        // a port whose connections the OS gave none of
        lone.read.insert(4001, None);
        assert_eq!(lone.of_holder(4001, 1), None);
    }

    #[test]
    fn a_reader_reads_a_port_it_was_not_told_to_prefetch() {
        // nothing listens on 4000 here: no connections, or none read at all
        let mut reader = ConnectionReader::new(vec![listener()], 0, true);
        assert_eq!(reader.of_port(4000).is_some(), READS_CONNECTIONS);
        // read once: the second call is the first one's answer
        assert!(reader.read.contains_key(&4000));
        assert_eq!(reader.of_port(4000).is_some(), READS_CONNECTIONS);
    }

    #[test]
    fn a_reader_reads_while_the_work_runs() {
        let table: Vec<PortInfo> = (0..6)
            .map(|i| {
                let mut port = listener();
                port.port = 4000 + i;
                port
            })
            .collect();
        let ports: Vec<u16> = table.iter().map(|p| p.port).collect();
        let mut reader = ConnectionReader::new(table, 0, true);
        // the work's answer comes back, and every port is read, in one reading
        let answer = reader.read_during(&ports, || 42);
        assert_eq!(answer, 42);
        assert_eq!(reader.read.len(), ports.len());
        // nothing left to read: the work still runs
        assert_eq!(reader.read_during(&ports, || "done"), "done");
        assert_eq!(reader.read_during(&[], || 1), 1);
    }

    #[test]
    fn a_reader_reads_now_what_it_has_not_read() {
        let mut reader = ConnectionReader::new(vec![listener()], 0, true);
        reader.read_now(&[4000, 4000]);
        assert_eq!(reader.read.len(), 1);
        // read already, or nothing asked: nothing more is read
        reader.read_now(&[4000]);
        reader.read_now(&[]);
        assert_eq!(reader.read.len(), 1);
    }

    #[test]
    fn a_ports_counts_are_json_without_the_peers() {
        let port = held(bound(1, "127.0.0.1"), Some(connections_of(1, 2)));
        let json = serde_json::to_value(&port).unwrap();
        assert_eq!(json["port"], 4000);
        assert_eq!(json["exposure"], "local");
        assert_eq!(json["connections"]["total"], 2);
        assert_eq!(json["connections"]["by_state"]["established"], 2);
        assert!(json["connections"].get("peers").is_none(), "{json}");

        // where the OS gives none the key is absent, never null
        let json = serde_json::to_value(held(bound(1, "127.0.0.1"), None)).unwrap();
        assert!(json.get("connections").is_none(), "{json}");
    }

    #[test]
    fn the_card_options_read_the_flag_the_terminal_and_the_clock() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let plain = card_options(false);
        assert!(!plain.verbose);
        assert_eq!(plain.width, terminal_width());
        assert!(plain.now_secs >= now && plain.now_secs <= now + 5);
        assert!(card_options(true).verbose);
    }

    #[test]
    fn no_process_has_no_ports_and_the_work_still_runs() {
        let (held, answer) = held_during(&[], usize::MAX, || 7);
        assert!(held.unwrap().is_empty());
        assert_eq!(answer, 7);
    }

    #[test]
    fn a_port_table_that_cannot_be_read_is_an_error_and_the_work_still_runs() {
        let (held, answer) = held_during_with(
            || Err(crate::error::ProcError::SystemError("no lsof".to_string())),
            &[1, 2],
            usize::MAX,
            || 7,
        );
        assert!(held.is_err());
        assert_eq!(answer, 7);
        // a view shows each process without ports, not with none
        let shown = known(held, 2);
        assert_eq!(shown.len(), 2);
        assert!(shown.iter().all(Option::is_none));

        // the same lookup with a table that reads
        let (held, _) = held_during_with(|| Ok(vec![listener()]), &[1, 2], 0, || ());
        let shown = known(held, 2);
        assert_eq!(shown[0].as_ref().unwrap().len(), 1);
        assert_eq!(shown[1].as_ref().unwrap().len(), 0);
    }

    #[test]
    fn a_card_without_known_ports_leaves_out_listening_and_connections() {
        let out = printed(|w| write_process(w, &detailed(), None, &opts()));
        assert!(out.starts_with("✓ x (PID 1)\n\n  Process:"), "{out}");
        assert!(out.contains("  Status:      sleeping"), "{out}");
        assert!(!out.contains("Listening:"), "{out}");
        assert!(!out.contains("Connections:"), "{out}");
        assert!(out.ends_with("\n\n"), "{out}");
    }

    #[test]
    fn ports_whose_connections_could_not_be_read_keep_their_lines() {
        let ports = [held(bound(1, "127.0.0.1"), None)];
        let out = printed(|w| write_process(w, &detailed(), Some(&ports), &opts()));
        assert!(out.contains("Listening:   :4000 TCP on 127.0.0.1"), "{out}");
        assert!(!out.contains("Connections:"), "{out}");
    }

    #[test]
    fn a_process_that_listens_on_nothing_holds_no_ports() {
        let mut child = idle_child();
        let mut own = vec![record_of(&child)];
        let held = measure_with_ports(&mut own, usize::MAX);
        end(&mut child);
        let held = held.unwrap();
        assert_eq!(held.len(), 1);
        assert!(held[0].is_empty());
        assert_eq!(measure_with_ports(&mut [], 0).unwrap().len(), 0);
    }

    #[test]
    fn a_process_holds_the_port_it_listens_on_with_its_connections() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (_accepted, _) = listener.accept().unwrap();

        let mut own = vec![Process::find_by_pid(std::process::id()).unwrap().unwrap()];
        let held = measure_with_ports(&mut own, usize::MAX).unwrap();
        let mine: Vec<&HeldPort> = held[0].iter().filter(|h| h.port.port == port).collect();
        assert_eq!(mine.len(), 1, "{held:?}");
        // the BSDs read none: the key is absent there, never an empty count
        assert_eq!(mine[0].connections.is_some(), READS_CONNECTIONS);
        if let Some(found) = mine[0].connections.as_ref() {
            assert!(found.total >= 1, "{found:?}");
        }
        drop(client);
    }
}
