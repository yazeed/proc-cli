---
title: "How to check open ports on Linux, Mac and Windows"
description: "List the ports listening on your machine and the program behind each, with ss, lsof, netstat and PowerShell; tell a port open to the network from one only this machine can reach; and fix \"lsof: command not found\"."
date: 2026-11-12
---

"Which ports are open?" usually means one of two things: which ports have a program **listening** on this machine, or which of those can be **reached from the network**. The first is a single command on every system. The second depends on the address each program listens on, and on the firewall.

## The short answer

```bash
# Linux
sudo ss -tlnp

# macOS (and Linux with lsof)
sudo lsof -iTCP -sTCP:LISTEN -P -n
```

```bat
:: Windows (Command Prompt)
netstat -ano | findstr LISTENING
```

Each lists the TCP ports something is listening on, with the program. Run them with `sudo` (or as administrator on Windows) to see other users' and the system's programs too.

## Linux: ss

`ss` comes with every current Linux distribution and reads the kernel's socket table directly.

```bash
sudo ss -tlnp
```

```
State   Recv-Q  Send-Q  Local Address:Port  Peer Address:Port  Process
LISTEN  0       511     0.0.0.0:3000        0.0.0.0:*          users:(("node",pid=4821,fd=22))
LISTEN  0       244     127.0.0.1:5432      0.0.0.0:*          users:(("postgres",pid=912,fd=7))
```

- `-t` TCP, `-l` listening only, `-n` numbers instead of service names, `-p` the process.
- Add `-u` for UDP: `sudo ss -tulnp`.
- One port: `sudo ss -tlnp 'sport = :3000'`.

Without `sudo`, the Process column stays empty for programs you don't own.

## macOS and Linux: lsof

`lsof` ("list open files") ships with macOS and is installed on most Linux systems:

```bash
# every listening TCP port
sudo lsof -iTCP -sTCP:LISTEN -P -n

# one port
sudo lsof -i :3000 -sTCP:LISTEN -P -n
```

```
COMMAND    PID USER   FD   TYPE DEVICE SIZE/OFF NODE NAME
node      4821 dev    22u  IPv6 0x1a2b      0t0  TCP *:3000 (LISTEN)
postgres   912 dev     7u  IPv4 0x3c4d      0t0  TCP 127.0.0.1:5432 (LISTEN)
```

`-P` and `-n` keep port numbers and addresses as numbers (`3000`, not a service name from `/etc/services`), which is also faster.

### lsof: command not found

Minimal Linux installs and most Docker images don't include it. Install it:

```bash
sudo apt install lsof        # Debian, Ubuntu
sudo dnf install lsof        # Fedora, RHEL
sudo apk add lsof            # Alpine
sudo pacman -S lsof          # Arch
```

Or don't: `ss` (above) gives the same answer for ports and is usually installed. `netstat: command not found` is the same story: it's in the old `net-tools` package, which many distributions no longer install, and `ss` replaces it.

## macOS: netstat

macOS's `netstat` lists listening ports too, and `-v` adds the PID:

```bash
netstat -anv -p tcp | grep LISTEN
```

The PID column is wide and unlabelled in places; `lsof` is easier to read on a Mac.

## Windows: netstat and PowerShell

```bat
netstat -ano | findstr LISTENING
```

```
  TCP    0.0.0.0:3000           0.0.0.0:0              LISTENING       4821
  TCP    127.0.0.1:5432         0.0.0.0:0              LISTENING       912
```

The last column is the PID; `tasklist /FI "PID eq 4821"` names the program. PowerShell gives objects instead of text:

```powershell
# every listening port, with its PID
Get-NetTCPConnection -State Listen

# the program behind a PID
Get-Process -Id 4821
```

## Open to the network, or only to this machine?

A listening port isn't necessarily open to anyone else. Look at the address each program listens on:

| Listens on | Who can connect |
|------------|-----------------|
| `127.0.0.1` or `::1` | Only this machine |
| `0.0.0.0`, `::` or `*` | Any network this machine is on, unless a firewall blocks it |
| One of your addresses, such as `192.168.1.20` | Anyone who can reach that address |

So in the examples above, Postgres on `127.0.0.1:5432` is private, and the Node server on `0.0.0.0:3000` is reachable from your network. A Node or Go server given only a port (`listen(3000)`, `:3000`) listens on every address, which is worth knowing on café Wi-Fi; Vite, Rails and Django's development servers listen on `127.0.0.1` unless told otherwise.

A firewall can still block a port that listens on everything:

```bash
sudo ufw status                   # Ubuntu
sudo firewall-cmd --list-ports    # Fedora, RHEL
```

To test from **another** machine whether a port really answers:

```bash
nc -zv 192.168.1.20 3000
```

```powershell
Test-NetConnection 192.168.1.20 -Port 3000
```

Only scan machines you're responsible for: port scans of other people's hosts can break their terms of service or the law.

## With proc

[proc](https://github.com/yazeed/proc-cli) lists listening ports the same way on Linux, macOS, Windows, FreeBSD and NetBSD, and says for each one whether the network can reach it:

```bash
proc ports
```

```
 PORT  PROTO  ADDRESS    EXPOSURE  PID   NAME      UPTIME  COMMAND
 3000  TCP    0.0.0.0    exposed   4821  node      2h 4m   node server.js
 5432  TCP    127.0.0.1  local     912   postgres  3d 1h   /usr/lib/postgresql/16/bin/postgres
```

```bash
# only the ports open to the network
proc ports --exposed

# a range, or one port with its connections
proc ports --range 3000-9000
proc on :3000
```

On Linux it reads the kernel's socket table itself, so it works where `lsof` isn't installed. When a port belongs to another user and proc can't see which process holds it, it says so instead of leaving it out.

To stop what's on a port, see [find and kill a process on a port](/posts/find-kill-process-on-port-macos-linux/). For the three classic tools side by side, see [ss vs netstat vs lsof](/posts/ss-vs-netstat-vs-lsof/).

## Install

```bash
# macOS
brew install yazeed/proc/proc

# npm or bun, any OS
npm install -g proc-cli

# Rust
cargo install proc-cli
```

On Windows: `scoop bucket add proc https://github.com/yazeed/scoop-bucket-proc` and `scoop install proc`. The [install page](/install) lists every other way.
