//! The connections to a port, read from the real system.
//!
//! Every port these tests look at is one the test started: a copy of the
//! listener example (`examples/listener.rs`), which binds a port and never
//! accepts. The clients are `TcpStream`s kept in the test process, so each
//! connection's peer is the test itself. The listeners are killed and reaped on
//! drop, and the clients closed before a test ends.

use proc_cli::core::connections::{connections_to, Connections};
use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Stdio};

/// `target/debug/examples/listener`, found relative to this test binary
/// (`target/debug/deps/cmd_connections-<hash>`)
fn listener_path() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("examples");
    path.push(format!("listener{}", std::env::consts::EXE_SUFFIX));
    assert!(path.exists(), "build the example first: {}", path.display());
    path
}

/// One listener the test started. Killed and reaped on drop.
struct Holder {
    child: Child,
    port: u16,
}

impl Holder {
    fn start(args: &[&str]) -> Self {
        let mut child = std::process::Command::new(listener_path())
            .args(args)
            .stdout(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .expect("spawn the listener example");
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        let read = stdout.read_line(&mut line);
        let port = read
            .ok()
            .and_then(|_| line.trim().strip_prefix("PORT "))
            .and_then(|p| p.parse().ok());
        let Some(port) = port else {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the listener printed {line:?} instead of `PORT <n>`");
        };
        Holder { child, port }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `n` connections to `address:port`, held until the caller drops them
fn connect(address: &str, port: u16, n: usize) -> Vec<TcpStream> {
    (0..n)
        .map(|_| TcpStream::connect(format!("{address}:{port}")).expect("connect to the listener"))
        .collect()
}

#[test]
fn connections_count_each_client_once() {
    let holder = Holder::start(&[]);
    let clients = connect("127.0.0.1", holder.port, 3);

    let c = connections_to(holder.port, &[holder.pid()], 50).unwrap();
    assert_eq!(c.total, 3, "{c:?}");
    assert_eq!(c.by_state.get("established"), Some(&3), "{c:?}");
    assert_eq!(c.by_state.len(), 1, "{c:?}");
    assert_eq!(c.peers.len(), 3, "{c:?}");
    assert!(!c.peers_truncated);
    for peer in &c.peers {
        assert_eq!(peer.holder_pid, holder.pid(), "{c:?}");
        assert_eq!(peer.state, "established");
        assert_eq!(peer.remote_address, "127.0.0.1");
        let process = peer
            .peer
            .as_ref()
            .expect("the test process owns the client");
        assert_eq!(process.pid, std::process::id(), "{c:?}");
        assert!(!process.name.is_empty());
    }
    // the remote ports are the clients' own
    let mut ports: Vec<u16> = c.peers.iter().map(|p| p.remote_port).collect();
    ports.sort_unstable();
    let mut local: Vec<u16> = clients
        .iter()
        .map(|s| s.local_addr().unwrap().port())
        .collect();
    local.sort_unstable();
    assert_eq!(ports, local);
    drop(clients);
}

#[test]
fn sixty_clients_cap_the_peers() {
    let holder = Holder::start(&[]);
    let clients = connect("127.0.0.1", holder.port, 60);

    let c = connections_to(holder.port, &[holder.pid()], 50).unwrap();
    assert_eq!(c.total, 60, "{c:?}");
    assert_eq!(c.by_state.get("established"), Some(&60), "{c:?}");
    assert_eq!(c.peers.len(), 50);
    assert!(c.peers_truncated);
    drop(clients);
}

#[test]
fn ipv4_and_ipv6_holders_of_one_port_each_get_their_own_connections() {
    if let Err(e) = std::net::TcpListener::bind("[::1]:0") {
        proc_cli::err_println!("skipped: no IPv6 loopback on this machine ({e})");
        return;
    }
    let v4 = Holder::start(&[]);
    let v6 = Holder::start(&["--v6", "--port", &v4.port.to_string()]);
    assert_eq!(v4.port, v6.port, "both listeners hold one port number");
    let port = v4.port;
    let mut clients = connect("127.0.0.1", port, 2);
    clients.extend(connect("[::1]", port, 1));

    let both = connections_to(port, &[v4.pid(), v6.pid()], 50).unwrap();
    assert_eq!(both.total, 3, "{both:?}");
    assert_eq!(both.by_state.get("established"), Some(&3), "{both:?}");
    let under = |c: &Connections, pid: u32| c.peers.iter().filter(|p| p.holder_pid == pid).count();
    assert_eq!(under(&both, v4.pid()), 2, "{both:?}");
    assert_eq!(under(&both, v6.pid()), 1, "{both:?}");
    assert!(both
        .peers
        .iter()
        .all(|p| p.peer.as_ref().map(|x| x.pid) == Some(std::process::id())));

    // each holder alone is asked only about its own socket
    let first = connections_to(port, &[v4.pid()], 50).unwrap();
    assert_eq!((first.total, under(&first, v4.pid())), (2, 2), "{first:?}");
    let second = connections_to(port, &[v6.pid()], 50).unwrap();
    assert_eq!(
        (second.total, under(&second, v6.pid())),
        (1, 1),
        "{second:?}"
    );
    drop(clients);
}

#[test]
fn a_port_nobody_holds_has_no_connections() {
    // a port the OS just handed out and took back, with no holder to ask about
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    assert_eq!(
        connections_to(port, &[], 50).unwrap(),
        Connections::default()
    );
    // a holder that does not hold it
    let c = connections_to(port, &[std::process::id()], 50).unwrap();
    assert_eq!(c, Connections::default());
}

#[test]
fn a_listener_with_no_clients_has_no_connections() {
    let holder = Holder::start(&[]);
    let c = connections_to(holder.port, &[holder.pid()], 50).unwrap();
    assert_eq!(c, Connections::default());
}
