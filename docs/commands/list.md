---
layout: page
title: proc list
description: "List processes with filters and sorting."
permalink: /commands/list
---

List all processes, with optional filters. Aliases: `l`, `ps`.

```bash
proc list                      # all processes
proc list node                 # only the processes named node
proc list node --cmd           # ... or with node anywhere in their command line
proc list node python          # several names
proc list --sort cpu --limit 10  # top 10 by CPU
```

Several names are separate arguments. Comma-separated lists (`node,python`) still work but are deprecated.

```
✓ Found 2 processes

 PID    NAME     USER  CPU%     MEM  UPTIME  COMMAND
 39158  python3  dev    0.0  26.8MB     13s  python3 -m http.server 0 --bind 127.0.0.1
 38120  zsh      dev    0.0   4.1MB  2h 14m  -zsh
```

Every process table in proc (`list`, `by`, `in`, `for` with several hits, `orphans`, `stuck`, `watch`, and the list in front of a `--dry-run` or a question) has these columns: PID, NAME, USER, CPU%, MEM, UPTIME and COMMAND. UPTIME is there at any terminal width. A value proc cannot read shows as `-`. With `--in`, the directory is the second column (`PID DIR NAME ...`). `--verbose` keeps the table and adds PPID, STATUS and the directory (DIR) to it, and cuts nothing; `--json` has all of it, and for each process also `parent_name`, `child_count`, `cpu_time_secs` and `open_files` (a key is left out where the OS does not give the value), and `"runs_proc": true` on a process proc runs under (its shell, or the agent that ran it), absent on every other.

One process is always one line. COMMAND takes the width the other columns leave and ends with `…` where it is cut. Every other column keeps its width, except the directory under `--in`, which is cut in the middle (`/Users/…/my-app/.next/standalone`) before it takes more than a third of the line. `--verbose` and `--wide` print everything whole, even when the line is longer than the terminal. The list in front of a `--dry-run` or a question prints the command line whole, since that is what you approve.

The status is `running`, `sleeping`, `stopped`, `zombie`, `dead` or `unknown`, and three more where the OS tells them apart: `idle` (Linux: an idle kernel thread), `starting` (macOS, FreeBSD and NetBSD: a process still being created by `fork`) and `disk wait` (Linux: uninterruptible sleep on the disk, shown as `disk_wait` in JSON and spelled `disk-wait` or `disk_wait` for `--status`). Before 2.3.0 these three were all `unknown`.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--cmd` | | Match the names as a substring of the process name or full command line (like `pkill -f`) |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--path <path>` | `-p` | Filter by executable path |
| `--min-cpu <n>` | | Minimum CPU % |
| `--min-mem <n>` | | Minimum memory (MB) |
| `--status <s>` | | Filter by status: running, sleeping, stopped, zombie, idle (Linux), starting (macOS, FreeBSD, NetBSD), disk-wait (Linux; `disk_wait` also works) |
| `--min-uptime <s>` | | Minimum uptime in seconds |
| `--parent <pid>` | | Only children of this parent PID |
| `--sort <key>` | `-s` | Sort by: cpu, mem (biggest first); pid, name (ascending). Ties come by PID (default: cpu) |
| `--limit <n>` | `-n` | Limit results |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add PPID, STATUS and DIR to the table, and cut nothing |
| `--wide` | `-w` | Print the directory and the command line whole instead of fitting the terminal |

## Examples

```bash
# top 10 processes by CPU
proc list --sort cpu --limit 10

# node and python processes
proc list node python

# high-memory processes in current project
proc list --in . --min-mem 100

# running processes sorted by memory
proc list --status running --sort mem
```

## See also

[by](by) and [in](in) for targeted filtering, [watch](watch) for real-time monitoring.
