//! Port discovery and management
//!
//! Provides cross-platform utilities for discovering which processes
//! are listening on network ports.

use crate::core::{escape_control, Process};
use crate::error::{ProcError, Result};
use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use std::path::Path;
// lsof on macOS, sockstat on FreeBSD and NetBSD, netstat and tasklist on
// Windows; Linux reads /proc instead
#[cfg(not(target_os = "linux"))]
use std::process::Command;

/// Network protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    /// Transmission Control Protocol - reliable, ordered delivery
    Tcp,
    /// User Datagram Protocol - fast, connectionless delivery
    Udp,
}

/// Information about a listening port
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortInfo {
    /// Port number
    pub port: u16,
    /// Protocol (TCP/UDP)
    pub protocol: Protocol,
    /// Process ID using this port
    pub pid: u32,
    /// Process name
    pub process_name: String,
    /// Bind address (e.g., "0.0.0.0", "127.0.0.1", "::")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
}

impl PortInfo {
    /// Get all listening ports on the system, port ascending and, for one port,
    /// PID ascending. The system lists them in its own order (lsof's on macOS,
    /// the kernel's socket table on Linux, netstat's on Windows), so the order
    /// is fixed here once for everything that reads ports: a process's ports are
    /// ascending in `on`, `for` and `why`, and the holders of a port are by PID.
    pub fn get_all_listening() -> Result<Vec<PortInfo>> {
        #[cfg(target_os = "macos")]
        let mut ports = Self::get_listening_macos()?;
        #[cfg(target_os = "linux")]
        let mut ports = Self::get_listening_linux()?;
        #[cfg(target_os = "windows")]
        let mut ports = Self::get_listening_windows()?;
        #[cfg(any(target_os = "freebsd", target_os = "netbsd"))]
        let mut ports = Self::get_listening_bsd()?;
        // One name per process, whatever the socket table calls it
        let pids: Vec<u32> = ports.iter().map(|p| p.pid).collect();
        name_as_processes(&mut ports, &crate::core::process::names_by_pid(&pids));
        ports.sort_by_key(|p| (p.port, p.pid));
        Ok(ports)
    }

    /// Every process listening on a port, each once, PID ascending (empty when
    /// nothing listens). One port number can be held by several processes:
    /// `127.0.0.1` and `[::1]` are different sockets.
    pub fn holders_of(port: u16) -> Result<Vec<PortInfo>> {
        Ok(holders_in(Self::get_all_listening()?, port))
    }

    /// Get the full process info for this port's process
    pub fn get_process(&self) -> Result<Option<Process>> {
        Process::find_by_pid(self.pid)
    }

    /// FreeBSD and NetBSD: `sockstat`, in the base system of both, lists the
    /// listening sockets with the process that holds each one. NetBSD's has no
    /// `-P` (protocol) option, so its UDP rows are dropped by the parser, and
    /// needs `-n` for numeric addresses
    #[cfg(any(target_os = "freebsd", target_os = "netbsd"))]
    fn get_listening_bsd() -> Result<Vec<PortInfo>> {
        #[cfg(target_os = "freebsd")]
        let args = ["-4", "-6", "-l", "-P", "tcp"];
        #[cfg(target_os = "netbsd")]
        let args = ["-4", "-6", "-l", "-n"];
        let output = crate::ui::interrupt::unblocked_in_child(&mut Command::new("sockstat"))
            .args(args)
            .output()
            .map_err(|e| ProcError::SystemError(format!("Failed to run sockstat: {}", e)))?;

        // sockstat cuts names short (FreeBSD keeps 10 characters);
        // `get_all_listening` names each holder as the process list does
        Ok(Self::parse_sockstat_output(
            &String::from_utf8_lossy(&output.stdout),
            cfg!(target_os = "netbsd"),
        ))
    }

    /// The listening TCP ports in sockstat's output, one entry per port and
    /// process. `dotted`: the address and port are joined with a dot
    /// (`127.0.0.1.8080`, NetBSD) rather than a colon (`127.0.0.1:8080`,
    /// FreeBSD). Compiled on other systems for the tests only
    #[cfg(any(target_os = "freebsd", target_os = "netbsd", test))]
    fn parse_sockstat_output(stdout: &str, dotted: bool) -> Vec<PortInfo> {
        let mut ports = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for port_info in stdout
            .lines()
            .filter_map(|line| Self::parse_sockstat_line(line, dotted))
        {
            // One entry per port and process, as on macOS: a socket a process
            // holds on both IPv4 and IPv6, or under several descriptors, is one
            if seen.insert((port_info.port, port_info.pid)) {
                ports.push(port_info);
            }
        }
        ports
    }

