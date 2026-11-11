---
layout: page
title: proc orphans
description: "Processes whose parent has exited."
permalink: /commands/orphans
---

Often the dev servers, file watchers and headless browsers a closed terminal or a coding agent left behind. Alias: `o`. Not on Windows.

```bash
proc orphans --in .               # leftovers from this project
proc orphans --by node            # orphaned node processes
proc orphans --in . --kill --dry-run    # what --kill would end
proc orphans --in . --kill        # end them, after showing the list and asking
```

## Output

`⚠ Found 3 orphaned processes`, then the process table [`proc list`](list) prints.

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

## Details

- **It finds candidates, not leftovers.** Any process whose parent is PID 1 is listed, apart from system daemons. On macOS that includes every app launchd started (Preview, Notification Center, helper agents), so `proc orphans` with no filter lists dozens of ordinary apps. `nohup` and `tmux` jobs are orphaned on purpose too. Narrow it with `--in` (your project) or `--by`, and check each process (`proc info`, `proc why`) before `--kill`.
- **Not on Windows.** Windows doesn't re-parent a process whose parent exited, so there is no reliable orphan to find: every form of `proc orphans`, `--kill` and `--dry-run` included, exits 4 with "not supported on Windows".

## When nothing matches

`✓ No orphaned processes found`, exit 0.

## JSON

```text
{
  "action": "orphans", "success": true, "count": 3,
  "processes": [<Process>, ...]
}
```

Nothing found is `{"action": "orphans", "success": true, "count": 0, "message": "No orphaned processes found"}`, with no `processes`. With `--kill --yes` it is a receipt, as for [`kill`](kill): `processes` each with its `outcome` (`succeeded` or `failed`, and `error` when it failed), in the order you approved, and `succeeded` and `failed` split in two. Every key: [the JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#list-by-in-stuck-orphans).

## See also

[stuck](stuck) for hung processes, [kill](kill) to end one you have checked.
