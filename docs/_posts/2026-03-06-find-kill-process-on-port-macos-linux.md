---
title: "How to find and kill a process on a port (Mac, Linux and Windows)"
description: "Find what's using a port and kill it on macOS, Linux and Windows: lsof, ss, netstat, fuser, taskkill, PowerShell and npx kill-port, with the ports that are usually taken (3000, 5000, 8080)."
date: 2026-03-06
last_modified_at: 2026-11-12
---

You start your dev server and get this:

```
Error: listen EADDRINUSE: address already in use :::3000
```

Something is already bound to port 3000. Maybe a crashed process, a forgotten background job, or a previous run of the same server that didn't shut down cleanly. You need to find what's on the port, then kill it.

This is one of the most common tasks in day-to-day development, and the commands are different depending on your OS, which tool is available, and what output format you need. This post covers every reliable method on macOS, Linux and Windows.

## The short answer

Swap `3000` for your port. Each block first shows what holds the port, then stops it.

```bash
# macOS and Linux
lsof -i :3000 -sTCP:LISTEN
kill 12345   # PID from column 2

# Linux without lsof
sudo ss -tlnp 'sport = :3000'
sudo fuser -k -TERM 3000/tcp
```

```bat
:: Windows (Command Prompt)
netstat -ano | findstr :3000
taskkill /PID 12345 /F
```

Each tool is explained below, with its quirks: what "Access is denied" means, why the port can stay busy after the process is gone, and which ports you shouldn't free by killing.

## lsof -i :PORT

`lsof` stands for "list open files." On Unix systems, network sockets are files, so `lsof` can find processes bound to network ports. It works on both macOS and Linux, though some flags behave differently.

### Basic usage

```bash
lsof -i :3000
```

This lists all processes with any connection involving port 3000 — listening, established, CLOSE_WAIT and so on. It never shows TIME_WAIT, because no process owns those sockets. On a busy server, this can return a lot of rows.

The output looks like this:

```
COMMAND   PID   USER   FD   TYPE DEVICE SIZE/OFF NODE NAME
node    12345   user   23u  IPv6 0x1234      0t0  TCP *:3000 (LISTEN)
node    12345   user   24u  IPv6 0x5678      0t0  TCP localhost:3000->localhost:52431 (ESTABLISHED)
```

### Useful flags

**`-P` (no port name resolution):** Without this, lsof may show `hbci` instead of `3000` because it looks up port names from `/etc/services`. Always use `-P` if you want to see actual port numbers.

**`-n` (no hostname resolution):** Prevents DNS lookups on IP addresses. Faster output and avoids hangs when DNS is misconfigured.

**`-sTCP:LISTEN` (listening sockets only):** Filters to only processes that are *listening* on the port — the ones actually blocking it. This is usually what you want.

**`-t` (terse/PID only):** Outputs just the PID, one per line. Ideal for piping into `kill`.

### Recommended combinations

```bash
# See what's listening on port 3000 (fast, readable)
lsof -i :3000 -P -n -sTCP:LISTEN

# Get just the PID (for scripting)
lsof -i :3000 -P -n -sTCP:LISTEN -t

# Find and kill in one line
lsof -i :3000 -t | xargs kill

# Check a range of ports
lsof -i :3000-3010 -P -n -sTCP:LISTEN
```

### macOS vs Linux differences

On macOS, `lsof` is always available — it ships with the OS.

On Linux, lsof may or may not be installed. Most distributions include it, but minimal Docker images and Alpine-based containers often don't. You can install it with `apt install lsof` or `yum install lsof`.

One subtle difference: a dual-stack socket appears once (as IPv6, `*:3000`), while an app that opens separate IPv4 and IPv6 sockets appears twice. This depends on the app, not the OS.

## ss -tlnp (Linux only)

`ss` is the modern replacement for `netstat` on Linux. It's faster and gives more detailed socket information. It reads directly from kernel data structures via netlink, while netstat parses `/proc/net/tcp`.

`ss` is **Linux-only**. It does not exist on macOS.

### Flag breakdown

```bash
ss -tlnp
```

