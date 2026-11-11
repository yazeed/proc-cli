---
layout: page
title: proc stop
description: "Ask processes to exit, and force them after a timeout."
permalink: /commands/stop
---

SIGTERM first; whatever is still running after `--timeout` (10 seconds by default) gets SIGKILL. It asks first, showing what it would stop. Alias: `s`.

```bash
proc stop :3000                    # stop what's on port 3000, after asking
proc stop node --dry-run           # what would it stop?
proc stop node --in .              # only the node processes in this project
proc stop node --timeout 5         # force-kill after 5 seconds, not 10
proc stop worker --signal INT      # ask with SIGINT instead of SIGTERM
proc stop 1234 5678 :8080          # two PIDs and whatever holds port 8080
```

## Output

The list of what it would stop, then `Stop 2 processes? [y/N]`. After a yes, `✓ Stopped 2 processes` with a `→ node [PID 1234]` line for each, or the reason for each process the system refused.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--yes` | `-y` | Skip confirmation |
| `--dry-run` | | Show what would be stopped |
| `--timeout <secs>` | `-t` | Whole seconds before force kill (default: 10) |
| `--signal <name>` | `-S` | Send a different first signal instead of SIGTERM: HUP, INT, QUIT, ABRT, KILL, TERM, STOP, CONT, USR1 or USR2, in any case, with or without `SIG` (numbers aren't accepted; exits 4 on Windows). A process still running after `--timeout` is force-killed, so this is not a way to send a signal and leave the process running |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--start-time <secs>` | | Stop only if the PID still belongs to the process that started then (its `start_time` from the JSON, to the second). One PID target. A PID a new process holds exits 2 |
| `--cmd` | | Match name targets and `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add PPID, STATUS and DIR to the table of processes it lists, and cut nothing |

## Details

- **It asks first.** The question comes with the list of what it would touch: the same table as `proc list`, with the whole command line, on the terminal (on stderr when stdout is a file or a pipe). If the list can't be shown, nothing is asked and nothing changes (exit 4, `needs_confirmation`). `Ctrl+C` at the question ends proc with the terminal put back (exit 130).
- **A name is the whole process name**, ignoring case: `node` leaves `Claude Helper` alone. `--cmd` matches any part of the name or the command line, like `pkill -f`; preview it with `--dry-run`. A path is not a name: use [`proc for <file>`](for).
- **Several targets** are separate arguments. Comma-separated lists (`:3000,:8080`) still work but are deprecated.
- **All targets are asked at once and share the timeout**, so stopping ten processes takes as long as the slowest, not ten times as long.
- **Child processes are not signalled**: a server's workers may keep running, and keep its port. [`proc free`](free) checks the port afterwards.

## When nothing matches

`No process found matching '…'` (or `No process listening on port …`), exit 2, and nothing is touched. With several targets, the ones found are handled and `⚠ Not found:` names the rest. A name that matches only the shell or agent running proc is left out and named (`⚠ Left out: zsh (PID 81340), which runs this proc`); when that was every match, it is exit 2. [Messages](messages) explains each one.

## JSON

```text
{
  "action": "stop", "success": true, "dry_run": true, "count": 1,
  "not_found": [], "processes": [<Process>, ...]
}

{
  "action": "stop", "success": true, "count": 1,
  "processes": [<Process>, ...], "succeeded": [...], "failed": [...],
  "succeeded_count": 1, "failed_count": 0, "not_found": []
}
```

The first is `--dry-run`, the second the receipt with `--yes`: `processes` lists every process it stopped, in the order of the list you were asked to approve, each as a whole process object with its `outcome` (`succeeded` or `failed`, and `error` when it failed). `succeeded` and `failed` hold the same processes split in two. Without `--yes`, `--json` is not a yes: exit 4, `reason: "needs_confirmation"`, with the `processes` it would have stopped. Every key: [the JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#kill-stop-freeze-thaw-orphans---kill-stuck---kill).

## See also

[kill](kill) for an immediate SIGKILL, [free](free) to free a port and check it, [freeze](freeze) to pause instead.
