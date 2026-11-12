---
title: "ss vs netstat vs lsof: a practical guide"
description: "When to use ss, netstat, or lsof for network debugging. How each works under the hood, output differences, and awk patterns for parsing."
date: 2026-03-06
---

Three tools, overlapping purposes, different strengths. If you've ever needed to figure out what's listening on a port, you've used at least one of `lsof`, `ss`, or `netstat`. Most people just use whichever they learned first and never think about it again.

That's fine until it isn't. Maybe you're debugging on a minimal Alpine container and `lsof` isn't there. Maybe you're writing a shell script that needs to work on both macOS and Linux. Maybe you're wondering why your port lookup takes 400ms when it should be instant.

This post covers what each tool actually does under the hood, when to pick which, and how to parse their output. If you only need the list of open ports, [how to check open ports on Linux, Mac and Windows](/posts/check-open-ports-linux-mac-windows/) has the one command for each system.

## lsof: walking the file descriptor table

`lsof` stands for "list open files." In Unix, sockets are files, so `lsof` can show you network connections — but that's a side effect of its real purpose, which is enumerating everything every process has open.

**How it works:** `lsof` iterates over `/proc/<pid>/fd` for every process on the system (on Linux) or queries the kernel's file descriptor table via `proc_info` syscalls (on macOS). For each file descriptor, it determines the type — regular file, pipe, socket, device — and collects metadata. When you pass `-i`, it filters to only show network sockets, but internally it's still walking the full table and then discarding non-network entries.

This is why `lsof` is slower than `ss`. It's doing more work. On a system with thousands of processes and tens of thousands of open file descriptors, that walk takes time.

**Key flags:**

```bash
lsof -i :3000            # All connections involving port 3000
lsof -i -P -n            # All network files, numeric ports and hosts
lsof -i -sTCP:LISTEN     # Only TCP sockets in LISTEN state
lsof -i :3000 -t         # Terse: print only PIDs
lsof -i :3000 -P -n      # Numeric output (skip DNS/service resolution)
```

A few things to note:

- **`-P` and `-n` are almost always what you want.** Without them, `lsof` resolves port numbers to service names (3000 becomes "hbci") and IP addresses to hostnames. This triggers DNS lookups that can add seconds of latency, or hang entirely if DNS is misconfigured. If your `lsof` call is slow, add `-P -n` first.
- **`-t` is for scripting.** It outputs bare PIDs, one per line, which you can pipe directly to `kill` or `xargs`.
- **`-sTCP:LISTEN` filters by socket state.** Without it, you'll see all TCP states — ESTABLISHED, CLOSE_WAIT and so on — not just the listening socket. TIME_WAIT never appears, because no process owns those sockets.

**Platform support:** macOS and Linux. This is lsof's biggest practical advantage. It's the one network diagnostic tool that works the same way on both platforms. On macOS, it's the primary tool since `ss` doesn't exist.

**Example output:**

```
COMMAND   PID   USER   FD   TYPE    DEVICE SIZE/OFF NODE NAME
node    12345   dev   22u  IPv6 0x1234abc      0t0  TCP *:3000 (LISTEN)
```

Nine columns. The PID is column 2. The process name is column 1. The socket state is at the end, in parentheses.

## ss: reading kernel socket tables directly

`ss` stands for "socket statistics." It's part of the `iproute2` package on Linux and was designed specifically to replace `netstat`.

**How it works:** `ss` uses netlink sockets to query the kernel's socket tables directly. It calls `SOCK_DIAG` netlink messages, which return structured data about active sockets without needing to walk the process table. (With `-p`, `ss` does also scan `/proc/<pid>/fd` to find each socket's owner, as `netstat -p` does.) This is fundamentally different from `lsof` — instead of asking "what files does each process have open?", `ss` asks "what sockets exist?" The kernel answers from its own internal hash tables, which is fast.

On modern Linux kernels (3.3+), `ss` uses `INET_DIAG` for TCP/UDP sockets. The kernel responds with binary-encoded socket data that `ss` decodes directly. No text parsing of `/proc` files for the socket data itself. Without `-p`, no per-process iteration.

**Key flags:**

