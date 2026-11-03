---
layout: page
title: proc stuck
description: "Find stuck/hung processes."
permalink: /commands/stuck
---

Find processes that appear stuck or hung. Alias: `x`.

```bash
proc stuck                 # find all stuck processes
proc stuck --kill          # find and kill them
proc stuck --by node       # only stuck processes named node
```

A process counts as stuck when it has been running longer than `--timeout` and, in two readings a second apart, is either using more than half a CPU core or blocked in uninterruptible sleep (waiting on the kernel, usually for I/O, where it can't even be interrupted). One busy moment isn't enough. A long build at full CPU looks the same as a loop that never ends, so check the list before using `--kill`.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--timeout <secs>` | `-t` | Only processes running longer than this (default: 300) |
| `--kill` | `-k` | Kill found stuck processes |
| `--dry-run` | | Show what would be killed |
| `--yes` | `-y` | Skip confirmation when killing |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--cmd` | | Match `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add PPID, STATUS and DIR to the table, and cut nothing |
| `--wide` | `-w` | Print the directory and the command line whole instead of fitting the terminal |

## Examples

```bash
# find stuck processes (5-minute threshold)
proc stuck

# lower threshold to 60 seconds
proc stuck --timeout 60

# find and kill stuck processes
proc stuck --kill --yes

# preview what would be killed
proc stuck --kill --dry-run

# only stuck node processes in current project
proc stuck --by node --in .
```

## See also

[unstick](unstick) for recovery attempts before killing, [kill](kill) for direct termination.
