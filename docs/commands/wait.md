---
layout: page
title: proc wait
description: "Block until processes exit."
permalink: /commands/wait
---

It never asks and never changes anything, so it works in scripts and pipelines.

```bash
proc wait 1234                         # until PID 1234 exits
proc wait :3000                        # until whatever holds port 3000 exits
proc wait node python --timeout 3600   # until every node and python process exits, or an hour
proc wait node -n 10 -q                # check every 10 seconds, no status lines
proc wait node --in .                  # only this project's node processes
proc wait node --json --timeout 120 || echo "still running"
```

## Output

`~ Waiting for 2 processes to exit...`, a line each time one exits, a still-running line every `--interval` seconds (5 by default), and the result. Exit 0 when they all exited, 1 when `--timeout` ran out first (`Operation timed out: after 120s — 1 still running: node [1234]`).

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--interval <secs>` | `-n` | Whole seconds between status lines (default: 5); exits are noticed within a quarter second |
| `--timeout <secs>` | `-t` | Max wait time in whole seconds, 0 = no timeout (default: 0). Exits 1 if processes are still running |
| `--quiet` | `-q` | Don't print the periodic still-running lines; the start line, each exit and the result still print |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--start-time <secs>` | | Wait only if the PID still belongs to the process that started then (its `start_time` from the JSON, to the second). One PID target. A PID a new process holds exits 2 |
| `--cmd` | | Match name targets and `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add each process's CPU and memory to the list of what it waits for |

## Details

- **Set a `--timeout` in scripts**: the default, 0, waits forever.
- **It keeps waiting if its reader leaves** (`proc wait node | head -n1`), and exits with its own result: the exit code is the answer it exists to give.
- **Several targets** are separate arguments, and it waits for all of them. Comma-separated lists (`node,python`) still work but are deprecated.

## When nothing matches

`No process found matching '…'`, exit 2: there is nothing to wait for. With several targets, it waits for the ones found and `⚠ Not found:` names the rest. [Messages](messages) explains each one.

## JSON

```text
{
  "action": "wait", "success": true, "timed_out": false,
  "count": 1, "targets": ["1234"], "elapsed_secs": 3,
  "exited": [<Process with "exited_after_secs">, ...], "still_running": [], "not_found": []
}
```

Each process in `exited` and `still_running` is a whole process object, as `list` prints it: an `exited` one carries the last reading proc had of it, a `still_running` one the reading taken when `wait` began. On a timeout, `success` is `false`, `timed_out` is `true` and the exit code is 1. Deprecated, use the new name (removed in 3.0): `elapsed_seconds`, `exited_after_seconds`, `initial_count` and `target`. Every key: [the JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#wait).

## See also

[watch](watch) for a live table, [free](free) to free a port and check it, [stop](stop) to make a process exit.
