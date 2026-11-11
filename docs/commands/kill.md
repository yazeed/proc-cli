---
layout: page
title: proc kill
description: "End processes now, with SIGKILL."
permalink: /commands/kill
---

SIGKILL can't be caught, so the process gets no chance to clean up. It asks first, showing what it would kill. Alias: `k`.

```bash
proc kill :3000              # everything on port 3000, after asking
proc kill node --dry-run     # what would it kill?
proc kill node --in .        # only the node processes in this project
proc kill 1234 5678 :8080    # two PIDs and whatever holds port 8080
proc kill :3000 --yes        # a port you've seen: no question
proc kill node --graceful    # SIGTERM instead (or use proc stop)
```

## Output

The list of what it would kill, then `Kill 2 processes? [y/N]`. After a yes, `✓ Killed 2 processes` with a `→ node [PID 1234]` line for each, or `✗ Failed to kill …` with the reason for each process the system refused.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--yes` | `-y` | Skip confirmation |
| `--dry-run` | | Show what would be killed |
| `--graceful` | `-g` | Send SIGTERM instead of SIGKILL. On Windows it asks the program to close its windows, so a console program refuses (exit 1); use [`proc stop`](stop) for a stop that escalates |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--start-time <secs>` | | Kill only if the PID still belongs to the process that started then (its `start_time` from the JSON, to the second). One PID target. A PID a new process holds exits 2 |
| `--cmd` | | Match name targets and `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add PPID, STATUS and DIR to the table of processes it lists, and cut nothing |

## Details

- **It asks first.** The question comes with the list of what it would touch: the same table as `proc list`, with the whole command line, on the terminal (on stderr when stdout is a file or a pipe). If the list can't be shown, nothing is asked and nothing changes (exit 4, `needs_confirmation`). `Ctrl+C` at the question ends proc with the terminal put back (exit 130).
- **A name is the whole process name**, ignoring case: `node` leaves `Claude Helper` alone. `--cmd` matches any part of the name or the command line, like `pkill -f`; preview it with `--dry-run`. A path is not a name: use [`proc for <file>`](for).
- **Several targets** are separate arguments. Comma-separated lists (`:3000,:8080`) still work but are deprecated.
- **For a polite stop**, [`proc stop`](stop) sends SIGTERM and forces it only after a timeout.
- **Child processes are not signalled**: a server's workers may keep running, and keep its port. [`proc free`](free) checks the port afterwards.

## When nothing matches

`No process found matching '…'` (or `No process listening on port …`), exit 2, and nothing is touched. With several targets, the ones found are handled and `⚠ Not found:` names the rest. A name that matches only the shell or agent running proc is left out and named (`⚠ Left out: zsh (PID 81340), which runs this proc`); when that was every match, it is exit 2. [Messages](messages) explains each one.

## JSON

```text
{
  "action": "kill", "success": true, "dry_run": true, "count": 1,
  "not_found": [], "processes": [<Process>, ...]
}

{
  "action": "kill", "success": true, "count": 1,
  "processes": [<Process>, ...], "succeeded": [...], "failed": [...],
  "succeeded_count": 1, "failed_count": 0, "not_found": []
}
```

The first is `--dry-run`, the second the receipt with `--yes`: `processes` lists every process it killed, in the order of the list you were asked to approve, each as a whole process object with its `outcome` (`succeeded` or `failed`, and `error` when it failed). `succeeded` and `failed` hold the same processes split in two. Without `--yes`, `--json` is not a yes: exit 4, `reason: "needs_confirmation"`, with the `processes` it would have killed. Every key: [the JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#kill-stop-freeze-thaw-orphans---kill-stuck---kill).

## See also

[stop](stop) for a polite stop with a timeout, [free](free) to free a port and check it, [unstick](unstick) for stuck processes.
