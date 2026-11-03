//! The connections to a listening port: how many there are, in which TCP
//! states, and who is on the other end.
//!
//! Each OS answers with the table it already has for ports: `lsof` on macOS,
//! `/proc/net/tcp` and `tcp6` with the owner of each socket on Linux, and
//! `netstat -ano` on Windows. The FreeBSD and NetBSD `sockstat` prints no
//! connection state (NetBSD's man page lists no such column; FreeBSD's `-s` has
//! one, but no output of it is on record here), so on those two the connections
//! are left out rather than guessed at.
//!
//! A connection is the pair of its two ends, the server's (the port asked about)
//! and the client's. Each end is a socket row of its own, so a connection can
//! appear twice (once per end), or once (a client on another machine; on macOS
//! the server's end while the server has not accepted it, because `lsof` lists
//! only sockets some process has open), and is counted once. Its state is the
//! server's end when that is listed.

// the BSDs read no sockets, so nothing there parses a table
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows", test))]
use crate::core::port::PortInfo;
use crate::error::Result;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
#[cfg(target_os = "macos")]
use {crate::error::ProcError, std::process::Command};

/// The connections to one port. `total` and `by_state` count every connection;
/// `peers` lists up to the limit asked for, and `peers_truncated` says the list
/// was cut. A state with no connection is left out of `by_state`, and a port
/// with none gives `Connections::default()`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Connections {
    /// Every connection to the port, of any state but listening
    pub total: usize,
    /// How many are in each state: `established`, `close_wait`, `time_wait`,
    /// `fin_wait1`, `fin_wait2`, `syn_recv`, `last_ack`, `closing`
    pub by_state: BTreeMap<String, usize>,
    /// The connections, by holder, then by the client's port; at most the limit
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub peers: Vec<Peer>,
    /// There are more connections than `peers` lists
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub peers_truncated: bool,
}

/// One connection to the port
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Peer {
    /// The process that holds the listening socket this connection came in on
    pub holder_pid: u32,
    /// The connection's TCP state, as in `Connections::by_state`
    pub state: String,
    /// The client's address, without brackets for IPv6
    pub remote_address: String,
    /// The client's port
    pub remote_port: u16,
    /// The process at the client's end, when that end is a socket on this
    /// machine whose owner is visible
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer: Option<PeerProcess>,
}

/// The process at the client's end of a connection
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PeerProcess {
    /// Its PID
    pub pid: u32,
    /// Its name, as everywhere else in proc
    pub name: String,
}

/// An address and a port, the address as `normal_address` writes it
type Endpoint = (String, u16);

/// One row of a socket table, whatever the OS
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Sock {
    /// The processes that have the socket open (none: unknown, or the kernel
    /// keeps none, as for TIME_WAIT)
    pub pids: Vec<u32>,
    /// The name the table gives the process (lsof only), for a process that
    /// has gone by the time its name is looked up
    pub name: Option<String>,
    /// `state_name` of the socket's state
    pub state: String,
    pub local: Endpoint,
    pub remote: Endpoint,
}

impl Sock {
    #[cfg_attr(any(target_os = "freebsd", target_os = "netbsd"), allow(dead_code))]
    fn new(pids: Vec<u32>, state: &str, local: Endpoint, remote: Endpoint) -> Sock {
        Sock {
            pids,
            name: None,
            state: state_name(state),
            local: (normal_address(&local.0), local.1),
            remote: (normal_address(&remote.0), remote.1),
        }
    }
}

/// Count and list the connections to `port`, which `holders` (PIDs) listen on.
///
/// Only connections that came in on a socket one of `holders` has open are
/// counted, each once, under the holder whose socket it is: the holder with the
/// accepted socket open, or else the holder whose listening address the
/// connection came in on (a connection nobody has accepted yet, or a TIME_WAIT
/// one, has no owner). No holders, no connections. `peer_limit` caps the list
/// in `peers`, not the counts.
///
/// One system call per reading: macOS runs `lsof -nP -iTCP:<port>`, Linux reads
/// `/proc/net/tcp*` and the owner of each socket of the port, Windows runs
/// `netstat -ano`. FreeBSD and NetBSD give no connections (see the module
/// documentation).
pub fn connections_to(port: u16, holders: &[u32], peer_limit: usize) -> Result<Connections> {
    if holders.is_empty() {
        return Ok(Connections::default());
    }
    let socks = read_socks(port)?;
    Ok(assemble(
        &socks,
        port,
        holders,
        peer_limit,
        crate::core::process::names_by_pid,
    ))
}