    /// One sockstat row: `USER COMMAND PID FD PROTO LOCAL FOREIGN`. Read from
    /// the right, so a command name with a space in it stays whole; the header
    /// and anything that isn't a listening TCP socket give `None`
    #[cfg(any(target_os = "freebsd", target_os = "netbsd", test))]
    fn parse_sockstat_line(line: &str, dotted: bool) -> Option<PortInfo> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let n = parts.len();
        if n < 7 {
            return None;
        }
        let (proto, local, foreign) = (parts[n - 3], parts[n - 2], parts[n - 1]);
        if !proto.starts_with("tcp") || !foreign.starts_with('*') {
            return None;
        }
        let pid: u32 = parts[n - 5].parse().ok()?;
        let process_name = escape_control(&parts[1..n - 5].join(" ")).into_owned();
        let (address, port) = if dotted {
            let (host, port) = local.rsplit_once('.')?;
            split_listen_address(&format!("{host}:{port}"))?
        } else {
            split_listen_address(local)?
        };
        Some(PortInfo {
            port,
            protocol: Protocol::Tcp,
            pid,
            process_name,
            address: Some(address),
        })
    }

    #[cfg(target_os = "macos")]
    fn get_listening_macos() -> Result<Vec<PortInfo>> {
        // Use lsof on macOS - only TCP LISTEN sockets. `+c 0` prints the whole
        // command name; by default lsof cuts it to 9 characters
        let output = crate::ui::interrupt::unblocked_in_child(&mut Command::new("lsof"))
            .args(["+c", "0", "-iTCP", "-sTCP:LISTEN", "-P", "-n"])
            .output()
            .map_err(|e| ProcError::SystemError(format!("Failed to run lsof: {}", e)))?;

        Ok(Self::parse_lsof_output(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }

    /// The listening ports in lsof's output, one entry per port and process
    /// (compiled on other systems for the tests only)
    #[cfg(any(target_os = "macos", test))]
    fn parse_lsof_output(stdout: &str) -> Vec<PortInfo> {
        let mut ports = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for line in stdout.lines().skip(1) {
            // Skip header
            if let Some(port_info) = Self::parse_lsof_line(line) {
                // Deduplicate (same port can appear multiple times for IPv4/IPv6)
                let key = (port_info.port, port_info.pid);
                if seen.insert(key) {
                    ports.push(port_info);
                }
            }
        }

        ports
    }

    /// lsof writes characters it can't print in a column, spaces included, as
    /// \xNN, so "Activity Monitor" arrives as "Activity\x20Monitor".
    ///
    /// Only printable ASCII is decoded. Control characters stay escaped, so a
    /// process name can't send escape sequences to the terminal, and the name is
    /// read as bytes, so a "\x" before a multi-byte character can't panic.
    #[cfg(any(target_os = "macos", test))]
    fn unescape_lsof(name: &str) -> String {
        fn hex(b: u8) -> Option<u8> {
            (b as char).to_digit(16).map(|d| d as u8)
        }
        let bytes = name.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if let [b'\\', b'x', hi, lo, ..] = bytes[i..] {
                if let (Some(hi), Some(lo)) = (hex(hi), hex(lo)) {
                    let decoded = hi << 4 | lo;
                    if (0x20..=0x7e).contains(&decoded) {
                        out.push(decoded);
                        i += 4;
                        continue;
                    }
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    #[cfg(any(target_os = "macos", test))]
    fn parse_lsof_line(line: &str) -> Option<PortInfo> {
        // lsof output format:
        // COMMAND  PID USER  FD  TYPE  DEVICE  SIZE/OFF  NODE  NAME
        // rapportd 643 zee   8u  IPv4  0x...   0t0       TCP   *:52633 (LISTEN)
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 9 {
            return None;
        }

        let process_name = escape_control(&Self::unescape_lsof(parts[0])).into_owned();
        let pid: u32 = parts[1].parse().ok()?;

        // Find the NAME column - it's after the NODE (TCP/UDP) column
        // The NAME looks like "*:3000" or "127.0.0.1:8080" or "*:52633 (LISTEN)"
        // Find the column that contains a colon and looks like an address:port
        let name_col = parts.iter().skip(8).find(|p| p.contains(':'))?;

        // Remove any trailing state like "(LISTEN)" by taking just the address:port part
        let addr_port =
            name_col.trim_end_matches(|c: char| c == ')' || c.is_alphabetic() || c == '(');

        let (address, port) = split_listen_address(addr_port)?;

        Some(PortInfo {
            port,
            protocol: Protocol::Tcp,
            pid,
            process_name,
            address: Some(address),
        })
    }

    /// Linux: read the kernel's socket tables directly, as `ss` does, so nothing
    /// needs to be installed. Listening sockets come from /proc/net/tcp and tcp6,
    /// and each socket's holders are all the processes with that socket open in
    /// /proc/<pid>/fd (a master and its workers share one).
    /// Sockets of other users' processes have no visible owner without root, so
    /// they are left out, as with `ss -p`.
    #[cfg(target_os = "linux")]
    fn get_listening_linux() -> Result<Vec<PortInfo>> {
        Self::listening_from(Path::new("/proc/net"), Path::new("/proc"))
    }

    /// `get_listening_linux` reading the socket tables from `net` and the
    /// processes from `procs`, which are /proc/net and /proc
    #[cfg(target_os = "linux")]
    fn listening_from(net: &Path, procs: &Path) -> Result<Vec<PortInfo>> {
        let mut sockets = Vec::new();
        let mut read_any = false;
        for (table, v6) in [("tcp", false), ("tcp6", true)] {
            // tcp6 is missing when IPv6 is disabled
            let Ok(text) = std::fs::read_to_string(net.join(table)) else {
                continue;
            };
            read_any = true;
            sockets.extend(
                text.lines()
                    .skip(1)
                    .filter_map(|line| Self::parse_proc_net_line(line, v6)),
            );
        }
        if !read_any {
            return Err(ProcError::SystemError(
                "Can't read /proc/net/tcp to look up ports".to_string(),
            ));
        }

        let wanted: std::collections::HashSet<u64> = sockets.iter().map(|s| s.0).collect();
        let owners = Self::socket_owners(&wanted, procs);

        let mut ports = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (inode, port, address) in sockets {
            let Some(pids) = owners.get(&inode) else {
                continue;
            };
            // every process with the socket open holds the port (a master and
            // its forked workers share one listen socket)
            for &pid in pids {
                // Same port on IPv4 and IPv6 is listed once, as on macOS
                if !seen.insert((port, pid)) {
                    continue;
                }
                // any process can set its own comm (prctl), escape sequences included
                let process_name = std::fs::read_to_string(procs.join(format!("{pid}/comm")))
                    .map(|name| {
                        let name = name.strip_suffix('\n').unwrap_or(&name);
                        escape_control(name).into_owned()
                    })
                    .unwrap_or_else(|_| "unknown".to_string());
                ports.push(PortInfo {
                    port,
                    protocol: Protocol::Tcp,
                    pid,
                    process_name,
                    address: Some(address.clone()),
                });
            }
        }
        Ok(ports)
    }

    /// Parse a line of /proc/net/tcp or tcp6 into (socket inode, port, address),
    /// keeping only listening sockets. Addresses are hex in the kernel's byte
    /// order: 0100007F:1F90 is 127.0.0.1:8080.
    #[cfg(target_os = "linux")]
    fn parse_proc_net_line(line: &str, v6: bool) -> Option<(u64, u16, String)> {
        const LISTEN: &str = "0A";
        let fields: Vec<&str> = line.split_whitespace().collect();
        let (local, state, inode) = (fields.get(1)?, fields.get(3)?, fields.get(9)?);
        if *state != LISTEN {
            return None;
        }
        let (addr_hex, port_hex) = local.split_once(':')?;
        let port = u16::from_str_radix(port_hex, 16).ok()?;
        let inode: u64 = inode.parse().ok()?;

        // Each 32-bit word is printed as a number in host byte order
        let word = |i: usize| -> Option<[u8; 4]> {
            let hex = addr_hex.get(i * 8..i * 8 + 8)?;
            Some(u32::from_str_radix(hex, 16).ok()?.to_ne_bytes())
        };
        let address = if v6 {
            if addr_hex.len() != 32 {
                return None;
            }
            let mut bytes = [0u8; 16];
            for i in 0..4 {
                bytes[i * 4..i * 4 + 4].copy_from_slice(&word(i)?);
            }
            std::net::Ipv6Addr::from(bytes).to_string()
        } else {
            if addr_hex.len() != 8 {
                return None;
            }
            std::net::Ipv4Addr::from(word(0)?).to_string()
        };
        Some((inode, port, address))
    }

    /// Map socket inodes to the PIDs that have them open, by reading the
    /// "socket:[inode]" links in /proc/<pid>/fd. Unreadable processes are skipped.
    #[cfg(target_os = "linux")]
    fn socket_owners(
        wanted: &std::collections::HashSet<u64>,
        procs: &Path,
    ) -> std::collections::HashMap<u64, Vec<u32>> {
        let mut owners = std::collections::HashMap::new();
        let Ok(procs) = std::fs::read_dir(procs) else {
            return owners;
        };
        for entry in procs.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<u32>().ok())
            else {
                continue;
            };
            let Ok(fds) = std::fs::read_dir(entry.path().join("fd")) else {
                continue;
            };
            for fd in fds.flatten() {
                let Ok(target) = std::fs::read_link(fd.path()) else {
                    continue;
                };
                let inode = target
                    .to_str()
                    .and_then(|t| t.strip_prefix("socket:["))
                    .and_then(|t| t.strip_suffix(']'))
                    .and_then(|t| t.parse::<u64>().ok());
                if let Some(inode) = inode.filter(|i| wanted.contains(i)) {
                    // a process with the socket open on several fds counts once
                    let pids = owners.entry(inode).or_insert_with(Vec::new);
                    if !pids.contains(&pid) {
                        pids.push(pid);
                    }
                }
            }
        }
        for pids in owners.values_mut() {
            pids.sort_unstable();
        }
        owners
    }

    #[cfg(target_os = "windows")]
    fn get_listening_windows() -> Result<Vec<PortInfo>> {
        // `netstat -p TCP` lists IPv4 sockets only; IPv6 ones need `-p TCPv6`
        let v4 = Command::new("netstat")
            .args(["-ano", "-p", "TCP"])
            .output()
            .map_err(|e| ProcError::SystemError(format!("Failed to run netstat: {}", e)))?;
        // A machine without IPv6 may refuse the second call; that leaves the
        // IPv4 listeners, not an error
        let v6 = Command::new("netstat")
            .args(["-ano", "-p", "TCPv6"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();

        // netstat gives no names; `get_all_listening` names every socket's
        // process from one process-list lookup (a `tasklist` per socket made
        // each reading take seconds, and `proc ports --in` run past a minute)
        Ok(Self::parse_netstat_output(
            &[&String::from_utf8_lossy(&v4.stdout), &v6],
            |_| None,
        ))
    }

    /// The listening ports in the outputs of netstat, one per protocol
    /// (`-p TCP` and `-p TCPv6`); `name_of` says what a PID is called
    /// (compiled on other systems for the tests only)
    #[cfg(any(target_os = "windows", test))]
    fn parse_netstat_output(
        outputs: &[&str],
        name_of: impl Fn(u32) -> Option<String>,
    ) -> Vec<PortInfo> {
        let mut ports = Vec::new();

        for line in outputs.iter().flat_map(|o| o.lines()) {
            if line.contains("LISTENING") {
                if let Some(port_info) = Self::parse_netstat_line(line, &name_of) {
                    ports.push(port_info);
                }
            }
        }

        ports
    }

    #[cfg(any(target_os = "windows", test))]
    fn parse_netstat_line(
        line: &str,
        name_of: &impl Fn(u32) -> Option<String>,
    ) -> Option<PortInfo> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            return None;
        }

        // Local address is column 2 (e.g., "0.0.0.0:135" or "[::]:135")
        let (address, port) = split_listen_address(parts[1])?;

        // PID is the last column
        let pid: u32 = parts.last()?.parse().ok()?;

        // Get process name from PID
        let process_name = name_of(pid).unwrap_or_else(|| "unknown".to_string());

        Some(PortInfo {
            port,
            protocol: Protocol::Tcp,
            pid,
            process_name,
            address: Some(address),
        })
    }

    /// Whether the port is open to the network: bound to every address. A port
    /// with no known address counts as open.
    pub fn is_exposed(&self) -> bool {
        self.address
            .as_ref()
            .map(|a| a == "0.0.0.0" || a == "::" || a == "*")
            .unwrap_or(true)
    }

    /// Whether the port is bound to loopback only
    pub fn is_local(&self) -> bool {
        self.address
            .as_ref()
            .map(|a| a == "127.0.0.1" || a == "::1")
            .unwrap_or(false)
    }
}

/// Name each row as `names` (the process list's names, by PID) does. A PID
/// missing from `names` is already gone and keeps the name its socket table
/// gave it.
fn name_as_processes(ports: &mut [PortInfo], names: &std::collections::HashMap<u32, String>) {
    for port in ports {
        if let Some(name) = names.get(&port.pid) {
            port.process_name = name.clone();
        }
    }
}

/// The listeners in `listening` that hold `port`, one per process: a process
/// that listens on both an IPv4 and an IPv6 address is one holder, listed with
/// the first address
fn holders_in(listening: Vec<PortInfo>, port: u16) -> Vec<PortInfo> {
    let mut seen = std::collections::HashSet::new();
    listening
        .into_iter()
        .filter(|p| p.port == port && seen.insert(p.pid))
        .collect()
}

/// Split a listening socket's local address, as lsof and netstat print it, into
/// the bind address and the port. IPv6 addresses come in brackets (`[::1]:631`,
/// `[fe80::1%eth0]:22`) and are returned without them, as the kernel tables on
/// Linux are; `*`, every address, is returned as `0.0.0.0`. A scope suffix
/// (`%eth0`) stays.
#[cfg(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "freebsd",
    target_os = "netbsd",
    test
))]
fn split_listen_address(local: &str) -> Option<(String, u16)> {
    let (host, port) = local.rsplit_once(':')?;
    let port = port.parse().ok()?;
    let host = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    let address = if host.is_empty() || host == "*" {
        "0.0.0.0"
    } else {
        host
    };
    Some((address.to_string(), port))
}

