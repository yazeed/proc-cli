---
title: "The ps | grep | awk | kill pipeline, explained"
description: "Break down the classic ps aux | grep | awk | xargs kill pipeline. What each piece does, why grep -v grep, and how awk extracts PIDs."
date: 2026-03-06
---

Every Unix user eventually learns this incantation:

```bash
ps aux | grep node | grep -v grep | awk '{print $2}' | xargs kill -9
```

It works. Most people copy-paste it, tweak the process name, and move on. But each piece is doing something specific, and understanding why the pipeline looks like this makes you better at Unix in general.

Let's break it down.

## `ps aux` -- list every process

`ps` on its own shows only processes attached to your current terminal. The three flags expand that:

- **`a`** -- show processes from all users, not just yours
- **`u`** -- display in user-oriented format (adds columns like CPU%, MEM%, start time)
- **`x`** -- include processes not attached to a terminal (daemons, background jobs)

The output looks like this:

```
USER       PID  %CPU %MEM    VSZ   RSS TTY      STAT START   TIME COMMAND
root         1   0.0  0.1 169376 13292 ?        Ss   Feb28   0:10 /sbin/init
dev      48291   2.1  1.3 985432 108764 pts/1   Sl+  10:22   1:45 node server.js
dev      48350   0.0  0.0  12784  3340 pts/2    S+   10:23   0:00 vim notes.txt
```

The columns that matter for process management:

| Column | Position | What it is |
|--------|----------|------------|
| USER   | $1       | Process owner |
| PID    | $2       | Process ID (what you pass to `kill`) |
| %CPU   | $3       | CPU usage percentage |
| %MEM   | $4       | Memory usage percentage |
| STAT   | $8       | Process state (S=sleeping, R=running, Z=zombie, T=stopped) |
| COMMAND | $11+    | The full command with arguments |

Note that COMMAND is $11 and beyond -- it can contain spaces, so `awk '{print $11}'` won't always give you the full command.

## `grep node` -- filter by name

This is straightforward text matching. `grep node` keeps only lines containing the string "node". But it's blunt:

```bash
ps aux | grep node
```

This matches:

- `node server.js` -- yes, you want this
- `node worker.js` -- probably want this too
- `vim /home/dev/node_modules/express/index.js` -- probably not
- `grep node` -- definitely not (more on this next)

Grep matches against the entire line, including the COMMAND column and all its arguments. If you're looking for `python`, you'll also match `python-config`, `/usr/lib/python3.11/something`, and any process whose arguments happen to contain the word "python".

This is the fundamental fragility of `ps | grep`. You're doing substring matching on unstructured text.

## `grep -v grep` -- the self-referential problem

This is the part that confuses people the first time they see it. Run this:

```bash
ps aux | grep node
```

You'll get output like:

```
dev      48291   2.1  1.3 985432 108764 pts/1   Sl+  10:22   1:45 node server.js
dev      49102   0.0  0.0  12340  2140 pts/2    S+   10:25   0:00 grep node
```

The `grep` process itself shows up in the results, because `grep node` is a running process whose command line contains the word "node". It matched itself.

`grep -v grep` inverts the match -- it removes any line containing "grep". This filters out the grep process from its own results.

An alternative trick you'll sometimes see:

```bash
ps aux | grep [n]ode
```

The square brackets make grep search for the regex character class `[n]` followed by `ode`, which matches the string "node". But the grep process's own command line contains `[n]ode` literally, which is *not* the string "node", so it doesn't match itself. Clever, but cryptic.

## `awk '{print $2}'` -- extract the PID

`awk` splits each input line into fields by whitespace. `$1` is the first field, `$2` is the second, and so on. Since PID is the second column in `ps aux` output, `awk '{print $2}'` extracts just the process ID.

Before awk:

```
dev      48291   2.1  1.3 985432 108764 pts/1   Sl+  10:22   1:45 node server.js
```

After `awk '{print $2}'`:

```
48291
```

### Other useful awk patterns for process management

Once you understand field splitting, awk becomes a powerful filter:

```bash
# Processes using more than 50% CPU
ps aux | awk '$3 > 50.0'

# Processes using more than 1GB of RSS memory (column 6, in KB)
ps aux | awk '$6 > 1048576'

# Zombie processes (status column contains Z)
ps aux | awk '$8 ~ /Z/'

# Processes owned by a specific user
ps aux | awk '$1 == "dev"'

# Print PID and command only (cleaner output)
ps aux | awk '{print $2, $11}'

# Sum total memory usage of all node processes
ps aux | grep node | grep -v grep | awk '{sum += $6} END {print sum/1024 " MB"}'
```