#[cfg(target_os = "macos")]
fn read_socks(port: u16) -> Result<Vec<Sock>> {
    // no state filter: the rows of both ends of a connection, and the listeners
    // that say whose a connection nobody has accepted is
    let output = crate::ui::interrupt::unblocked_in_child(&mut Command::new("lsof"))
        .args(["-nP", &format!("-iTCP:{port}")])
        .output()
        .map_err(|e| ProcError::SystemError(format!("Failed to run lsof: {}", e)))?;
    Ok(parse_lsof_connections(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

#[cfg(target_os = "linux")]
fn read_socks(port: u16) -> Result<Vec<Sock>> {
    linux_socks(
        std::path::Path::new("/proc/net"),
        std::path::Path::new("/proc"),
        port,
    )
}

#[cfg(target_os = "windows")]
fn read_socks(_port: u16) -> Result<Vec<Sock>> {
    let [v4, v6] = PortInfo::netstat_tables()?;
    Ok(parse_netstat_connections(&[&v4, &v6]))
}

#[cfg(any(target_os = "freebsd", target_os = "netbsd"))]
fn read_socks(_port: u16) -> Result<Vec<Sock>> {
    Ok(Vec::new())
}

/// The sockets of `port` in the kernel tables under `net` (/proc/net), with the
/// processes under `procs` (/proc) that have each open
#[cfg(target_os = "linux")]
fn linux_socks(net: &std::path::Path, procs: &std::path::Path, port: u16) -> Result<Vec<Sock>> {
    let mut rows = Vec::new();
    for (text, v6) in PortInfo::proc_net_tables(net)? {
        // both ends of a connection to the port, and its listeners
        rows.extend(
            parse_proc_net_connections(&text, v6)
                .into_iter()
                .filter(|(_, s)| s.local.1 == port || s.remote.1 == port),
        );
    }
    // a socket no process owns (TIME_WAIT) has inode 0, and no fd links to it
    let wanted: HashSet<u64> = rows.iter().map(|(inode, _)| *inode).collect();
    let owners = PortInfo::socket_owners(&wanted, procs);
    Ok(rows
        .into_iter()
        .map(|(inode, mut sock)| {
            if let Some(pids) = owners.get(&inode) {
                sock.pids = pids.clone();
            }
            sock
        })
        .collect())
}

/// One state, one name: lowercase, `fin_wait1`, `syn_recv`, whichever the OS
/// writes (`FIN_WAIT_1`, `SYN_RCVD`, `SYN_RECEIVED`, `LISTENING`)
#[cfg_attr(any(target_os = "freebsd", target_os = "netbsd"), allow(dead_code))]
fn state_name(raw: &str) -> String {
    match raw.to_ascii_uppercase().as_str() {
        "LISTEN" | "LISTENING" => "listen".to_string(),
        "SYN_RCVD" | "SYN_RECV" | "SYN_RECEIVED" | "NEW_SYN_RECV" => "syn_recv".to_string(),
        "FIN_WAIT_1" | "FIN_WAIT1" => "fin_wait1".to_string(),
        "FIN_WAIT_2" | "FIN_WAIT2" => "fin_wait2".to_string(),
        "CLOSE" | "CLOSED" => "closed".to_string(),
        other => other.to_ascii_lowercase(),
    }
}

/// An address as every table compares it: an IPv4 address that an IPv6 socket
/// shows (`::ffff:127.0.0.1`, a dual-stack listener's client) is the IPv4
/// address the client's own socket shows
#[cfg_attr(any(target_os = "freebsd", target_os = "netbsd"), allow(dead_code))]
fn normal_address(address: &str) -> String {
    match address.parse::<std::net::Ipv6Addr>() {
        Ok(v6) => v6
            .to_ipv4_mapped()
            .map_or_else(|| address.to_string(), |v4| v4.to_string()),
        Err(_) => address.to_string(),
    }
}

/// The sockets in `lsof -nP -iTCP:<port>`'s output. The state is the
/// parenthesis at the end of a row, the endpoints the name before it
/// (`127.0.0.1:61747->127.0.0.1:61739`, or just the local one of a listener); a
/// row without a state or an address is skipped
#[cfg(any(target_os = "macos", test))]
fn parse_lsof_connections(stdout: &str) -> Vec<Sock> {
    use crate::core::port::split_listen_address;
    stdout
        .lines()
        .skip(1)
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let n = parts.len();
            if n < 9 {
                return None;
            }
            let state = parts[n - 1].strip_prefix('(')?.strip_suffix(')')?;
            let pid: u32 = parts[1].parse().ok()?;
            let (local, remote) = match parts[n - 2].split_once("->") {
                Some((local, remote)) => {
                    (split_listen_address(local)?, split_listen_address(remote)?)
                }
                None => (
                    split_listen_address(parts[n - 2])?,
                    ("0.0.0.0".to_string(), 0),
                ),
            };
            let mut sock = Sock::new(vec![pid], state, local, remote);
            let name = PortInfo::unescape_lsof(parts[0]);
            sock.name = Some(crate::core::escape_control(&name).into_owned());
            Some(sock)
        })
        .collect()
}

/// The sockets of one /proc/net/tcp or tcp6 (`v6`), with their inodes, in
/// every state but not yet with an owner. A line that doesn't parse is skipped
#[cfg(any(target_os = "linux", test))]
fn parse_proc_net_connections(text: &str, v6: bool) -> Vec<(u64, Sock)> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let (local, remote) = (fields.get(1)?, fields.get(2)?);
            let state = match *fields.get(3)? {
                "01" => "ESTABLISHED",
                "02" => "SYN_SENT",
                "03" => "SYN_RECV",
                "04" => "FIN_WAIT1",
                "05" => "FIN_WAIT2",
                "06" => "TIME_WAIT",
                "07" => "CLOSE",
                "08" => "CLOSE_WAIT",
                "09" => "LAST_ACK",
                "0A" => "LISTEN",
                "0B" => "CLOSING",
                "0C" => "NEW_SYN_RECV",
                _ => return None,
            };
            let inode: u64 = fields.get(9)?.parse().ok()?;
            let local = PortInfo::parse_proc_net_endpoint(local, v6)?;
            let remote = PortInfo::parse_proc_net_endpoint(remote, v6)?;
            Some((inode, Sock::new(Vec::new(), state, local, remote)))
        })
        .collect()
}

