---
layout: page
title: proc orphans
description: "Find orphaned processes."
permalink: /commands/orphans
---

Find processes whose parent has exited, leaving them orphaned (reparented to PID 1). Alias: `o`.

**Not available on Windows.** Windows does not re-parent a process whose parent exited, so there is no reliable orphan to find: every form of `proc orphans`, including `--kill` and `--dry-run`, exits 4 with "not supported on Windows".

```bash
proc orphans                 # list all orphaned processes
proc orphans --kill --yes    # kill all orphans without confirmation
proc orphans --by node       # find orphaned processes named node
```

With `--kill --json --yes`, the result lists `processes`: every process it killed, in the order of the list you were asked to approve, each as a whole process object with its `outcome` (`succeeded` or `failed`, and `error` when it failed), and `count`. `succeeded` and `failed` hold the same processes split in two, and stay.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--kill` | `-k` | Kill orphaned processes |
| `--yes` | `-y` | Skip confirmation (with --kill) |
| `--dry-run` | | Show what would be killed (with --kill) |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--cmd` | | Match `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add PPID, STATUS and DIR to the table, and cut nothing |
| `--wide` | `-w` | Print the directory and the command line whole instead of fitting the terminal |

## Examples

```bash
# list all orphaned processes
proc orphans

# find orphans in the current project
proc orphans --in .

# find orphaned node processes
proc orphans --by node

# preview which orphans would be killed
proc orphans --kill --dry-run

# kill all orphans without confirmation
proc orphans --kill --yes
```

## See also

[stuck](stuck) for hung processes, [unstick](unstick) to recover stuck processes.