```bash
ss -tlnp                 # TCP, listening, numeric, show process
ss -tanp                 # TCP, all states, numeric, show process
ss -tlnp 'sport = :3000' # Built-in filter: source port 3000
ss -tlnp 'sport >= :3000 and sport <= :4000'  # Port range
```

The `-t` (TCP), `-l` (listening), `-n` (numeric), `-p` (process) combination is so common you'll see `ss -tlnp` in nearly every guide. The flag order doesn't matter.

**The built-in filter syntax is underused.** Instead of piping to `grep`, you can pass filter expressions directly:

```bash
ss -tnp 'dst 10.0.0.0/8'           # Connections to 10.x.x.x
ss -tnp 'dport = :443'             # Connections to remote port 443
ss -tlnp 'sport = :3000 or sport = :8080'  # Multiple ports
```

These filters are evaluated inside the kernel via netlink, so they're faster than grepping the output.

**Platform support:** Linux only. This is the hard constraint. macOS has no `ss` equivalent because macOS doesn't have netlink.

**Example output:**

```
State   Recv-Q  Send-Q  Local Address:Port  Peer Address:Port  Process
LISTEN  0       511     0.0.0.0:3000        0.0.0.0:*          users:(("node",pid=12345,fd=22))
```

Six columns. The process info is in column 6, packed into a `users:((...))` format that requires extraction.

## netstat: the legacy path

`netstat` reads `/proc/net/tcp`, `/proc/net/udp`, and related virtual files on Linux. On macOS, it uses `sysctl` and the routing table interfaces. It was the standard network diagnostic tool for decades, and it still ships by default on macOS.

**How it works (Linux):** The `/proc/net/tcp` file is a text table maintained by the kernel. Each line represents one socket, with fields like local address, remote address, state, and inode number encoded in hexadecimal. `netstat` reads this file, parses the hex-encoded addresses, resolves the socket inodes back to processes (by walking `/proc/<pid>/fd`, similar to `lsof`), and prints human-readable output.

This is slower than `ss` for two reasons: it's parsing a text representation instead of receiving binary data via netlink, and it needs to do the same per-process fd walk to map sockets to PIDs.

**How it works (macOS):** On macOS, `netstat` uses the `sysctl` interface and routing socket APIs. The implementation is completely different from the Linux version, and the flags don't fully overlap.

**Flag differences between platforms:**

```bash
# Linux
netstat -tlnp            # TCP, listening, numeric, show PID/program

# macOS
netstat -anp tcp         # All, numeric, protocol tcp
netstat -vanp tcp        # Verbose, all, numeric, protocol tcp
```

The `-p` flag is a common gotcha. On Linux, `-p` means "show process." On macOS, `-p` takes a protocol argument (`tcp`, `udp`). They're completely different flags that happen to share a letter.

**Deprecation status:** On Linux, `netstat` is part of the `net-tools` package, which has been deprecated in favor of `iproute2` (which includes `ss`). Many modern Linux distributions no longer install `net-tools` by default. On macOS, `netstat` is still the standard and is actively maintained — but its output is limited compared to `lsof -i`.

**Example output (Linux):**

```
Proto Recv-Q Send-Q Local Address     Foreign Address   State       PID/Program name
tcp        0      0 0.0.0.0:3000      0.0.0.0:*         LISTEN      12345/node
```

The PID and program name are in the last column, separated by a `/`.

**Example output (macOS):**

```
Proto Recv-Q Send-Q  Local Address      Foreign Address    (state)
tcp4       0      0  *.3000             *.*                LISTEN
```

Note: plain macOS `netstat` doesn't show PIDs. Add `-v` to get `pid` and `epid` columns, but the output is wide, and most macOS users end up using `lsof` instead.

## Comparison