/// The sockets in the outputs of `netstat -ano -p TCP` and `-p TCPv6`
/// (`TCP LOCAL FOREIGN STATE PID`). PID 0, which Windows gives a TIME_WAIT
/// socket, is no owner
#[cfg(any(target_os = "windows", test))]
fn parse_netstat_connections(outputs: &[&str]) -> Vec<Sock> {
    use crate::core::port::split_listen_address;
    outputs
        .iter()
        .flat_map(|o| o.lines())
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() != 5 || parts[0] != "TCP" {
                return None;
            }
            let local = split_listen_address(parts[1])?;
            let remote = split_listen_address(parts[2])?;
            let pid: u32 = parts[4].parse().ok()?;
            let pids = if pid == 0 { Vec::new() } else { vec![pid] };
            Some(Sock::new(pids, parts[3], local, remote))
        })
        .collect()
}

/// What is known of one connection while the rows are read
struct Found {
    state: String,
    /// The processes that have the server's end open
    server_pids: Vec<u32>,
}

/// Turn the socket rows into the connections to `port` that `holders` listen
/// for. `names` gives the name of each PID asked about, in one call (and not at
/// all when there is no peer to name).
fn assemble(
    socks: &[Sock],
    port: u16,
    holders: &[u32],
    peer_limit: usize,
    names: impl FnOnce(&[u32]) -> HashMap<u32, String>,
) -> Connections {
    if holders.is_empty() {
        return Connections::default();
    }
    let held: HashSet<u32> = holders.iter().copied().collect();
    let connected = || socks.iter().filter(|s| s.state != "listen");

    // the listening sockets of the port that the holders have open
    let listeners: Vec<(&str, u32)> = socks
        .iter()
        .filter(|s| s.state == "listen" && s.local.1 == port)
        .flat_map(|s| {
            s.pids
                .iter()
                .filter(|pid| held.contains(pid))
                .map(move |&pid| (s.local.0.as_str(), pid))
        })
        .collect();

    // each connection once, keyed by (server end, client end). The server's
    // rows first: they have its state and its owner
    let mut found: BTreeMap<(Endpoint, Endpoint), Found> = BTreeMap::new();
    for sock in connected().filter(|s| s.local.1 == port) {
        let entry = found
            .entry((sock.local.clone(), sock.remote.clone()))
            .or_insert_with(|| Found {
                state: sock.state.clone(),
                server_pids: Vec::new(),
            });
        entry.server_pids.extend(&sock.pids);
    }
    // then a client's row whose server end is not listed: a connection nobody
    // has accepted (lsof) or a TIME_WAIT one that went the other way. Only to a
    // server on this machine: a socket going out to port 8080 elsewhere is not
    // a connection to ours
    let on_this_machine = |address: &str| {
        address
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
            || listeners.iter().any(|(a, _)| *a == address)
    };
    for sock in connected().filter(|s| s.local.1 != port && s.remote.1 == port) {
        if on_this_machine(&sock.remote.0) {
            found
                .entry((sock.remote.clone(), sock.local.clone()))
                .or_insert_with(|| Found {
                    state: sock.state.clone(),
                    server_pids: Vec::new(),
                });
        }
    }

    // who has the socket at each end: the client's, for the peer
    let mut owners: HashMap<(&Endpoint, &Endpoint), Vec<u32>> = HashMap::new();
    for sock in connected().filter(|s| !s.pids.is_empty()) {
        owners
            .entry((&sock.local, &sock.remote))
            .or_default()
            .extend(&sock.pids);
    }
    let process_names: HashMap<u32, &str> = socks
        .iter()
        .filter(|s| s.pids.len() == 1)
        .filter_map(|s| Some((s.pids[0], s.name.as_deref()?)))
        .collect();

    // the listener a connection came in on: the address it was bound to, or
    // else a wildcard (lsof prints both families' wildcard the same way)
    let listener_of = |address: &str| {
        let pick = |wanted: &dyn Fn(&str) -> bool| {
            listeners
                .iter()
                .filter(|(a, _)| wanted(a))
                .map(|&(_, pid)| pid)
                .min()
        };
        pick(&|a| a == address).or_else(|| pick(&|a| a == "0.0.0.0" || a == "::"))
    };

    struct Row<'a> {
        holder: u32,
        client: &'a Endpoint,
        state: &'a str,
        peer: Option<u32>,
    }
    let mut rows: Vec<Row> = Vec::new();
    for ((server, client), conn) in &found {
        let holder = conn
            .server_pids
            .iter()
            .copied()
            .filter(|pid| held.contains(pid))
            .min()
            .or_else(|| listener_of(&server.0));
        let Some(holder) = holder else {
            continue;
        };
        let peer = owners
            .get(&(client, server))
            .and_then(|pids| pids.iter().copied().min());
        rows.push(Row {
            holder,
            client,
            state: &conn.state,
            peer,
        });
    }
    rows.sort_by(|a, b| {
        (a.holder, a.client.1, &a.client.0).cmp(&(b.holder, b.client.1, &b.client.0))
    });

    let mut by_state: BTreeMap<String, usize> = BTreeMap::new();
    for row in &rows {
        *by_state.entry(row.state.to_string()).or_default() += 1;
    }
    let listed = &rows[..rows.len().min(peer_limit)];
    let mut pids: Vec<u32> = listed.iter().filter_map(|r| r.peer).collect();
    pids.sort_unstable();
    pids.dedup();
    let live = if pids.is_empty() {
        HashMap::new()
    } else {
        names(&pids)
    };
    let peers = listed
        .iter()
        .map(|row| Peer {
            holder_pid: row.holder,
            state: row.state.to_string(),
            remote_address: row.client.0.clone(),
            remote_port: row.client.1,
            // the process list's name; the table's own for a process that is
            // gone; without either, nobody to name
            peer: row.peer.and_then(|pid| {
                let name = live
                    .get(&pid)
                    .map(String::as_str)
                    .or_else(|| process_names.get(&pid).copied())?;
                Some(PeerProcess {
                    pid,
                    name: name.to_string(),
                })
            }),
        })
        .collect();
    Connections {
        total: rows.len(),
        by_state,
        peers,
        peers_truncated: rows.len() > listed.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names `connections_to` would look up: `proc<pid>`
    fn names(pids: &[u32]) -> HashMap<u32, String> {
        pids.iter().map(|&p| (p, format!("proc{p}"))).collect()
    }

    /// (holder_pid, state, remote address, remote port, peer pid) of each peer
    fn rows(c: &Connections) -> Vec<(u32, &str, &str, u16, Option<u32>)> {
        c.peers
            .iter()
            .map(|p| {
                (
                    p.holder_pid,
                    p.state.as_str(),
                    p.remote_address.as_str(),
                    p.remote_port,
                    p.peer.as_ref().map(|x| x.pid),
                )
            })
            .collect()
    }

    fn states(c: &Connections) -> Vec<(&str, usize)> {
        c.by_state.iter().map(|(k, v)| (k.as_str(), *v)).collect()
    }

    // `lsof -nP -iTCP:61739` on macOS 15 (Darwin 24.6), run while two copies of
    // the listener example held 127.0.0.1:61739 and [::1]:61739 (pids 37214 and
    // 37220, which never accept) and a Python process (pid 37292) had opened two
    // connections to the first and one to the second. The kernel completed the
    // handshakes, but lsof lists only sockets with a file descriptor, so the
    // server's end of each connection is missing and only the client rows show.
    const LSOF_QUEUED: &str = "\
COMMAND     PID USER   FD   TYPE             DEVICE SIZE/OFF NODE NAME
listener  37214  zee    3u  IPv4 0x6e1c6547d6fec2d8      0t0  TCP 127.0.0.1:61739 (LISTEN)
listener  37220  zee    3u  IPv6 0xde316f641f668527      0t0  TCP [::1]:61739 (LISTEN)
python3.1 37292  zee    3u  IPv4 0x527a5cd7e0a81ccb      0t0  TCP 127.0.0.1:61747->127.0.0.1:61739 (ESTABLISHED)
python3.1 37292  zee    4u  IPv4 0xbd8743e0698e53d4      0t0  TCP 127.0.0.1:61748->127.0.0.1:61739 (ESTABLISHED)
python3.1 37292  zee    5u  IPv6 0x2e3852125c5dd844      0t0  TCP [::1]:61749->[::1]:61739 (ESTABLISHED)
";

    // The same command with a Python server (pid 41782) that accepted three
    // connections from a Python client (pid 41819), which then closed the last
    // one: the server's ends are listed too, the closed one CLOSE_WAIT, and the
    // client no longer has a socket for it.
    const LSOF_ACCEPTED: &str = "\
COMMAND     PID USER   FD   TYPE             DEVICE SIZE/OFF NODE NAME
python3.1 41782  zee    3u  IPv4 0x4513faa2acfa75c8      0t0  TCP 127.0.0.1:62065 (LISTEN)
python3.1 41782  zee    4u  IPv4 0x813284a0b5491f21      0t0  TCP 127.0.0.1:62065->127.0.0.1:62072 (ESTABLISHED)
python3.1 41782  zee    5u  IPv4 0x40d694b6fee217c5      0t0  TCP 127.0.0.1:62065->127.0.0.1:62073 (ESTABLISHED)
python3.1 41782  zee    6u  IPv4 0xaa6f4191fe8439d4      0t0  TCP 127.0.0.1:62065->127.0.0.1:62074 (CLOSE_WAIT)
python3.1 41819  zee    3u  IPv4 0xa432ac72781bc90f      0t0  TCP 127.0.0.1:62072->127.0.0.1:62065 (ESTABLISHED)
python3.1 41819  zee    4u  IPv4 0x2598174675ad3f6c      0t0  TCP 127.0.0.1:62073->127.0.0.1:62065 (ESTABLISHED)
";

    #[test]
    fn lsof_rows_of_clients_the_server_has_not_accepted_count_under_the_listener() {
        let socks = parse_lsof_connections(LSOF_QUEUED);
        // the two LISTEN rows and three connections
        assert_eq!(socks.len(), 5);
        let c = assemble(&socks, 61739, &[37214, 37220], 50, names);
        assert_eq!(c.total, 3);
        assert_eq!(states(&c), vec![("established", 3)]);
        assert!(!c.peers_truncated);
        assert_eq!(
            rows(&c),
            vec![
                // 127.0.0.1 is held by the first listener, [::1] by the second
                (37214, "established", "127.0.0.1", 61747, Some(37292)),
                (37214, "established", "127.0.0.1", 61748, Some(37292)),
                (37220, "established", "::1", 61749, Some(37292)),
            ]
        );
        assert_eq!(
            c.peers[0].peer.as_ref().map(|p| p.name.as_str()),
            Some("proc37292")
        );
    }

    #[test]
    fn a_connection_is_counted_once_under_its_own_holder() {
        let socks = parse_lsof_connections(LSOF_QUEUED);
        // asked about the IPv4 holder alone, the IPv6 connection is not its own
        let c = assemble(&socks, 61739, &[37214], 50, names);
        assert_eq!(c.total, 2);
        assert!(c.peers.iter().all(|p| p.holder_pid == 37214));
        let c = assemble(&socks, 61739, &[37220], 50, names);
        assert_eq!(c.total, 1);
        assert_eq!(c.peers[0].remote_address, "::1");
    }

    #[test]
    fn lsof_rows_of_both_ends_make_one_connection_with_the_servers_state() {
        let socks = parse_lsof_connections(LSOF_ACCEPTED);
        let c = assemble(&socks, 62065, &[41782], 50, names);
        // three connections, though two of them show a row at each end
        assert_eq!(c.total, 3);
        assert_eq!(states(&c), vec![("close_wait", 1), ("established", 2)]);
        assert_eq!(
            rows(&c),
            vec![
                (41782, "established", "127.0.0.1", 62072, Some(41819)),
                (41782, "established", "127.0.0.1", 62073, Some(41819)),
                // the client closed its end: nobody to name
                (41782, "close_wait", "127.0.0.1", 62074, None),
            ]
        );
    }

    #[test]
    fn lsof_keeps_the_name_it_prints_for_a_process_that_is_gone() {
        let socks = parse_lsof_connections(LSOF_ACCEPTED);
        // the process list no longer has the client: lsof's own name stands
        let c = assemble(&socks, 62065, &[41782], 50, |_| HashMap::new());
        assert_eq!(
            c.peers[0].peer.as_ref().map(|p| p.name.as_str()),
            Some("python3.1")
        );
        // and a server's end that never showed a name leaves the peer out
        let only_server: Vec<Sock> = socks
            .iter()
            .filter(|s| s.pids != [41819])
            .cloned()
            .collect();
        let c = assemble(&only_server, 62065, &[41782], 50, |_| HashMap::new());
        assert!(c.peers.iter().all(|p| p.peer.is_none()));
    }

    #[test]
    fn lsof_lines_that_are_not_sockets_with_a_state_are_skipped() {
        let out = "\
COMMAND     PID USER   FD   TYPE             DEVICE SIZE/OFF NODE NAME
node        100  zee   20u  IPv4 0xa                 0t0  TCP *:3000 (LISTEN)
node        100  zee   21u  IPv4 0xb                 0t0  TCP 127.0.0.1:3000->127.0.0.1:50000 (ESTABLISHED)
short line
node        abc  zee   22u  IPv4 0xc                 0t0  TCP 127.0.0.1:3000->127.0.0.1:50001 (ESTABLISHED)
node        100  zee   23u  IPv4 0xd                 0t0  TCP 127.0.0.1:3000->127.0.0.1:50002
node        100  zee   24u  IPv4 0xe                 0t0  TCP 127.0.0.1:3000->127.0.0.1:notaport (ESTABLISHED)
node        100  zee   25u  IPv4 0xf                 0t0  TCP noport (LISTEN)
";
        let socks = parse_lsof_connections(out);
        let seen: Vec<_> = socks
            .iter()
            .map(|s| (s.pids.clone(), s.state.as_str(), s.local.1, s.remote.1))
            .collect();
        assert_eq!(
            seen,
            vec![
                (vec![100], "listen", 3000, 0),
                (vec![100], "established", 3000, 50000),
            ]
        );
    }

    #[test]
    fn lsof_names_with_escapes_are_decoded_and_made_safe() {
        let out = "\
COMMAND     PID USER   FD   TYPE             DEVICE SIZE/OFF NODE NAME
Activity\\x20Monitor 7 zee 8u IPv4 0xd 0t0 TCP 127.0.0.1:50000->127.0.0.1:3000 (ESTABLISHED)
evil\\x1b[2J 8 zee 9u IPv4 0xe 0t0 TCP 127.0.0.1:50001->127.0.0.1:3000 (ESTABLISHED)
";
        let socks = parse_lsof_connections(out);
        assert_eq!(socks[0].name.as_deref(), Some("Activity Monitor"));
        // a control character never reaches the terminal
        assert!(!socks[1].name.as_deref().unwrap().contains('\u{1b}'));
    }

    // /proc/net/tcp and tcp6 of an nginx:1.27-alpine container (Linux, Docker
    // Desktop): nginx listening on port 80, three `nc` clients holding
    // connections to 127.0.0.1:80, two `wget` requests that finished (one over
    // IPv4, one over [::1]) and left a TIME_WAIT socket each. Trailing blanks
    // trimmed from the lines. The kernel keeps no owner for TIME_WAIT sockets
    // (inode 0).
    const PROC_NET_TCP: &str = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000:0050 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 6750 1 00000000ea18da6a 100 0 0 10 0
   1: 0100007F:0050 0100007F:AFA7 01 00000000:00000000 00:00000000 00000000   101        0 16714 1 000000001c7ddd5a 20 0 0 10 -1
   2: 0100007F:0050 0100007F:A455 01 00000000:00000000 00:00000000 00000000   101        0 16713 1 00000000e6f84e62 20 0 0 10 -1
   3: 0100007F:0050 0100007F:B18B 01 00000000:00000000 00:00000000 00000000   101        0 16712 1 000000004005a3ae 20 0 0 10 -1
   4: 0100007F:A455 0100007F:0050 01 00000000:00000000 00:00000000 00000000     0        0 7580 2 000000004e5f16ca 20 0 0 10 -1
   5: 0100007F:AFA7 0100007F:0050 01 00000000:00000000 00:00000000 00000000     0        0 15605 2 00000000700cc970 20 0 0 10 -1
   6: 0100007F:B18B 0100007F:0050 01 00000000:00000000 00:00000000 00000000     0        0 16711 2 00000000e9941d16 20 0 0 10 -1
   7: 0100007F:C342 0100007F:0050 06 00000000:00000000 03:0000170B 00000000     0        0 0 3 00000000c79a5013
";
    const PROC_NET_TCP6: &str = "\
  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000000000000:0050 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 6751 1 00000000d969e58e 100 0 0 10 0
   1: 00000000000000000000000001000000:0050 00000000000000000000000001000000:D178 06 00000000:00000000 03:0000170B 00000000     0        0 0 3 000000006dfa50b6
";

    /// The sockets of the two tables above, with owners for the inodes: the
    /// kernel tables don't name them; /proc/<pid>/fd does. nginx's master
    /// (pid 1) holds both listeners, its worker (31) the accepted sockets, and
    /// the three `nc` processes (88 to 90) the clients' ends.
    fn fixture_linux() -> Vec<Sock> {
        let owners: HashMap<u64, Vec<u32>> = HashMap::from([
            (6750, vec![1]),
            (6751, vec![1]),
            (16712, vec![31]),
            (16713, vec![31]),
            (16714, vec![31]),
            (7580, vec![88]),
            (15605, vec![89]),
            (16711, vec![90]),
        ]);
        [(PROC_NET_TCP, false), (PROC_NET_TCP6, true)]
            .into_iter()
            .flat_map(|(text, v6)| parse_proc_net_connections(text, v6))
            .map(|(inode, mut sock)| {
                if let Some(pids) = owners.get(&inode) {
                    sock.pids = pids.clone();
                }
                sock
            })
            .collect()
    }

    #[test]
    fn linux_tables_give_every_state_and_both_address_families() {
        let socks = fixture_linux();
        // 8 IPv4 rows and 2 IPv6 rows
        assert_eq!(socks.len(), 10);
        let listen = socks.iter().filter(|s| s.state == "listen").count();
        let time_wait = socks.iter().filter(|s| s.state == "time_wait").count();
        assert_eq!((listen, time_wait), (2, 2));
        let v6 = socks.iter().find(|s| s.local.0 == "::1").unwrap();
        assert_eq!(
            (v6.local.1, v6.remote.0.as_str(), v6.remote.1),
            (80, "::1", 0xD178)
        );
    }

    #[test]
    fn linux_connections_count_each_socket_once_with_time_wait() {
        let socks = fixture_linux();
        // nginx's master and worker both hold port 80
        let c = assemble(&socks, 80, &[1, 31], 50, names);
        assert_eq!(c.total, 5);
        assert_eq!(states(&c), vec![("established", 3), ("time_wait", 2)]);
        assert_eq!(
            rows(&c),
            vec![
                // no owner for a TIME_WAIT socket: the listener's address says
                // whose it is (both wildcards, so the lower pid), and nobody is
                // named
                (1, "time_wait", "127.0.0.1", 0xC342, None),
                (1, "time_wait", "::1", 0xD178, None),
                // the worker holds the accepted sockets; the clients hold theirs
                (31, "established", "127.0.0.1", 0xA455, Some(88)),
                (31, "established", "127.0.0.1", 0xAFA7, Some(89)),
                (31, "established", "127.0.0.1", 0xB18B, Some(90)),
            ]
        );
    }

    #[test]
    fn a_connection_whose_server_end_is_held_by_a_holder_is_that_holders() {
        let socks = fixture_linux();
        // the master alone: the accepted sockets are the worker's, but the
        // listener's address still says they came in on this port
        let c = assemble(&socks, 80, &[1], 50, names);
        assert_eq!(c.total, 5);
        assert!(c.peers.iter().all(|p| p.holder_pid == 1));
    }

    #[test]
    fn linux_ipv4_mapped_addresses_are_the_ipv4_address() {
        // a dual-stack listener ([::]:8080) accepts an IPv4 client: tcp6 shows
        // the server's end as ::ffff:127.0.0.1, tcp the client's as 127.0.0.1
        let tcp6 = "\
  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000000000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 10 1 0 100 0 0 10 0
   1: 0000000000000000FFFF00000100007F:1F90 0000000000000000FFFF00000100007F:E000 01 00000000:00000000 00:00000000 00000000     0        0 11 1 0 20 0 0 10 -1
";
        let tcp = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:E000 0100007F:1F90 01 00000000:00000000 00:00000000 00000000     0        0 12 1 0 20 0 0 10 -1
";
        let owners: HashMap<u64, Vec<u32>> =
            HashMap::from([(10, vec![5]), (11, vec![5]), (12, vec![6])]);
        let socks: Vec<Sock> = [(tcp6, true), (tcp, false)]
            .into_iter()
            .flat_map(|(text, v6)| parse_proc_net_connections(text, v6))
            .map(|(inode, mut s)| {
                s.pids = owners.get(&inode).cloned().unwrap_or_default();
                s
            })
            .collect();
        let c = assemble(&socks, 8080, &[5], 50, names);
        assert_eq!(c.total, 1, "one connection, not one per table: {c:?}");
        assert_eq!(
            rows(&c),
            vec![(5, "established", "127.0.0.1", 0xE000, Some(6))]
        );
    }

    #[test]
    fn linux_lines_that_are_not_sockets_are_skipped() {
        let text = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:0050 0100007F:AFA7 01 00000000:00000000 00:00000000 00000000   101        0 16714 1 0 20 0 0 10 -1
garbage
   1: ZZ:0050 0100007F:AFA7 01 00000000:00000000 00:00000000 00000000   101        0 1 1 0
   2: 0100007F:0050 0100007F:AFA7 FF 00000000:00000000 00:00000000 00000000   101        0 2 1 0
   3: 0100007F:0050 0100007F:AFA7 01 00000000:00000000 00:00000000 00000000   101        0 x 1 0
";
        let rows = parse_proc_net_connections(text, false);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, 16714);
        // an address of the wrong length for its table
        assert!(parse_proc_net_connections(text, true).is_empty());
    }

    // `netstat -ano -p TCP` and `-p TCPv6` in the layout Windows prints (the
    // columns and the LISTENING and ESTABLISHED rows are those of the netstat
    // fixtures in port.rs; the TIME_WAIT row, which Windows gives PID 0, and the
    // CLOSE_WAIT row follow the same layout). A process on 4242 serves 8080;
    // 5000 is the client. 10.0.0.7:50010 is 5000 connecting out to another
    // machine's port 8080.
    const NETSTAT_V4: &str = "\
Active Connections

  Proto  Local Address          Foreign Address        State           PID
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1032
  TCP    0.0.0.0:8080           0.0.0.0:0              LISTENING       4242
  TCP    127.0.0.1:8080         127.0.0.1:50000        ESTABLISHED     4242
  TCP    127.0.0.1:8080         127.0.0.1:50001        ESTABLISHED     4242
  TCP    127.0.0.1:8080         127.0.0.1:50002        CLOSE_WAIT      4242
  TCP    127.0.0.1:8080         127.0.0.1:50003        TIME_WAIT       0
  TCP    127.0.0.1:50000        127.0.0.1:8080         ESTABLISHED     5000
  TCP    127.0.0.1:50001        127.0.0.1:8080         ESTABLISHED     5000
  TCP    192.168.1.5:8080       192.168.1.9:61000      ESTABLISHED     4242
  TCP    10.0.0.7:50010         10.0.0.8:8080          ESTABLISHED     5000
  TCP    0.0.0.0:99 ESTABLISHED
  TCP    127.0.0.1:8080         127.0.0.1:50009        ESTABLISHED     notapid
";
    const NETSTAT_V6: &str = "\
Active Connections

  Proto  Local Address          Foreign Address        State           PID
  TCP    [::]:8080              [::]:0                 LISTENING       4242
  TCP    [::1]:8080             [::1]:50004            ESTABLISHED     4242
  TCP    [::1]:50004            [::1]:8080             ESTABLISHED     5000
";

    #[test]
    fn netstat_rows_give_every_state_and_pid_zero_is_nobody() {
        let socks = parse_netstat_connections(&[NETSTAT_V4, NETSTAT_V6]);
        // 10 IPv4 rows and 3 IPv6 rows; the two broken rows are skipped
        assert_eq!(socks.len(), 13);
        let time_wait = socks.iter().find(|s| s.state == "time_wait").unwrap();
        assert!(time_wait.pids.is_empty());
        assert_eq!(parse_netstat_connections(&["", ""]).len(), 0);
    }

    #[test]
    fn netstat_connections_to_one_port_leave_out_what_goes_to_other_machines() {
        let socks = parse_netstat_connections(&[NETSTAT_V4, NETSTAT_V6]);
        let c = assemble(&socks, 8080, &[4242], 50, names);
        assert_eq!(c.total, 6);
        assert_eq!(
            states(&c),
            vec![("close_wait", 1), ("established", 4), ("time_wait", 1)]
        );
        assert_eq!(
            rows(&c),
            vec![
                (4242, "established", "127.0.0.1", 50000, Some(5000)),
                (4242, "established", "127.0.0.1", 50001, Some(5000)),
                (4242, "close_wait", "127.0.0.1", 50002, None),
                (4242, "time_wait", "127.0.0.1", 50003, None),
                (4242, "established", "::1", 50004, Some(5000)),
                // a client on another machine: no process here to name
                (4242, "established", "192.168.1.9", 61000, None),
            ]
        );
    }

    #[test]
    fn the_peers_are_capped_and_the_cap_is_said() {
        let socks = parse_netstat_connections(&[NETSTAT_V4, NETSTAT_V6]);
        let c = assemble(&socks, 8080, &[4242], 2, names);
        // the count is of every connection, the list is cut
        assert_eq!(c.total, 6);
        assert_eq!(c.by_state.values().sum::<usize>(), 6);
        assert_eq!(c.peers.len(), 2);
        assert!(c.peers_truncated);
        let c = assemble(&socks, 8080, &[4242], 6, names);
        assert_eq!(c.peers.len(), 6);
        assert!(!c.peers_truncated);
        let c = assemble(&socks, 8080, &[4242], 0, names);
        assert!(c.peers.is_empty() && c.peers_truncated && c.total == 6);
    }

    #[test]
    fn a_port_nobody_holds_has_no_connections() {
        let socks = parse_netstat_connections(&[NETSTAT_V4, NETSTAT_V6]);
        // no holder given, and a holder that has no socket on the port
        for holders in [&[][..], &[999_999][..]] {
            let c = assemble(&socks, 8080, holders, 50, names);
            assert_eq!(c, Connections::default(), "{holders:?}");
        }
        // a port with no sockets at all
        let c = assemble(&socks, 9, &[4242], 50, names);
        assert_eq!(c, Connections::default());
        // and the public call never looks at the system without a holder
        assert_eq!(
            connections_to(8080, &[], 50).unwrap(),
            Connections::default()
        );
    }

    #[test]
    fn states_have_one_name_on_every_os() {
        for (raw, name) in [
            ("ESTABLISHED", "established"),
            ("CLOSE_WAIT", "close_wait"),
            ("TIME_WAIT", "time_wait"),
            ("FIN_WAIT_1", "fin_wait1"),
            ("FIN_WAIT_2", "fin_wait2"),
            ("FIN_WAIT1", "fin_wait1"),
            ("SYN_RCVD", "syn_recv"),
            ("SYN_RECEIVED", "syn_recv"),
            ("SYN_RECV", "syn_recv"),
            ("NEW_SYN_RECV", "syn_recv"),
            ("SYN_SENT", "syn_sent"),
            ("LAST_ACK", "last_ack"),
            ("CLOSING", "closing"),
            ("LISTEN", "listen"),
            ("LISTENING", "listen"),
            ("CLOSE", "closed"),
            ("CLOSED", "closed"),
            ("IDLE", "idle"),
        ] {
            assert_eq!(state_name(raw), name, "{raw}");
        }
    }

    #[test]
    fn json_leaves_out_what_is_empty() {
        let none = serde_json::to_value(Connections::default()).unwrap();
        assert_eq!(none, serde_json::json!({ "total": 0, "by_state": {} }));
        let socks = parse_lsof_connections(LSOF_ACCEPTED);
        let c = assemble(&socks, 62065, &[41782], 2, names);
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json["total"], 3);
        assert_eq!(json["by_state"]["established"], 2);
        assert_eq!(json["by_state"]["close_wait"], 1);
        assert_eq!(json["peers_truncated"], true);
        assert_eq!(
            json["peers"][0],
            serde_json::json!({
                "holder_pid": 41782, "state": "established",
                "remote_address": "127.0.0.1", "remote_port": 62072,
                "peer": { "pid": 41819, "name": "proc41819" }
            })
        );
        // a peer nobody owns has no `peer` key
        let c = assemble(&socks, 62065, &[41782], 50, names);
        let json = serde_json::to_value(&c).unwrap();
        assert!(json["peers"][2].get("peer").is_none(), "{json}");
        assert!(json.get("peers_truncated").is_none(), "{json}");
    }

    /// A fake /proc: the tables and one directory per process
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_reads_the_tables_and_finds_each_sockets_owner() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("net")).unwrap();
        std::fs::write(dir.path().join("net/tcp"), PROC_NET_TCP).unwrap();
        std::fs::write(dir.path().join("net/tcp6"), PROC_NET_TCP6).unwrap();
        for (pid, sockets) in [
            ("1", vec![6750, 6751]),
            ("31", vec![16712, 16713, 16714]),
            ("88", vec![7580]),
            ("89", vec![15605]),
            ("90", vec![16711]),
            // another port's socket: not asked for
            ("91", vec![99999]),
        ] {
            let fds = dir.path().join(pid).join("fd");
            std::fs::create_dir_all(&fds).unwrap();
            for (i, inode) in sockets.iter().enumerate() {
                std::os::unix::fs::symlink(format!("socket:[{inode}]"), fds.join(i.to_string()))
                    .unwrap();
            }
        }
        let socks = super::linux_socks(&dir.path().join("net"), dir.path(), 80).unwrap();
        let c = assemble(&socks, 80, &[1, 31], 50, names);
        assert_eq!(c.total, 5);
        assert_eq!(states(&c), vec![("established", 3), ("time_wait", 2)]);
        assert_eq!(c.peers[2].peer.as_ref().map(|p| p.pid), Some(88));
        // a port nothing has a socket on
        let none = super::linux_socks(&dir.path().join("net"), dir.path(), 81).unwrap();
        assert!(none.is_empty());
        // no tables: an error, as for the listening ports
        let missing = dir.path().join("nothing");
        assert!(super::linux_socks(&missing, dir.path(), 80).is_err());
    }
}