These are genuinely useful, and worth keeping in your toolkit even if you use other tools day-to-day.

## `xargs kill -9` -- kill the processes

`xargs` reads items from stdin and passes them as arguments to the specified command. So if `awk` outputs:

```
48291
48350
```

Then `xargs kill -9` runs `kill -9 48291 48350`.

### Signals: -9 vs -15

`kill -9` sends SIGKILL. `kill -15` (or just `kill` with no signal) sends SIGTERM. The difference matters:

**SIGTERM (-15)** asks the process to shut down gracefully. The process can:
- Close database connections
- Flush write buffers
- Clean up temp files
- Release locks

**SIGKILL (-9)** terminates the process immediately. The kernel destroys it. The process gets no chance to clean up.

The convention in the copy-paste pipeline is `-9` because people reach for this when a process is stuck and they want it gone *now*. But in most cases, you should try `-15` first:

```bash
# Polite first
ps aux | grep node | grep -v grep | awk '{print $2}' | xargs kill -15

# Nuclear option if that didn't work
ps aux | grep node | grep -v grep | awk '{print $2}' | xargs kill -9
```

### xargs gotcha: empty input

If `awk` produces no output (no matching processes), `xargs kill -9` runs `kill -9` with no arguments, which prints an error. Use `xargs -r` on Linux to skip execution when input is empty. On macOS, `xargs` does this by default.

## Better alternatives: pgrep and pkill

The `ps | grep | awk` pipeline is so common that dedicated tools exist to replace it.

### pgrep -- find PIDs by name

```bash
pgrep node           # PIDs of processes whose name contains "node" (add -x for exactly "node")
pgrep -f "node.*server"  # Match against full command line
pgrep -u dev node    # Only processes owned by user dev
pgrep -l node        # Show PID and process name
pgrep -lf node       # Show PID and full command line (macOS and Linux; on Linux `pgrep -a` also works)
```

`pgrep` matches against the process name by default, not the full command line. This avoids the "matching too broadly" problem. Use `-f` to match against the full command if you need it.

### pkill -- find and kill in one step

```bash
pkill node           # Send SIGTERM to processes whose name contains "node" (add -x for exactly "node")
pkill -9 node        # Send SIGKILL
pkill -f "node server.js"  # Match full command line
```

`pkill` is the entire pipeline collapsed into one command. No grep-matching-itself problem, no awk, no xargs.

### kill -0 -- check if a process exists

A lesser-known trick:

```bash
kill -0 48291        # Exit 0 if process exists, exit 1 if not
```

Signal 0 is a no-op. The kernel checks whether the process exists and whether you have permission to signal it, but doesn't actually send anything. Useful in scripts:

```bash
if kill -0 "$PID" 2>/dev/null; then
    echo "Process $PID is still running"
fi
```

## Common gotchas

**Matching too broadly.** `grep python` matches everything with "python" in the command line, including editors with Python files open, Python-based tools, and scripts in `/usr/lib/python3/`. You kill things you didn't intend to.

**No confirmation.** The pipeline runs silently. There's no "are you sure?" step. If your grep was too broad, you find out by noticing that your other programs stopped working.

**Race conditions.** Between `ps` listing the PID and `kill` executing, the process could exit on its own and a new process could take that PID. This is unlikely but possible on busy systems.

**Whitespace sensitivity.** The pipeline assumes `ps aux` output has consistent whitespace formatting. This is true on modern systems, but older or non-standard `ps` implementations can break awk's field splitting.

**No tree awareness.** Killing a parent process may leave child processes orphaned. Or the parent might respawn the child immediately. The pipeline has no concept of process trees.

## Or skip the pipeline

[proc](https://github.com/yazeed/proc-cli) is a CLI tool that handles the common cases without chaining five commands together:

```bash
proc kill node              # Kill processes named "node" (confirms first)
proc kill :3000             # Kill whatever is on port 3000
proc kill node --dry-run    # Preview what would be killed
proc on node                # See what ports node is using (without killing)
```

It handles target resolution, deduplication, confirmation prompts, and signal management. The full pipeline is still there for you when you need something unusual -- understanding it is worth your time regardless of what tools you use.

## Install

```bash
brew install yazeed/proc/proc     # macOS
cargo install proc-cli            # Rust
npm install -g proc-cli           # npm/bun
```

See the [GitHub repo](https://github.com/yazeed/proc-cli) for all installation options.
