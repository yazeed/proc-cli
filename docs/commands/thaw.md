---
layout: page
title: proc thaw
description: "Resume frozen processes with SIGCONT."
permalink: /commands/thaw
---

Send SIGCONT to resume processes paused by `proc freeze`. Asks for confirmation unless `--yes`. Not available on Windows: it exits 4.

```bash
proc thaw node             # resume all frozen node processes
proc thaw :3000            # resume frozen process on port 3000
proc thaw :3000 :8080 node # resume multiple targets
```

Several targets are separate arguments. Comma-separated lists (`:3000,:8080`) still work but are deprecated.

With `--json` and `--yes`, the result lists `processes`: every process it resumed, in the order of the list you were asked to approve, each as a whole process object with its `outcome` (`succeeded` or `failed`, and `error` when it failed), and `count`, how many. `succeeded` and `failed` hold the same processes split in two, and stay.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--yes` | `-y` | Skip confirmation |
| `--dry-run` | | Show what would be resumed |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--start-time <secs>` | | Resume only if the PID still belongs to the process that started then (its `start_time` from the JSON, to the second). One PID target. A PID a new process holds exits 2 |
| `--cmd` | | Match name targets and `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add PPID, STATUS and DIR to the table of processes it lists, and cut nothing |

## Examples

```bash
# resume all frozen node processes
proc thaw node

# resume frozen process on a specific port
proc thaw :3000

# preview before resuming
proc thaw node --dry-run

# resume without confirmation
proc thaw :3000 --yes
```

## See also

[freeze](freeze) to pause processes, [stop](stop) for graceful shutdown.