| | lsof | ss | netstat |
|---|---|---|---|
| **OS support** | macOS + Linux | Linux only | macOS + Linux |
| **Speed** | Slow (walks fd table) | Fast (kernel netlink; slower with `-p`) | Medium (parses /proc text) |
| **What it reads** | Per-process fd table | Kernel socket tables via netlink | /proc/net/* (Linux), sysctl (macOS) |
| **Shows PIDs** | Yes | Yes (with `-p`) | Yes on Linux (`-p`); on macOS with `-v` |
| **Filter syntax** | Flag-based | Built-in expressions | Flag-based |
| **Best for** | macOS; fd-level detail | Linux; speed; scripted checks | Legacy scripts; macOS (limited) |
| **Install** | Usually preinstalled | `iproute2` package | `net-tools` (Linux); preinstalled (macOS) |

## Parsing output with awk

Each tool has a different output format, so extracting PIDs requires different `awk` patterns. Here are the one-liners you'll actually use.

### Extracting PIDs from lsof

```bash
lsof -i :3000 -P -n | awk 'NR>1 {print $2}' | sort -u
```

Skip the header row (`NR>1`), print column 2 (PID), deduplicate. This is the cleanest of the three because `lsof` puts the PID in a consistent column.

### Extracting PIDs from ss

```bash
ss -tlnp 'sport = :3000' | awk 'NR>1 {print $6}' | grep -oP 'pid=\K[0-9]+'
```

Column 6 contains the `users:(("node",pid=12345,fd=22))` blob. The `grep -oP` extracts just the number after `pid=`. The `-P` flag enables Perl-compatible regex (`\K` resets the match start). If you're on a system without `grep -P`, use `sed` instead:

```bash
ss -tlnp 'sport = :3000' | sed -n 's/.*pid=\([0-9]*\).*/\1/p'
```

### Extracting PIDs from netstat (Linux)

```bash
netstat -tlnp 2>/dev/null | awk '/:3000/ {split($7,a,"/"); print a[1]}'
```

Column 7 has the `PID/program` format. `split` on `/` gives you the PID in `a[1]`. The `2>/dev/null` suppresses warnings about not having full permissions.

### Extracting PIDs from netstat (macOS)

This is the awkward one. Plain macOS `netstat` doesn't show PIDs, but `netstat -v` adds `pid` and `epid` columns:

```bash
# macOS: the pid column is field 11 on current macOS (check the header on your version)
netstat -vanp tcp | awk '/\.3000 / && /LISTEN/ {print $11}'

# Simpler: just use lsof on macOS
lsof -i :3000 -P -n -t
```

The column position depends on the macOS version, which is one of the reasons `lsof` dominates on macOS.

## Which should you use?

The decision is mostly made for you by your platform and use case:

**On macOS?** Use `lsof`. You have no other real option. `ss` doesn't exist, and macOS `netstat` only shows PIDs with `-v`. `lsof -i -P -n` is your go-to.

**On Linux and need speed?** Use `ss`. If you're writing a monitoring script that checks port status every few seconds, the netlink path is measurably faster. On a busy server with thousands of connections, `ss` is the lighter query.

**On Linux and need file descriptor detail?** Use `lsof`. If you need to know which specific file descriptor a socket is on, or you need to correlate network sockets with other open files in the same process, `lsof` gives you that level of detail.

**In a minimal container?** Use `ss` if you can add it. Alpine's busybox has `netstat` but not `ss` (install the `iproute2-ss` package), and slim Debian images often omit `iproute2` too. `lsof` and `net-tools` are often missing as well. If none is available, you can read `/proc/net/tcp` directly — it's always there on Linux.

**Writing cross-platform scripts?** This is where it gets awkward. You either need platform detection and separate code paths, or you use `lsof` everywhere (it works on both, just slower on Linux). Neither option is great.

## Or abstract the differences

[proc](https://github.com/yazeed/proc-cli) handles the platform detection for you. Same command on macOS and Linux, same output format:

```bash
proc on :3000            # What's listening on port 3000
proc on node             # What ports is node using
proc kill :3000 --yes    # Kill whatever is on port 3000
```

No flags to remember. No `awk` to extract PIDs. No platform-specific code paths. `proc on :3000` returns the process name, PID, port, command, and working directory in one call, and `--json` gives you machine-readable output if you're scripting.

## Install

```bash
brew install yazeed/proc/proc     # macOS
cargo install proc-cli            # Rust
npm install -g proc-cli           # npm/bun
```

See the [GitHub repo](https://github.com/yazeed/proc-cli) for all installation options.
