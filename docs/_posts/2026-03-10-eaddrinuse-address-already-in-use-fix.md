---
title: "Address already in use (EADDRINUSE): what it means and how to fix it"
description: "What \"address already in use\" means in Node, Python, Go, Java, Docker, Postgres, ollama and on Windows, how to find what holds the port and free it, and why a port stays busy after the process is gone."
date: 2026-03-10
last_modified_at: 2026-11-12
---

You start your dev server and get this:

```
Error: listen EADDRINUSE: address already in use :::3000
```

In Python:

```
OSError: [Errno 48] Address already in use
```

In Go:

```
listen tcp :3000: bind: address already in use
```

Different languages, same problem. Something is already bound to that port. Here's what's actually happening and how to fix it.

## The short answer

1. **Find what holds the port:** `lsof -i :3000 -sTCP:LISTEN` on macOS and Linux, `netstat -ano | findstr :3000` on Windows.
2. **If it's yours** (an old run of your server, a forgotten terminal tab), stop it: `kill <PID>`, or `taskkill /PID <PID> /F` on Windows.
3. **If it's a service** (a database, Docker, ollama), don't kill it: a manager will restart it. Stop it through its manager, or start your app on another port.
4. **If nothing holds it**, wait a minute: the port is in TIME_WAIT (below).

## The same error in every stack

Each runtime words it differently, but every one of these is the same `bind()` failure:

| Stack | What you see |
|-------|--------------|
| Node.js | `Error: listen EADDRINUSE: address already in use :::3000` |
| Python (Linux) | `OSError: [Errno 98] Address already in use` |
| Python (macOS) | `OSError: [Errno 48] Address already in use` |
| Go | `listen tcp :3000: bind: address already in use` |
| Java | `java.net.BindException: Address already in use` |
| Spring Boot | `Web server failed to start. Port 8080 was already in use.` |
| Ruby (Puma, Rails) | `Address already in use - bind(2) for "127.0.0.1" port 3000 (Errno::EADDRINUSE)` |
| Docker | `Bind for 0.0.0.0:5432 failed: port is already allocated` |
| PostgreSQL | `could not bind IPv4 address "127.0.0.1": Address already in use` |
| ollama | `Error: listen tcp 127.0.0.1:11434: bind: address already in use` |
| Angular CLI | `Port 4200 is already in use. Use '--port' to specify a different port.` |
| netcat | `nc: Address already in use` |
| Windows (any program) | `Only one usage of each socket address (protocol/network address/port) is normally permitted` |

The number differs by system: `EADDRINUSE` is 98 on Linux, 48 on macOS, and 10048 (`WSAEADDRINUSE`) on Windows.

## what EADDRINUSE means

When a process wants to accept network connections, it calls `bind()` to claim a port. The kernel maintains a table of which ports are bound to which sockets. If another socket already holds that port, `bind()` fails with `EADDRINUSE` — "address already in use."

The "address" here is the combination of IP + port. Whether `0.0.0.0:3000` (all interfaces) conflicts with `127.0.0.1:3000` (localhost only) depends on the system:

- **Linux:** it conflicts. While anything listens on `127.0.0.1:3000`, binding `0.0.0.0:3000` fails with `EADDRINUSE`.
- **macOS:** it doesn't, when the program sets `SO_REUSEADDR`, which Node, Go and Rust do by default. Both binds succeed, and connections to `127.0.0.1:3000` keep reaching the **older**, more specific listener. So a new server can start without any error while `localhost:3000` still serves the old one. If your changes "don't show up", run `lsof -i :3000 -sTCP:LISTEN` and look for two listeners.

We checked both: on macOS 15 the second bind succeeded and a connection to `127.0.0.1` reached the first server; on Linux the same test failed with `Errno 98`.

## find what's using the port

### lsof (macOS and Linux)

```bash
lsof -i :3000 -P -n
```

`-P` prevents port name lookups, `-n` prevents hostname lookups. Both make it faster.

```
COMMAND   PID  USER   FD   TYPE  DEVICE SIZE/OFF NODE NAME
node    12345  user   22u  IPv6  0x1234      0t0  TCP *:3000 (LISTEN)
```

### ss (Linux)

```bash
ss -tlnp | grep :3000
```

```
LISTEN  0  128  *:3000  *:*  users:(("node",pid=12345,fd=22))
```

### netstat (everywhere)

```bash
# Linux
netstat -tlnp 2>/dev/null | grep :3000

# macOS (-v adds the PID)
netstat -anv | grep '\.3000 '

# Windows (the PID is the last column)
netstat -ano | findstr :3000
```

### fuser (Linux)

```bash
fuser 3000/tcp
```

Returns just the PID. Add `-k` to kill it (careful — no confirmation).

## kill the process

Once you have the PID:

```bash
# SIGTERM: ask it to exit
kill 12345

# SIGKILL: force it (last resort)
kill -9 12345
```

Or combine find and kill:

```bash
lsof -ti :3000 | xargs kill
```