/// Parse a port from various formats (":3000", "3000", etc.)
pub fn parse_port(input: &str) -> Result<u16> {
    let cleaned = input.trim().trim_start_matches(':');
    cleaned
        .parse()
        .map_err(|_| ProcError::InvalidInput(format!("Invalid port: '{}'", input)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_port() {
        assert_eq!(parse_port(":3000").unwrap(), 3000);
        assert_eq!(parse_port("3000").unwrap(), 3000);
        assert_eq!(parse_port("  :8080  ").unwrap(), 8080);
    }

    #[test]
    fn test_parse_port_invalid() {
        assert!(parse_port("abc").is_err());
        assert!(parse_port("").is_err());
    }

    #[test]
    fn test_parse_lsof_line_keeps_the_full_name() {
        // With `+c 0` lsof prints the whole name, escaping spaces as \x20; the
        // default 9-character column used to turn python3.14 into "python3.1"
        let line = "Activity\\x20Monitor  644  zee  8u  IPv4 0x3cc8c443bf3e8f62  0t0  TCP 127.0.0.1:7768 (LISTEN)";
        let info = PortInfo::parse_lsof_line(line).unwrap();
        assert_eq!(info.process_name, "Activity Monitor");
        assert_eq!(info.pid, 644);
        assert_eq!(info.port, 7768);
    }

    /// (port, pid, name, address) of each entry, for comparing parses
    fn rows(ports: &[PortInfo]) -> Vec<(u16, u32, &str, &str)> {
        ports
            .iter()
            .map(|p| {
                let address = p.address.as_deref().unwrap_or("");
                (p.port, p.pid, p.process_name.as_str(), address)
            })
            .collect()
    }

    #[test]
    fn freebsd_sockstat_rows_are_listening_ports() {
        // `sockstat -4 -6 -l -P tcp` on FreeBSD 15.1, plus a command with a
        // space in its name
        let out = "\
USER COMMAND     PID FD PROTO LOCAL ADDRESS         FOREIGN ADDRESS
proc nc         2651  3 tcp6  [::1]:45680           *:*
proc nc         2650  3 tcp4  *:45679               *:*
proc nc         2649  3 tcp4  127.0.0.1:45678       *:*
root sshd       2542  6 tcp6  *:22                  *:*
root sshd       2542  7 tcp4  *:22                  *:*
proc Web Content 2700 4 tcp46 *:8080                *:*
";
        assert_eq!(
            rows(&PortInfo::parse_sockstat_output(out, false)),
            vec![
                (45680, 2651, "nc", "::1"),
                (45679, 2650, "nc", "0.0.0.0"),
                (45678, 2649, "nc", "127.0.0.1"),
                // the IPv4 row of the same port and process adds nothing
                (22, 2542, "sshd", "0.0.0.0"),
                (8080, 2700, "Web Content", "0.0.0.0"),
            ]
        );
    }

    #[test]
    fn netbsd_sockstat_rows_are_listening_tcp_ports() {
        // `sockstat -4 -6 -l -n` on NetBSD 11.0: a dot before the port, UDP
        // and raw sockets mixed in (no -P), and a connected socket
        let out = "\
USER     COMMAND    PID   FD PROTO  LOCAL ADDRESS         FOREIGN ADDRESS
root     dhcpcd     545   13 2/0    *.*                   *.*
_mdnsd   mdnsd      555    3 udp    *.65535               *.*
proc     nc         3546   3 tcp    *.45679               *.*
root     sshd       5563   6 tcp6   *.22                  *.*
root     sshd       5563   7 tcp    *.22                  *.*
proc     nc         5715   3 tcp6   ::1.45680             *.*
proc     nc         11181  3 tcp    127.0.0.1.45678       *.*
proc     nc         11181  4 tcp    127.0.0.1.45678       127.0.0.1.65534
";
        assert_eq!(
            rows(&PortInfo::parse_sockstat_output(out, true)),
            vec![
                (45679, 3546, "nc", "0.0.0.0"),
                (22, 5563, "sshd", "0.0.0.0"),
                (45680, 5715, "nc", "::1"),
                (45678, 11181, "nc", "127.0.0.1"),
            ]
        );
    }

    #[test]
    fn a_sockstat_line_that_is_not_a_listener_is_skipped() {
        for (line, dotted) in [
            (
                "USER COMMAND PID FD PROTO LOCAL ADDRESS FOREIGN ADDRESS",
                false,
            ),
            ("proc nc 2654 3 tcp4 127.0.0.1:58879 127.0.0.1:45678", false),
            ("proc nc 2654 3 tcp4 127.0.0.1:http *:*", false),
            ("proc nc x 3 tcp4 127.0.0.1:80 *:*", false),
            // an unbound socket has no port
            ("proc nc 12 3 tcp *.* *.*", true),
            ("short line", false),
        ] {
            assert!(
                PortInfo::parse_sockstat_line(line, dotted).is_none(),
                "{line}"
            );
        }
    }

    #[test]
    fn test_split_listen_address() {
        for (local, address, port) in [
            ("0.0.0.0:22", "0.0.0.0", 22),
            ("127.0.0.1:8080", "127.0.0.1", 8080),
            ("*:80", "0.0.0.0", 80),
            ("[::]:80", "::", 80),
            ("[::1]:631", "::1", 631),
            ("[::ffff:127.0.0.1]:8080", "::ffff:127.0.0.1", 8080),
            ("[fe80::1%eth0]:22", "fe80::1%eth0", 22),
            ("127.0.0.53%lo:53", "127.0.0.53%lo", 53),
            // A bare IPv6 address with the port after the last colon
            (":::22", "::", 22),
        ] {
            assert_eq!(
                split_listen_address(local),
                Some((address.to_string(), port)),
                "{local}"
            );
        }
        for bad in ["[::1]", "[::1]:http", "3000", ""] {
            assert_eq!(split_listen_address(bad), None, "{bad}");
        }
    }

    #[test]
    fn test_parse_lsof_line_ipv6_has_no_brackets() {
        // lsof brackets IPv6 addresses; the address is reported without them on
        // every OS, so `ports --local` matches "::1"
        let line =
            "python3.12  4242  zee  3u  IPv6 0x3cc8c443bf3e8f62  0t0  TCP [::1]:8000 (LISTEN)";
        let info = PortInfo::parse_lsof_line(line).unwrap();
        assert_eq!(info.address.as_deref(), Some("::1"));
        assert_eq!(info.port, 8000);
        assert_eq!(info.pid, 4242);
        assert!(info.is_local() && !info.is_exposed());
    }

    #[test]
    fn test_parse_netstat_line_keeps_the_whole_ipv6_address() {
        // The address used to be the text between the last two colons, so
        // [::]:135 came back as "]" and [::1]:8080 as "]" too, and neither
        // `ports --exposed` nor `ports --local` kept them
        let any = "  TCP    [::]:135               [::]:0                 LISTENING       1092";
        let info = PortInfo::parse_netstat_line(any, &|_| None).unwrap();
        assert_eq!(info.address.as_deref(), Some("::"));
        assert_eq!(info.port, 135);
        assert_eq!(info.pid, 1092);
        assert!(info.is_exposed() && !info.is_local());

        let loopback =
            "  TCP    [::1]:8080             [::]:0                 LISTENING       4242";
        let info = PortInfo::parse_netstat_line(loopback, &|_| None).unwrap();
        assert_eq!(info.address.as_deref(), Some("::1"));
        assert_eq!(info.port, 8080);
        assert!(info.is_local() && !info.is_exposed());

        let v4 = "  TCP    127.0.0.1:8081        0.0.0.0:0              LISTENING       4242";
        let info = PortInfo::parse_netstat_line(v4, &|_| None).unwrap();
        assert_eq!(info.address.as_deref(), Some("127.0.0.1"));
        assert!(info.is_local() && !info.is_exposed());
    }

    #[test]
    fn exposed_and_local_read_the_address() {
        let on = |address: Option<&str>| PortInfo {
            port: 1,
            protocol: Protocol::Tcp,
            pid: 1,
            process_name: "x".to_string(),
            address: address.map(String::from),
        };
        for exposed in ["0.0.0.0", "::", "*"] {
            assert!(on(Some(exposed)).is_exposed(), "{exposed}");
            assert!(!on(Some(exposed)).is_local(), "{exposed}");
        }
        for local in ["127.0.0.1", "::1"] {
            assert!(on(Some(local)).is_local(), "{local}");
            assert!(!on(Some(local)).is_exposed(), "{local}");
        }
        // Bound to one address that is neither: in neither group
        let one = on(Some("192.168.1.5"));
        assert!(!one.is_exposed() && !one.is_local());
        // No known address: counts as open, never as local
        assert!(on(None).is_exposed() && !on(None).is_local());
    }

    #[test]
    fn test_unescape_lsof_is_safe_for_any_name() {
        // Control characters stay escaped, so a process name can't inject
        // terminal escape sequences (ESC [ 2 J clears the screen)
        assert_eq!(PortInfo::unescape_lsof("evil\\x1b[2J"), "evil\\x1b[2J");
        assert_eq!(PortInfo::unescape_lsof("a\\x7fb"), "a\\x7fb");
        assert_eq!(
            PortInfo::unescape_lsof("Activity\\x20Monitor"),
            "Activity Monitor"
        );
        // "\x" followed by a multi-byte character used to panic on a byte slice
        assert_eq!(PortInfo::unescape_lsof("x\\xé1"), "x\\xé1");
        assert_eq!(PortInfo::unescape_lsof("x\\x"), "x\\x");
        assert_eq!(PortInfo::unescape_lsof("x\\x4"), "x\\x4");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_proc_net_tcp_lines() {
        // 127.0.0.1:8080 listening (state 0A), socket inode 123456
        let v4 = "   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 123456 1 0000000000000000 100 0 0 10 0";
        assert_eq!(
            PortInfo::parse_proc_net_line(v4, false),
            Some((123456, 8080, "127.0.0.1".to_string()))
        );
        // 0.0.0.0:22
        let any = "   1: 00000000:0016 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 777 1 0000000000000000 100 0 0 10 0";
        assert_eq!(
            PortInfo::parse_proc_net_line(any, false),
            Some((777, 22, "0.0.0.0".to_string()))
        );
        // An established connection (state 01) is not a listening port
        let established = "   2: 0100007F:1F90 0100007F:D431 01 00000000:00000000 00:00000000 00000000  1000        0 888 1 0000000000000000 20 4 30 10 -1";
        assert_eq!(PortInfo::parse_proc_net_line(established, false), None);
        // [::]:80 and [::1]:3000 from /proc/net/tcp6
        let v6_any = "   0: 00000000000000000000000000000000:0050 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 222 1 0000000000000000 100 0 0 10 0";
        assert_eq!(
            PortInfo::parse_proc_net_line(v6_any, true),
            Some((222, 80, "::".to_string()))
        );
        let v6_local = "   1: 00000000000000000000000001000000:0BB8 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 333 1 0000000000000000 100 0 0 10 0";
        assert_eq!(
            PortInfo::parse_proc_net_line(v6_local, true),
            Some((333, 3000, "::1".to_string()))
        );
        // Malformed lines are skipped, never a panic
        assert_eq!(PortInfo::parse_proc_net_line("garbage", false), None);
        assert_eq!(
            PortInfo::parse_proc_net_line("  0: ZZ:1F90 0:0 0A", false),
            None
        );
        // An address of the wrong length for its table
        assert_eq!(PortInfo::parse_proc_net_line(v4, true), None);
        assert_eq!(PortInfo::parse_proc_net_line(v6_any, false), None);
    }

    /// A fake /proc: `net/tcp`, `net/tcp6` and one directory per process
    #[cfg(target_os = "linux")]
    struct FakeProc {
        dir: tempfile::TempDir,
    }

    #[cfg(target_os = "linux")]
    impl FakeProc {
        const HEADER: &'static str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n";

        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir(dir.path().join("net")).unwrap();
            FakeProc { dir }
        }

        fn table(&self, name: &str, rows: &[(&str, u64)]) {
            let mut text = Self::HEADER.to_string();
            for (local, inode) in rows {
                text.push_str(&format!(
                    "   0: {local} 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 {inode} 1 0000000000000000 100 0 0 10 0\n"
                ));
            }
            std::fs::write(self.dir.path().join("net").join(name), text).unwrap();
        }

        /// A process with `comm` (if any) and one fd link per target
        fn process(&self, pid: &str, comm: Option<&str>, fds: &[&str]) {
            let p = self.dir.path().join(pid);
            std::fs::create_dir_all(p.join("fd")).unwrap();
            if let Some(comm) = comm {
                std::fs::write(p.join("comm"), comm).unwrap();
            }
            for (i, target) in fds.iter().enumerate() {
                std::os::unix::fs::symlink(target, p.join("fd").join(i.to_string())).unwrap();
            }
        }

        fn listening(&self) -> Result<Vec<PortInfo>> {
            PortInfo::listening_from(&self.dir.path().join("net"), self.dir.path())
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn listening_ports_are_matched_to_the_process_holding_the_socket() {
        let fake = FakeProc::new();
        fake.table(
            "tcp",
            &[
                ("0100007F:1F90", 100), // 127.0.0.1:8080, held by 4242
                ("00000000:0016", 200), // 0.0.0.0:22, held by nobody we can see
                ("00000000:2328", 300), // 0.0.0.0:9000, held by 4243, which has no comm
            ],
        );
        // the same port on IPv6 is the same listener
        fake.table("tcp6", &[("00000000000000000000000000000000:1F90", 101)]);
        fake.process(
            "4242",
            Some("myapp\n"),
            &["socket:[100]", "socket:[101]", "/dev/null", "socket:[x]"],
        );
        fake.process("4243", None, &["socket:[300]", "socket:[999]"]);
        // not a process, and a process whose fds can't be listed
        std::fs::create_dir(fake.dir.path().join("self")).unwrap();
        std::fs::create_dir(fake.dir.path().join("4244")).unwrap();

        let mut ports = fake.listening().unwrap();
        ports.sort_by_key(|p| p.port);
        let seen: Vec<_> = ports
            .iter()
            .map(|p| (p.port, p.pid, p.process_name.as_str(), p.address.as_deref()))
            .collect();
        assert_eq!(
            seen,
            vec![
                (8080, 4242, "myapp", Some("127.0.0.1")),
                (9000, 4243, "unknown", Some("0.0.0.0")),
            ]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn every_process_with_a_listen_socket_open_holds_the_port() {
        let fake = FakeProc::new();
        fake.table("tcp", &[("0100007F:1F90", 100)]);
        // a master and two forked workers share the socket; the master has it
        // open twice; another process has nothing to do with it
        fake.process("4242", Some("nginx\n"), &["socket:[100]", "socket:[100]"]);
        fake.process("4250", Some("nginx\n"), &["socket:[100]"]);
        fake.process("4251", Some("nginx\n"), &["socket:[100]"]);
        fake.process("4300", Some("other\n"), &["socket:[555]"]);
        let ports = fake.listening().unwrap();
        let mut pids: Vec<u32> = ports.iter().map(|p| p.pid).collect();
        pids.sort_unstable();
        assert_eq!(pids, vec![4242, 4250, 4251]);
        assert!(ports.iter().all(|p| p.port == 8080));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_missing_ipv6_table_is_not_an_error() {
        let fake = FakeProc::new();
        fake.table("tcp", &[("0100007F:1F90", 100)]);
        fake.process("4242", Some("myapp\n"), &["socket:[100]"]);
        let ports = fake.listening().unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].port, 8080);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn no_socket_tables_is_an_error() {
        let fake = FakeProc::new();
        let err = fake.listening().unwrap_err();
        assert!(
            matches!(&err, ProcError::SystemError(m) if m == "Can't read /proc/net/tcp to look up ports"),
            "{err:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unreadable_processes_leave_no_owners() {
        let fake = FakeProc::new();
        fake.table("tcp", &[("0100007F:1F90", 100)]);
        let missing = fake.dir.path().join("no-such-proc-dir");
        let ports = PortInfo::listening_from(&fake.dir.path().join("net"), &missing).unwrap();
        assert!(ports.is_empty());
    }

    #[test]
    fn lsof_output_lists_each_listener_once() {
        let out = "\
COMMAND  PID USER   FD   TYPE DEVICE SIZE/OFF NODE NAME
node     100  zee   20u  IPv4 0xa    0t0      TCP  *:3000 (LISTEN)
node     100  zee   21u  IPv6 0xb    0t0      TCP  *:3000 (LISTEN)
node     100  zee   22u  IPv4 0xc    0t0      TCP  127.0.0.1:3001 (LISTEN)
Activity\\x20Monitor 7 zee 8u IPv6 0xd 0t0 TCP [::1]:3002 (LISTEN)
short line
node     abc  zee   23u  IPv4 0xe    0t0      TCP  *:3003 (LISTEN)
node     101  zee   24u  IPv4 0xf    0t0      TCP  noport (LISTEN)
node     101  zee   25u  IPv4 0xf    0t0      TCP  *:notaport (LISTEN)
";
        let ports = PortInfo::parse_lsof_output(out);
        let seen: Vec<_> = ports
            .iter()
            .map(|p| (p.port, p.pid, p.process_name.as_str(), p.address.as_deref()))
            .collect();
        assert_eq!(
            seen,
            vec![
                (3000, 100, "node", Some("0.0.0.0")),
                (3001, 100, "node", Some("127.0.0.1")),
                (3002, 7, "Activity Monitor", Some("::1")),
            ]
        );
    }

    #[test]
    fn netstat_output_lists_the_listening_rows() {
        let out = "\
Active Connections

  Proto  Local Address          Foreign Address        State           PID
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1032
  TCP    127.0.0.1:8080         0.0.0.0:0              LISTENING       4242
  TCP    127.0.0.1:50000        127.0.0.1:8080         ESTABLISHED     5000
  TCP    0.0.0.0:99 LISTENING
  TCP    0.0.0.0:notaport       0.0.0.0:0              LISTENING       1
  TCP    0.0.0.0:136            0.0.0.0:0              LISTENING       notapid
";
        let name_of = |pid: u32| (pid == 1032).then(|| "svchost.exe".to_string());
        let ports = PortInfo::parse_netstat_output(&[out], name_of);
        let seen: Vec<_> = ports
            .iter()
            .map(|p| (p.port, p.pid, p.process_name.as_str(), p.address.as_deref()))
            .collect();
        assert_eq!(
            seen,
            vec![
                (135, 1032, "svchost.exe", Some("0.0.0.0")),
                (8080, 4242, "unknown", Some("127.0.0.1")),
            ]
        );
    }

    #[test]
    fn netstat_lists_ipv4_and_ipv6_listeners_from_both_calls() {
        // What `netstat -ano -p TCP` and `-p TCPv6` print: IPv6 rows only come
        // from the second call
        let v4 = "\
Active Connections

  Proto  Local Address          Foreign Address        State           PID
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1032
  TCP    127.0.0.1:8080         0.0.0.0:0              LISTENING       4242
";
        let v6 = "\
Active Connections

  Proto  Local Address          Foreign Address        State           PID
  TCP    [::]:135               [::]:0                 LISTENING       1032
  TCP    [::1]:8080             [::]:0                 LISTENING       4242
  TCP    [fe80::1%12]:9000      [::]:0                 LISTENING       4242
  TCP    [::1]:50000            [::1]:8080             ESTABLISHED     5000
";
        let name_of = |pid: u32| (pid == 1032).then(|| "svchost.exe".to_string());
        let ports = PortInfo::parse_netstat_output(&[v4, v6], name_of);
        let seen: Vec<_> = ports
            .iter()
            .map(|p| (p.port, p.address.as_deref().unwrap()))
            .collect();
        assert_eq!(
            seen,
            vec![
                (135, "0.0.0.0"),
                (8080, "127.0.0.1"),
                (135, "::"),
                (8080, "::1"),
                (9000, "fe80::1%12"),
            ]
        );
        assert!(ports[2].is_exposed() && ports[3].is_local());

        // No IPv6 output (netstat refused the call): the IPv4 listeners remain
        let ports = PortInfo::parse_netstat_output(&[v4, ""], |_| None);
        assert_eq!(ports.len(), 2);
    }

    #[test]
    fn a_port_held_through_two_sockets_has_two_holders() {
        let listener = |port: u16, pid: u32, address: &str| PortInfo {
            port,
            protocol: Protocol::Tcp,
            pid,
            process_name: format!("p{pid}"),
            address: Some(address.to_string()),
        };
        let listening = vec![
            listener(8080, 10, "127.0.0.1"),
            listener(9000, 12, "0.0.0.0"),
            listener(8080, 11, "::1"),
            // the same process on the IPv6 address too (netstat lists both)
            listener(8080, 10, "::1"),
        ];
        let holders = holders_in(listening.clone(), 8080);
        let seen: Vec<_> = holders
            .iter()
            .map(|p| (p.pid, p.address.as_deref().unwrap()))
            .collect();
        assert_eq!(seen, vec![(10, "127.0.0.1"), (11, "::1")]);
        assert!(holders_in(listening, 7).is_empty());
    }

    #[test]
    fn a_port_gives_its_own_process() {
        let me = PortInfo {
            port: 1,
            protocol: Protocol::Tcp,
            pid: std::process::id(),
            process_name: "me".to_string(),
            address: None,
        };
        assert_eq!(me.get_process().unwrap().unwrap().pid, std::process::id());
    }

    fn row(pid: u32, name: &str) -> PortInfo {
        PortInfo {
            port: 8000 + pid as u16,
            protocol: Protocol::Tcp,
            pid,
            process_name: name.to_string(),
            address: None,
        }
    }

    #[test]
    fn a_gone_pid_keeps_the_socket_tables_name() {
        let mut ports = vec![row(1, "python3.14"), row(2, "gone-name")];
        let mut names = std::collections::HashMap::new();
        names.insert(1, "python3".to_string());
        name_as_processes(&mut ports, &names);
        assert_eq!(ports[0].process_name, "python3", "a running PID is renamed");
        assert_eq!(
            ports[1].process_name, "gone-name",
            "a PID the process list lacks keeps the socket table's name"
        );
    }

    #[test]
    fn test_get_listening_ports() {
        // This test may or may not find ports depending on the system
        let result = PortInfo::get_all_listening();
        assert!(result.is_ok());
    }

    #[test]
    fn listening_ports_come_by_port_then_pid_and_a_process_has_its_ports_ascending() {
        // bound in the order the system hands them out, which is no rule
        let held: Vec<std::net::TcpListener> = (0..4)
            .map(|_| std::net::TcpListener::bind("127.0.0.1:0").unwrap())
            .collect();
        let ports = PortInfo::get_all_listening().unwrap();

        let keys: Vec<(u16, u32)> = ports.iter().map(|p| (p.port, p.pid)).collect();
        assert!(keys.windows(2).all(|w| w[0] <= w[1]), "{keys:?}");

        let me = std::process::id();
        let mine: Vec<u16> = ports
            .iter()
            .filter(|p| p.pid == me)
            .map(|p| p.port)
            .collect();
        assert!(mine.windows(2).all(|w| w[0] <= w[1]), "{mine:?}");
        for listener in &held {
            let port = listener.local_addr().unwrap().port();
            assert!(mine.contains(&port), "{port} is not in {mine:?}");
            let holders: Vec<u32> = PortInfo::holders_of(port)
                .unwrap()
                .iter()
                .map(|h| h.pid)
                .collect();
            assert!(holders.windows(2).all(|w| w[0] < w[1]), "{holders:?}");
        }
    }
}
