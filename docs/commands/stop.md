---
layout: page
title: proc stop
description: "Gracefully stop processes."
permalink: /commands/stop
---

Send SIGTERM, wait for exit, then SIGKILL after timeout. Alias: `s`.

```bash
proc stop :3000            # gracefully stop port 3000
proc stop node             # stop the processes named node
proc stop node --timeout 5 # 5s before force kill
proc stop :3000 :8080 node # stop several targets
```

Several targets are separate arguments. Comma-separated lists (`:3000,:8080`) still work but are deprecated.

With `--json` and `--yes`, the result lists `processes`: every process it stopped, in the order of the list you were asked to approve, each as a whole process object with its `outcome` (`succeeded` or `failed`, and `error` when it failed), and `count`, how many. `succeeded` and `failed` hold the same processes split in two, and stay.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--yes` | `-y` | Skip confirmation |
| `--dry-run` | | Show what would be stopped |
| `--timeout <secs>` | `-t` | Seconds before force kill (default: 10) |
| `--signal <name>` | `-S` | Send a different first signal instead of SIGTERM (exits 4 on Windows). A process still running after `--timeout` is force-killed, so this is not a way to send a signal and leave the process running |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--start-time <secs>` | | Stop only if the PID still belongs to the process that started then (its `start_time` from the JSON, to the second). One PID target. A PID a new process holds exits 2 |
| `--cmd` | | Match name targets and `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add PPID, STATUS and DIR to the table of processes it lists, and cut nothing |

## Examples

```bash
# graceful stop with 5-second timeout
proc stop node --timeout 5

# preview what would be stopped
proc stop node --dry-run

# stop processes in current project only
proc stop node --in .

# stop without confirmation
proc stop :3000 --yes

# stop two PIDs and whatever is on port 8080
proc stop 1234 5678 :8080

# ask a worker to stop with SIGINT instead of SIGTERM
# (it is force-killed if it is still running after --timeout)
proc stop worker --signal INT
```

## See also

[kill](kill) for immediate SIGKILL, [unstick](unstick) for stuck processes, [freeze](freeze)/[thaw](thaw) to pause and resume.