The `-t` flag makes lsof output only PIDs, which you pipe to `kill`.

## the phantom port: nothing shows up

Sometimes `lsof` shows nothing on the port, but you still get EADDRINUSE. Check for a listener on another address (`lsof -i :3000` shows all of them), a process owned by another user (`sudo lsof -i :3000`), or, if your code does not set `SO_REUSEADDR`, TIME_WAIT.

### what is TIME_WAIT?

When a TCP connection closes, the side that initiates the close enters TIME_WAIT state. The socket stays in the kernel's table for 2× the Maximum Segment Lifetime (MSL) — typically 60 seconds on Linux, 30 seconds on macOS.

Why? To handle late-arriving packets from the other end. If the kernel released the port immediately, a new process could bind to it and receive stale packets from the old connection.

You can see TIME_WAIT sockets:

```bash
ss -tan | grep :3000     # Linux
netstat -an | grep 3000  # macOS
```

If you see lines with `TIME_WAIT` but nothing in `LISTEN`, that's your answer. Wait a minute and try again.

### SO_REUSEADDR

If waiting isn't an option — like during development where you're constantly restarting — the fix is `SO_REUSEADDR`. This socket option tells the kernel "let me bind to this port even if there are TIME_WAIT sockets on it."

In Node, there is nothing to set: Node's `net` module sets `SO_REUSEADDR` for you on Unix, so an EADDRINUSE from Node is a real listener, not TIME_WAIT. Go's `net.Listen` does the same. If you're hitting TIME_WAIT issues, you may be using a lower-level API that doesn't.

In Python:

```python
import socket
s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(('', 3000))
```

In Go:

```go
listener, err := net.Listen("tcp", ":3000")
// Go sets SO_REUSEADDR by default
```

`SO_REUSEADDR` does **not** let two processes bind to the same port simultaneously. It only allows reuse when existing sockets are in TIME_WAIT. This is safe for development and generally safe for production.

## crashed processes

If your server crashes (segfault, `kill -9`, power loss), it doesn't get a chance to call `close()` on its sockets. The kernel eventually cleans up, but there's a window where the port appears stuck.

With `kill -9` specifically, the kernel closes file descriptors immediately when the process exits, so the port should free up fast. But if child processes inherited the socket (common with forking servers), the port stays busy until **all** processes holding that file descriptor exit.

Check for inherited sockets:

```bash
lsof -i :3000 -P -n
```

If you see multiple PIDs, the parent is gone but children are still holding the port.

## fixes by stack

Most of the time the holder is your own server from an earlier run, and stopping it is the fix. These are the cases where it isn't:

- **Docker: `port is already allocated`.** Another container publishes that port, or a process on the host holds it. `docker ps` shows which container (the PORTS column); stop it with `docker stop <name>`, or publish yours on another host port (`-p 5433:5432`). If no container has it, the host process does: find it with `lsof`.
- **PostgreSQL on 5432, MySQL on 3306, Redis on 6379.** Usually a copy installed as a service (Homebrew, Postgres.app, systemd) is already running. A killed service comes back, so stop it with `brew services stop <name>` or `sudo systemctl stop <name>`, or point your app at the one that's running.
- **ollama on 11434.** The ollama desktop app, or its system service on Linux, already runs a server, so `ollama serve` has nothing to do: just use it. To run your own instead, quit the app (or `sudo systemctl stop ollama`), or set `OLLAMA_HOST=127.0.0.1:11435`.
- **Spring Boot on 8080.** Another app server, a second instance in your IDE, or Jenkins. Stop it, or set `server.port=8081` in `application.properties`.
- **Rails: `A server is already running`.** That one isn't a port error: Rails found `tmp/pids/server.pid`. If no server is running, delete the file. If Puma then says `Errno::EADDRINUSE`, a server really holds the port.
- **Angular (4200), Vite (5173).** Angular reports the port and stops (newer versions offer to use another). Vite moves to the next free port by itself unless `strictPort` is set, so check which port it printed.
- **Port 5000 on a Mac.** The macOS AirPlay Receiver listens there. See [ports that are usually taken](/posts/find-kill-process-on-port-macos-linux/#ports-that-are-usually-taken).

## a note on port 0

If you don't care which port you get, bind to port 0. The kernel assigns an available ephemeral port. Useful for tests:

```javascript
const server = app.listen(0, () => {
  console.log(`Listening on port ${server.address().port}`);
});
```

## with proc

If you have [proc](https://github.com/yazeed/proc-cli) installed, `proc on :3000` shows what's on the port (the process, its command, its directory and who's connected), and `proc free :3000` frees it: it lists what it will stop and asks first, gives each holder 5 seconds to exit before forcing it, and checks the port is free afterwards.

```bash
# what's using this port?
proc on :3000

# stop what holds it and check the port is free
proc free :3000

# several ports at once
proc on :3000 :8080 :5432
```

The same commands work on macOS, Linux and Windows. When the port is held by another user's process, proc says so (`Port 80 is in use, but proc can't see which process holds it`) instead of calling the port free.

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
