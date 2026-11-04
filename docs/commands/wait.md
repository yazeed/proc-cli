---
layout: page
title: proc wait
description: "Wait for processes to exit."
permalink: /commands/wait
---

Block until target process(es) exit. Non-interactive — works in scripts and pipelines.

If the reader of its output leaves (`proc wait node | head -n1`), wait keeps waiting and exits with its own result: 0 when the processes exited, 1 on a timeout. Its exit code is the answer it exists to give, so it does not stop early the way `watch --json` does.

```bash
proc wait node                 # wait until the processes named node exit
proc wait :3000                # wait until port 3000's process exits
proc wait 1234                 # wait for specific PID to exit
proc wait node python          # wait for all to exit
```

Several targets are separate arguments. Comma-separated lists (`node,python`) still work but are deprecated.

With `--json`, each process in `exited` and `still_running` is a whole process object, as `list` prints it. An `exited` one carries the last reading proc had of it before it went; a `still_running` one, the reading taken when `wait` began.

The duration and count keys are `elapsed_secs`, `exited_after_secs` (in each `exited` entry) and `count`, and `targets` lists the targets as typed. Deprecated, use the new name; removed in 3.0: `elapsed_seconds`, `exited_after_seconds`, `initial_count` and `target` (every target joined by a space).

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--interval <secs>` | `-n` | Seconds between status lines (default: 5); exits are noticed within a quarter second |
| `--timeout <secs>` | `-t` | Max wait time, 0 = no timeout (default: 0). Exits 1 if processes are still running |
| `--quiet` | `-q` | Don't print the periodic still-running lines; the start line, each exit and the result still print |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--start-time <secs>` | | Wait only if the PID still belongs to the process that started then (its `start_time` from the JSON, to the second). One PID target. A PID a new process holds exits 2 |
| `--cmd` | | Match name targets and `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Show extra details |

## Examples

```bash
# wait for node processes to finish
proc wait node

# wait with 60-second timeout
proc wait node --timeout 60

# check every 10 seconds, quiet mode (for scripts)
proc wait node -n 10 -q

# wait for a port to become free
proc wait :3000

# wait for multiple targets
proc wait node python --timeout 3600

# wait for processes in current project
proc wait node --in .

# json output for scripting (exit 1 and "timed_out": true on timeout)
proc wait node --json --timeout 120 || echo "still running"
```

## See also

[watch](watch) for real-time interactive monitoring, [free](free) to kill and verify port freed, [by](by) to find processes by name.
