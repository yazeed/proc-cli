---
layout: page
title: proc for
description: "Find processes by file path."
permalink: /commands/for
---

Find processes running a specific file. Alias: `f`.

```bash
proc for ./server          # what's running this program (or holding it open)?
proc for /usr/bin/node     # processes using this executable
proc for ~/bin/myapp       # tilde expansion
```

A process counts when it runs the file or uses it: an open file, its working directory or root directory, or a file mapped into memory (a shared library). On Linux proc reads this from `/proc`, so nothing needs installing; macOS asks `lsof`, and FreeBSD and NetBSD `fstat`. Where that program is missing, `proc for` says so and exits 4 (`not_supported`) rather than reporting no processes. Windows finds the processes running the file only. proc leaves itself out.

One process gets the same card as `proc on`, under the line `✓ Found 1 process for ./server`: its command line, directory, path, user, parent, status, resources, start time, children, the ports it listens on and the connections to them. Several get the standard process table (the one `proc list` prints; `--verbose` adds PPID, STATUS and DIR), followed by the ports they listen on. `--json` is the same for one process or several. If the OS gives no port table (for example macOS without `lsof` on the PATH), the card and the table leave the ports out, and `--json` has no `ports` key for those processes.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--in [<dir>]` | `-i` | Filter by working directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--cmd` | | Match `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--min-cpu <n>` | | Minimum CPU % |
| `--min-mem <n>` | | Minimum memory (MB) |
| `--status <s>` | | Filter by status: running, sleeping, stopped, zombie, idle (Linux), starting (macOS, FreeBSD, NetBSD), disk-wait (Linux; `disk_wait` also works) |
| `--min-uptime <s>` | | Minimum uptime in seconds |
| `--sort <key>` | `-s` | Sort by: cpu, mem (biggest first); pid, name (ascending). Ties come by PID (default: cpu) |
| `--limit <n>` | `-n` | Limit results |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Print every value whole and every connection (default: cut to the terminal width, 5 peers); a table of several hits gains PPID, STATUS and DIR |
| `--wide` | `-w` | Print the directory and the command line whole instead of fitting the terminal |

## Examples

```bash
# what's running this program?
proc for ./server

# find processes with a data file open
proc for ./data.json

# high-CPU processes running a specific binary
proc for /usr/bin/python3 --min-cpu 10
```

## See also

[by](by) to filter by name, [in](in) to filter by directory.
