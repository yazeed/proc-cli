---
layout: page
title: proc watch
description: "A live process table."
permalink: /commands/watch
---

Refreshed every 2 seconds, or a stream of JSON for a script. It only reads. Aliases: `w`, `top`.

```bash
proc watch                                # every process
proc watch node --in .                    # the node processes in this project
proc watch :3000 :8080 node               # several targets together
proc watch --sort mem --limit 10 -n 1     # the 10 biggest, refreshed every second
proc watch node --json | jq '.processes[].name'   # a stream for a script
```

## Output

The table has the same columns as [`proc list`](list): PID, NAME, USER, CPU%, MEM, UPTIME and COMMAND, with the directory second under `--in`. COMMAND is cut to the width of the terminal, which watch reads again on every refresh. Press `q`, `Esc` or `Ctrl+C` to stop.

- **In a terminal**, watch takes the screen and refreshes in place, adapting to its size.
- **Piped, or without a terminal for keys,** it prints one snapshot and exits (`proc watch node | head -20`). If only stdin is missing, a `note:` on stderr says so.
- **With `--json`** it prints one JSON document per refresh, one per line. When the reader leaves (`proc watch --json | head -n1`), watch stops at once on Linux, macOS and the BSDs, and at the next refresh on Windows.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--interval <secs>` | `-n` | Refresh interval, at least 0.1 (default: 2) |
| `--sort <key>` | `-s` | Sort by: cpu, mem (biggest first); pid, name (ascending). Ties come by PID (default: cpu) |
| `--limit <n>` | `-l` | Limit results shown |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--cmd` | | Match name targets and `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--min-cpu <n>` | | Minimum CPU % |
| `--min-mem <n>` | | Minimum memory (MB) |
| `--json` | `-j` | NDJSON output (one object per refresh) |
| `--verbose` | `-v` | Add PPID, STATUS and DIR to the table, and cut nothing |
| `--wide` | `-w` | Print the directory and the command line whole instead of fitting the terminal |

## Details

- **The terminal is always put back**, also when a signal ends watch (`kill`, closing the window) or when it panics. proc then ends by that same signal, so a shell shows 130 for `Ctrl+C`, 143 for SIGTERM, 129 for SIGHUP and 131 for SIGQUIT (130 for any Ctrl+C, Ctrl+Break or closed console on Windows).
- **`--limit` is `-l` here**, because `-n` is `--interval`.
- **With `--verbose`** a row can be longer than the terminal and wrap; the row limit counts processes, not screen lines.
- **Several targets** are separate arguments. Comma-separated lists (`:3000,:8080`) still work but are deprecated.

## When nothing matches

A target that matches nothing gives an empty table, refreshed until you stop it; with `--json`, an empty document every refresh. Bound a script's stream yourself (`| head -n1`, or `timeout`).

## JSON

```text
{
  "action": "watch", "success": true, "count": 2,
  "processes": [<Process>, ...]
}
```

One per refresh, each on its own line. Every key: [the JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#watch).

## See also

[list](list) for one snapshot, [wait](wait) to block until a process exits.
