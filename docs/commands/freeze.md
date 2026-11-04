---
layout: page
title: proc freeze
description: "Pause processes with SIGSTOP."
permalink: /commands/freeze
---

Send SIGSTOP to pause processes. They remain in memory but stop executing. Asks for confirmation unless `--yes`. Not available on Windows, which has no SIGSTOP: it exits 4.

```bash
proc freeze node             # freeze the processes named node
proc freeze :3000            # freeze process on port 3000
proc freeze :3000 :8080 node # freeze multiple targets
```

Several targets are separate arguments. Comma-separated lists (`:3000,:8080`) still work but are deprecated.

With `--json` and `--yes`, the result lists `processes`: every process it froze, in the order of the list you were asked to approve, each as a whole process object with its `outcome` (`succeeded` or `failed`, and `error` when it failed), and `count`, how many. `succeeded` and `failed` hold the same processes split in two, and stay.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--yes` | `-y` | Skip confirmation |
| `--dry-run` | | Show what would be frozen |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--start-time <secs>` | | Freeze only if the PID still belongs to the process that started then (its `start_time` from the JSON, to the second). One PID target. A PID a new process holds exits 2 |
| `--cmd` | | Match name targets and `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add PPID, STATUS and DIR to the table of processes it lists, and cut nothing |

## Examples

```bash
# freeze all node processes
proc freeze node

# freeze process on a specific port
proc freeze :3000

# preview before freezing
proc freeze node --dry-run

# freeze processes in current project only
proc freeze node --in .

# freeze without confirmation
proc freeze :3000 --yes
```

## See also

[thaw](thaw) to resume frozen processes, [stop](stop) for graceful shutdown, [kill](kill) for immediate termination.