- `-t` — TCP sockets only (use `-u` for UDP, or both for `-tu`)
- `-l` — Listening sockets only (not established connections)
- `-n` — Numeric output (don't resolve service names)
- `-p` — Show process information (which process owns the socket)

### Filtering by port

You can grep the output, but `ss` also has built-in filter syntax:

```bash
# Grep approach
ss -tlnp | grep :3000

# Native filter syntax
ss -tlnp 'sport = :3000'

# Port range (native syntax)
ss -tlnp 'sport >= :3000 and sport <= :3010'
```

The native filter syntax is more reliable than grep because it won't accidentally match a PID or other field that happens to contain "3000."

### Output format

```
State    Recv-Q  Send-Q   Local Address:Port   Peer Address:Port  Process
LISTEN   0       128      0.0.0.0:3000         0.0.0.0:*          users:(("node",pid=12345,fd=23))
```

The process information is in a structured but unusual format: `users:(("name",pid=N,fd=N))`. Extracting the PID requires some parsing (covered in the awk section below).

### Note on permissions

`ss -p` needs root privileges to show process information for sockets owned by other users. If you run it without `sudo` and the PID column is empty, that's why.

```bash
# If process column is blank, try:
sudo ss -tlnp 'sport = :3000'
```

## netstat

`netstat` is the classic tool for network statistics. It exists on both macOS and Linux, but the flags and output differ significantly between platforms.

### Linux

```bash
netstat -tlnp
```

- `-t` — TCP
- `-l` — Listening only
- `-n` — Numeric (no name resolution)
- `-p` — Show PID and program name

Output:

```
Proto Recv-Q Send-Q Local Address     Foreign Address   State    PID/Program name
tcp   0      0      0.0.0.0:3000      0.0.0.0:*         LISTEN   12345/node
```

**Note:** `netstat` is deprecated on Linux. It's part of the `net-tools` package, which is no longer installed by default on many distributions (Ubuntu 18.04+, Fedora, Arch). Use `ss` instead. If you're writing scripts that need to be portable across old and new Linux systems, check for `ss` first, then fall back to `netstat`.

### macOS

macOS has `netstat` but its `-p` flag selects a protocol, not a process. Plain `netstat -an` does not show which process owns a socket, but `netstat -anv` adds `pid` and `epid` columns.

```bash
# macOS: shows listening TCP sockets, but no PID
netstat -an | grep LISTEN | grep '\.3000 '

# macOS: -v adds pid and epid columns
netstat -anv | grep LISTEN | grep '\.3000 '
```

On macOS, `netstat -anv` adds `pid` and `epid` columns, but the output is wide and hard to read. `lsof` is still easier on macOS.

### Key difference summary

| Feature | Linux `netstat -tlnp` | macOS `netstat -an` |
|---------|----------------------|---------------------|
| Shows PID | Yes | Only with `-v` |
| `-p` flag | Shows PID/name | Protocol filter (different meaning) |
| Port format | `0.0.0.0:3000` | `*.3000` or `127.0.0.1.3000` |
| Status | Deprecated (use `ss`) | Still the system netstat |

## fuser PORT/tcp

`fuser` identifies processes using files or sockets. The `fuser PORT/tcp` syntax is a Linux (psmisc) feature. macOS ships a `fuser`, but it only takes file names, so `fuser 3000/tcp` fails there.

### Basic usage

```bash
fuser 3000/tcp
```

Output:

```
3000/tcp:            12345
```

That's it — just the port and PID. Quick, but minimal.

### Important quirk: stderr

`fuser` prints the port label (`3000/tcp:`) to **stderr** and the PID to **stdout**. This matters for scripting:

```bash
# This captures the PID only
pid=$(fuser 3000/tcp 2>/dev/null)

# This prints only the PID (the label went to stderr and was dropped)
fuser 3000/tcp 2>/dev/null
# Output: 12345

# Common mistake — this captures the label too
result=$(fuser 3000/tcp 2>&1)
# result = "3000/tcp:            12345"
```

### The -k flag

```bash
fuser -k 3000/tcp
```

This sends SIGKILL to every process on the port. **No confirmation, no preview.** It kills immediately. Be cautious with this on production systems where multiple processes might share a port (e.g., behind a load balancer or with SO_REUSEPORT).

You can specify a signal:

```bash
fuser -k -TERM 3000/tcp   # Send SIGTERM instead of SIGKILL
fuser -k -INT 3000/tcp    # Send SIGINT
```

## Windows: netstat, taskkill and PowerShell

Windows has neither `lsof` nor `ss`. Its `netstat` shows the owning PID with `-o`.

### Command Prompt

```bat
netstat -ano | findstr :3000
```

```
  TCP    0.0.0.0:3000           0.0.0.0:0              LISTENING       12345
  TCP    [::]:3000              [::]:0                 LISTENING       12345
```

The last column is the PID. `findstr :3000` also matches ports 30000 to 30009 and connections *to* a remote port 3000, so read the local address (the second column) and the `LISTENING` state. To see what the PID is:

```bat
tasklist /FI "PID eq 12345"
```

Then stop it:

```bat
taskkill /PID 12345 /F
```

Without `/F`, `taskkill` asks the program to close, which a windowed app can honour but a console program such as `node` or `python` usually ignores. `/F` ends it immediately, like `kill -9`. Add `/T` to end its child processes too.

### PowerShell

PowerShell can ask for the listening socket directly instead of filtering text:

```powershell
# What's listening on port 3000, and which program it is
Get-NetTCPConnection -LocalPort 3000 -State Listen
Get-Process -Id (Get-NetTCPConnection -LocalPort 3000 -State Listen).OwningProcess

# Stop it
$ids = (Get-NetTCPConnection -LocalPort 3000 -State Listen).OwningProcess | Select-Object -Unique
Stop-Process -Id $ids -Force
```

`Get-NetTCPConnection` reports an error when nothing listens on the port, which is also the answer: the port is free.

### "Access is denied"

A process started by another user, or by a service, can only be stopped from a terminal opened with **Run as administrator**. Services are better stopped as services (`net stop <name>` or the Services app), since their manager may restart a killed process.

## Extracting PIDs with awk

Each tool has a different output format. Here's how to parse each one to get a clean PID.

### From lsof

```bash
# Skip the header line (NR>1), print the PID column ($2)
lsof -i :3000 -P -n | awk 'NR>1 {print $2}'
```

Or just use `lsof -t` which does this for you. But the awk version is useful when you want to extract other columns too:

```bash
# Get PID and command name
lsof -i :3000 -P -n | awk 'NR>1 {print $2, $1}'
```

### From ss

`ss` buries the PID inside a structured string like `users:(("node",pid=12345,fd=23))`. Extracting it:

```bash
# Using grep -oP (Perl regex, Linux only)
ss -tlnp | grep :3000 | grep -oP 'pid=\K[0-9]+'

# Using awk and match (gawk only — capture groups are a gawk extension)
ss -tlnp | grep :3000 | awk '{match($0, /pid=([0-9]+)/, a); print a[1]}'

# Using sed
ss -tlnp | grep :3000 | sed -n 's/.*pid=\([0-9]*\).*/\1/p'
```

Note: `grep -P` (Perl-compatible regex) is available on Linux via GNU grep but not on macOS by default. On macOS you'd use `grep -oE` with a slightly different pattern, but since `ss` itself is Linux-only, this is fine.

### From netstat (Linux)

```bash
# The PID/name is in column 7, formatted as "12345/node"
netstat -tlnp 2>/dev/null | awk '/:3000 / {split($7, a, "/"); print a[1]}'
```

The `split($7, a, "/")` breaks `12345/node` on the `/` delimiter and takes the first part (the PID).

### From fuser

```bash
# fuser already outputs just the PID to stdout
fuser 3000/tcp 2>/dev/null
```

If multiple PIDs are returned (space-separated), you can iterate:

```bash
fuser 3000/tcp 2>/dev/null | xargs -n1 echo
```

## Quick reference table

| Tool | OS | Command | Output |
|------|----|---------|--------|
| lsof | macOS, Linux | `lsof -i :3000 -P -n` | Full table (PID, user, command, etc.) |
| lsof | macOS, Linux | `lsof -i :3000 -t` | PID only, one per line |
| ss | Linux | `ss -tlnp 'sport = :3000'` | Table with process info in parens |
| netstat | Linux | <code markdown="0">netstat -tlnp &#124; grep :3000</code> | Table with PID/name column |
| netstat | macOS | <code markdown="0">netstat -anv &#124; grep '\.3000 '</code> | Table with a pid column |
| fuser | Linux | `fuser 3000/tcp` | PID to stdout, label to stderr |
| netstat | Windows | <code markdown="0">netstat -ano &#124; findstr :3000</code> | Table, PID in the last column |
| PowerShell | Windows | `Get-NetTCPConnection -LocalPort 3000 -State Listen` | Objects with `OwningProcess` |

## Killing the process

Once you have the PID, you need to send it a signal.

### kill PID (SIGTERM, signal 15)

```bash
kill 12345
# Equivalent to:
kill -15 12345
kill -TERM 12345
```

SIGTERM asks the process to shut down gracefully. The process can catch this signal, finish writing files, close database connections, and exit cleanly. **This is what you should try first.**

### kill -9 PID (SIGKILL)

```bash
kill -9 12345
kill -KILL 12345
```

SIGKILL immediately terminates the process. It cannot be caught, blocked, or ignored. The kernel removes the process without giving it a chance to clean up. Use this when:

- `kill` (SIGTERM) didn't work after a few seconds
- The process is completely unresponsive
- You don't care about graceful shutdown (e.g., a dev server)

**Don't default to `kill -9`.** It can leave temporary files, lock files, corrupted databases, and orphaned child processes. It's the "pull the power cord" option.

### kill -2 PID (SIGINT)

```bash
kill -2 12345
kill -INT 12345
```

SIGINT is the same signal sent by Ctrl+C. Some applications handle SIGINT differently from SIGTERM — for example, a Node.js server might print a shutdown message and clean up connections on SIGINT because that's what the developer tested.

### The full sequence for a stubborn process

```bash
# 1. Ask nicely
kill 12345

# 2. Wait a moment
sleep 2

# 3. Check if it's gone
kill -0 12345 2>/dev/null && echo "still running" || echo "stopped"

# 4. If still running, force it
kill -9 12345
```

`kill -0` doesn't send a signal — it just checks whether the process exists. The exit code tells you: 0 means it's still alive, non-zero means it's gone.

## One-liner patterns

Putting it all together — find the PID and kill it in one command:

```bash
# lsof (macOS and Linux)
lsof -i :3000 -t | xargs kill

# lsof with force kill
lsof -i :3000 -t | xargs kill -9

# fuser (Linux)
fuser -k -TERM 3000/tcp

# ss + awk + kill (Linux)
kill $(ss -tlnp 'sport = :3000' | grep -oP 'pid=\K[0-9]+')
```

```powershell
# PowerShell (Windows)
Stop-Process -Id ((Get-NetTCPConnection -LocalPort 3000 -State Listen).OwningProcess | Select-Object -Unique) -Force
```

### npx kill-port

If you're in a Node project, `npx kill-port 3000` is the one-liner people reach for (`npx kill-port 3000 8080` for several). It's handy, but know what it runs: on macOS and Linux it pipes `lsof` into `xargs kill -9`, and on Windows it runs `TaskKill /F`. So it force-kills whatever holds the port, with no look at what that is and no chance for the process to shut down cleanly. That's fine for your own dev server and risky for anything else on the port.

## Gotchas

### Root-owned processes require sudo

If a process was started by root (or another user), you can't kill it as your regular user. You also might not see it in `ss -p` or `fuser` output without elevated privileges.

```bash
# If "Operation not permitted":
sudo lsof -i :80 -t | sudo xargs kill
sudo fuser -k 80/tcp
```

On Linux, ports below 1024 require root to bind, so processes on ports like 80 or 443 are often root-owned. (macOS 10.14 and later lets any user bind them.)

### IPv4 vs IPv6

A process may listen on IPv4 (`0.0.0.0:3000`), IPv6 (`[::]:3000`), or both. Most modern applications use dual-stack sockets, listening on IPv6 which also accepts IPv4 connections.

`lsof -i :3000` shows both by default. If you want to filter:

```bash
lsof -i 4TCP:3000   # IPv4 only
lsof -i 6TCP:3000   # IPv6 only
```

With `ss`:

```bash
ss -tlnp4 'sport = :3000'   # IPv4
ss -tlnp6 'sport = :3000'   # IPv6
```

### Multiple processes on the same port

With `SO_REUSEPORT` (common in Go, nginx, and some Node.js cluster setups), multiple processes can listen on the same port. Your `lsof` or `ss` query may return several PIDs. Make sure you're killing the right ones — or all of them, if that's what you intend.

```bash
# See all processes on port 3000
lsof -i :3000 -P -n -sTCP:LISTEN

# Count them
lsof -i :3000 -P -n -sTCP:LISTEN -t | wc -l
```

### TIME_WAIT sockets

After killing a process, the port might still appear "in use" for up to 60 seconds due to TCP TIME_WAIT. This is the kernel keeping the socket open to handle any remaining packets from the closed connection.

You can't kill a TIME_WAIT socket — there's no process to kill. You have two options:

1. **Wait.** It will clear on its own (usually 30-60 seconds).
2. **Set SO_REUSEADDR** in your application, which most frameworks do by default. This lets a new process bind to the port even if TIME_WAIT sockets exist.

If `netstat` or `ss` shows an entry with no PID in TIME_WAIT, that's what's happening. `lsof` never lists these sockets.

## Ports that are usually taken

The same few ports cause most "already in use" errors, and the right fix depends on who holds them:

- **3000, 5173, 8000, 4200:** dev servers (Node, Next.js, React, Vite, Rails, Django, Angular). Usually your own server from an earlier run, a terminal tab you forgot, or a watcher that didn't exit: safe to stop once you've seen it's yours.
- **5000 and 7000 on a Mac:** since macOS 12 Monterey, the AirPlay Receiver listens on both, in a process called `ControlCenter`. Killing it doesn't help: it comes back. Turn off **System Settings → General → AirDrop & Handoff → AirPlay Receiver** (on Monterey, **System Preferences → Sharing → AirPlay Receiver**), or run your app on another port.
- **8080:** Java servers (Tomcat, Jenkins), proxies and the second dev server you started.
- **5432 (Postgres), 3306 (MySQL), 6379 (Redis), 27017 (MongoDB):** databases, often run as services. A service manager restarts what you kill, so stop it through the manager instead (`brew services list`, then `brew services stop <name>`; `sudo systemctl stop redis`; or the Services app on Windows), or point your app at the one that's already running.

If the process you killed keeps coming back on the same port, something is restarting it: a service manager, `nodemon`, a Docker container (`docker ps` shows its published ports) or a process supervisor. Stop that instead.

## The one-liner way

All of the above works. But if you're doing this regularly — killing dev servers, checking what's on a port, freeing up ports — it's a lot of flags to remember, and the commands are different on macOS vs Linux.

[proc](https://github.com/yazeed/proc-cli) is a cross-platform CLI that does this with one syntax, on macOS, Linux and Windows:

```bash
# What's on port 3000: the process, its command, directory and who's connected
proc on :3000

# Free the port: shows what it will stop and asks first, asks each holder to
# exit, forces any still running after 5 seconds, then checks the port is free
proc free :3000

# Several ports at once
proc free :3000 :8080 :4200

# Kill immediately (also asks first)
proc kill :3000
```

`proc free` and `proc kill` stop every process holding the port, including workers that share the socket, and list them before they act, so you see whether it's your dev server or something you'd rather keep. Add `--yes` to skip the question in a script. No flags to remember, no awk pipelines to construct.

## Install

```bash
# macOS
brew install yazeed/proc/proc

# npm or bun, any OS
npm install -g proc-cli

# Rust
cargo install proc-cli
```

On Windows: `scoop bucket add proc https://github.com/yazeed/scoop-bucket-proc` and `scoop install proc`. The [install page](/install) lists every other way: apt, dnf, pip, Nix, Docker and more.
