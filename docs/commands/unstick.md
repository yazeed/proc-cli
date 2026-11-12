---
layout: page
title: proc unstick
description: "Recover stuck processes with SIGCONT and SIGINT; --force ends the ones that stay stuck."
permalink: /commands/unstick
---

With no target it finds the stuck processes first, as [`proc stuck`](stuck) does. It asks first. Alias: `u`.

```bash
proc unstick --dry-run       # what would it try, on which processes?
proc unstick                 # try to recover every stuck process, after asking
proc unstick 1234            # try to recover PID 1234
proc unstick node --force    # recover the stuck node processes, or end them
proc unstick 1234 :3000      # several targets
```

## Output

`⚠ Found 2 processes to unstick:` and the process table every list in proc prints, then the question. After a yes, a line per process: `recovered`, `not stuck`, `still stuck`, `terminated` or `failed`.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--timeout <secs>` | `-t` | Without a target: only stuck processes running longer than this (default: 300) |
| `--force` | `-f` | Terminate if recovery fails |
| `--yes` | `-y` | Skip confirmation |
| `--dry-run` | | Show what would be done |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--start-time <secs>` | | Act only if the PID still belongs to the process that started then (its `start_time` from the JSON, to the second). One PID target (not with the search for stuck processes). A PID a new process holds exits 2 |
| `--cmd` | | Match name targets and `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add PPID, STATUS and DIR to the table of processes it lists, and cut nothing |

## Details

- **It can end a process without `--force`.** It sends SIGCONT (wakes a paused process) and watches for a second; a process still busy then gets SIGINT, the signal Ctrl+C sends, which ends most programs. A process that is gone afterwards counts as `terminated`.
- **With `--force`**, one still stuck 3 seconds later gets SIGTERM, and SIGKILL 5 seconds after that.
- **On Windows** there is no gentle step: without `--force` nothing is sent, and with it the process is ended.
- **A target that isn't stuck** is left alone and reported as `not stuck`.
- **Several targets** are separate arguments. Comma-separated lists (`1234,:3000`) still work but are deprecated.

## When nothing matches

With no target and nothing stuck, `✓ No stuck processes found`, exit 0. A target that matches nothing is `No process found matching '…'`, exit 2. [Messages](messages) explains each one.

## JSON

```text
{
  "action": "unstick", "success": true, "dry_run": false,
  "force": false, "count": 1, "not_found": [],
  "processes": [<Process with "outcome">, ...], "recovered_count": 0,
  "not_stuck_count": 1, "still_stuck_count": 0,
  "terminated_count": 0, "failed_count": 0
}
```

`outcome` is `recovered`, `terminated`, `still_stuck`, `not_stuck`, `would_attempt` (with `--dry-run`) or `failed: <reason>`; check `failed_count`, since a failure's `outcome` starts with `failed:`. The bare numbers `found`, `recovered`, `not_stuck`, `still_stuck`, `terminated` and `failed` are deprecated (use the `_count` names, and `count` for `found`) and go in 3.0. Every key: [the JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#unstick).

## See also

[stuck](stuck) to find stuck processes, [stop](stop) to end one politely.
