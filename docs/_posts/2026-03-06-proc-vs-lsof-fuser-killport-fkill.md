---
title: "proc vs lsof vs fuser vs killport vs fkill vs procs"
description: "A practical comparison of CLI tools for process and port management on macOS and Linux."
date: 2026-03-06
---

You're developing locally. Port 3000 is in use. What do you type?

If you've been using Unix long enough, you probably know the incantation:

```bash
lsof -i :3000 -t | xargs kill -9
```

It works. But it's not exactly *obvious*. And if you need to do anything beyond "find port, kill it" — like checking what directory a process is running from, or killing multiple targets at once, or watching processes in real-time — you're back to the man pages.

**proc** is a semantic CLI for process management. One tool, one syntax, for everything you do with processes and ports.

This post compares proc with the tools you're probably already using.

## The quick comparison

| Task | proc | Traditional |
|------|------|------------|
| What's on port 3000? | `proc on :3000` | `lsof -i :3000 -P -n` |
| Kill it | `proc kill :3000` | <code markdown="0">lsof -i :3000 -t &#124; xargs kill -9</code> |
| What ports is node using? | `proc on node` | <code markdown="0">lsof -i -P -n &#124; grep node</code> |
| Find by name + directory | `proc by node --in .` | <code markdown="0">ps aux &#124; grep node</code> + manual cwd check |
| Kill multiple targets | `proc kill :3000 :8080 node` | Three separate pipelines |
| Process tree | `proc tree node` | `pstree -p $(pgrep node)` |
| What's running this file? | `proc for ./app.js` | `lsof ./app.js` or `fuser ./app.js` |
| Real-time monitoring | `proc watch node` | <code markdown="0">watch -n2 'ps aux &#124; grep node'</code> |
| JSON output | `proc on :3000 --json` | `lsof -F -i :3000` (custom format) |

## Tool-by-tool breakdown

### lsof

**What it is:** "List open files." Since everything in Unix is a file, it can find network connections too.

**Strengths:** Installed everywhere. Extremely powerful. Can find any open file descriptor.

**The friction:**

```bash
# What's on port 3000? These all mean different things:
lsof -i :3000          # Shows all connections (not just listening)
lsof -i :3000 -sTCP:LISTEN  # Listening only, but shows all columns
lsof -i :3000 -P -n   # Suppress port/host resolution (faster)
lsof -i :3000 -t      # PID only (for piping to kill)
```

Four flags to get a PID. And you need to remember which combination to use. The `-P` and `-n` flags are practically mandatory but easy to forget.

**With proc:**

```bash
proc on :3000          # Shows process name, PID, port, directory
proc kill :3000        # One command
```

**Verdict:** lsof is a Swiss Army knife. proc is a purpose-built tool. If you specifically need to find who has a file descriptor open, use lsof. For everything else process/port related, proc is clearer.

### fuser

**What it is:** Identify processes using files or sockets.

**Strengths:** Simple, fast, installed on most Linux systems.

**The friction:**

```bash
fuser 3000/tcp           # Shows PIDs (cryptic output format)
fuser -k 3000/tcp        # Kill (no confirmation!)
fuser -n tcp 3000-9000   # Range... but output is hard to parse
```

fuser's output format is notoriously hard to parse — the label (`3000/tcp:`) goes to stderr and the PIDs to stdout, with leading padding. And `fuser -k` kills without asking.

**With proc:**

```bash
proc on :3000            # Clean output with process details
proc kill :3000          # Asks for confirmation first
proc ports --range 3000-9000  # Clean table output
```

**Verdict:** fuser is fast but crude. No process details, no confirmation, awkward output. proc gives you the same information in a usable format.

### killport

**What it is:** A Rust CLI specifically for killing processes on ports.

**Strengths:** Simple. `killport 3000` just works.

**The limitations:**

```bash
killport 3000            # Kill process on port 3000
killport 3000 8080       # Kill multiple ports
# ...and that's it
```

killport does one thing: kill by port. It doesn't support:
- Finding processes by name (`proc by node`)
- Directory-based filtering (`--in .`)
- Process info without killing (`proc on :3000`)
- Process trees, monitoring, stuck process recovery
- PID targeting, JSON output

**With proc:**

```bash
proc kill :3000          # Same thing, with confirmation
proc kill :3000 --yes    # Skip confirmation (like killport)
proc on :3000            # But also: just look without killing
proc kill :3000 node     # And: mixed targets
```

**Verdict:** If all you ever do is kill ports, killport is fine. proc does ports *and* everything else.

### fkill / fkill-cli

**What it is:** A Node.js interactive process killer with fuzzy search.

**Strengths:** Beautiful TUI. Great for browsing and finding processes you don't know the name of.

**The trade-offs:**

```bash
fkill                    # Opens interactive fuzzy finder
fkill node               # Kill by name
fkill :3000              # Kill by port
```

- Requires Node.js runtime
- Interactive-first design — not ideal for scripts
- No filtering by directory, CPU, memory
- No process trees, monitoring, or info commands
- No JSON output

**With proc:**

```bash
proc kill node           # Kill by name (with confirmation table)
proc kill :3000 node     # Multi-target
proc kill node --in .    # Only in current directory
proc kill node --dry-run # Preview first
```

**Verdict:** fkill is great when you want to *browse*. proc is for when you *know* what you want. Different philosophies. Also, proc doesn't require Node.js.

### procs

**What it is:** A Rust replacement for `ps` with color output and additional columns.

**Strengths:** Beautiful output. Searchable. Configurable columns. Good `ps` replacement.

**What it doesn't do:**

```bash
procs --watch node       # Watch mode exists
procs node               # Filter by name

# But no:
# A port lookup (ports are only columns)
# Kill/stop commands
# Directory filtering (--in .)
# Multi-target operations
# Ancestry tracing (proc why) or a tree rooted at a name (proc tree node)
# Stuck process detection
# File-based lookup
```

procs is a process *viewer*. proc is a process *manager*.

**Verdict:** procs is an excellent `ps` replacement. proc covers listing (with `proc list`) *plus* the entire lifecycle: find, inspect, filter, kill, stop, recover, monitor.

## When to use what

| You want to... | Use |
|----------------|-----|
| Find who has a specific file descriptor open | `lsof` |
| Replace `ps` with better output | `procs` |
| Browse processes interactively | `fkill` |
| Manage processes and ports from the command line | `proc` |

## The philosophy

proc's design principle is that process management shouldn't require remembering flag combinations. Every command reads like English:

- `proc on :3000` — What's **on** port 3000?
- `proc by node --in .` — Processes **by** name "node" **in** current directory
- `proc for ./script.py` — Processes **for** this file
- `proc kill :3000 :8080 node` — **Kill** these targets
- `proc watch node` — **Watch** node processes

One syntax. One tool. All of process management.

## Install

```bash
brew install yazeed/proc/proc     # macOS
cargo install proc-cli            # Rust
npm install -g proc-cli           # npm/bun
```

See the [GitHub repo](https://github.com/yazeed/proc-cli) for all installation options.
