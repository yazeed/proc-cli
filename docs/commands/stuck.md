---
layout: page
title: proc stuck
description: "Processes that look hung."
permalink: /commands/stuck
---

Busy or blocked for two seconds, and running longer than `--timeout` (5 minutes by default). It only reads, unless you add `--kill`. Alias: `x`.

```bash
proc stuck                         # what looks stuck?
proc stuck --timeout 60            # ... counting anything older than a minute
proc stuck --by node --in .        # only this project's node processes
proc stuck --kill --dry-run        # what --kill would end
proc stuck --kill                  # end them, after showing the list and asking
```

## Output

`⚠ Found 2 potentially stuck processes`, then the process table [`proc list`](list) prints.

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

## Details

- **What counts as stuck:** running longer than `--timeout`, and in two readings a second apart either using more than half a CPU core or blocked in uninterruptible sleep (waiting on the kernel, usually for I/O, where it can't even be interrupted). One busy moment isn't enough.
- **It finds candidates, not proof.** A long build or a busy server at full CPU looks the same as a loop that never ends, so check the list before `--kill`, or try [`unstick`](unstick) first.
- It takes about two seconds, for the two readings.

## When nothing matches

`✓ No stuck processes found (threshold: 300s)`, exit 0.

## JSON

```text
{
  "action": "stuck", "success": true, "count": 2,
  "processes": [<Process>, ...]
}
```

Nothing found is `{"action": "stuck", "success": true, "count": 0, "message": "No stuck processes found (threshold: 300s)"}`, with no `processes`. With `--kill --yes` it is a receipt, as for [`kill`](kill): `processes` each with its `outcome`, in the order you approved. Every key: [the JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#list-by-in-stuck-orphans).

## See also

[unstick](unstick) to try recovering them first, [kill](kill) to end one you have checked.
