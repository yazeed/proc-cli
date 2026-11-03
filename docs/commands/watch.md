---
layout: page
title: proc watch
description: "Real-time process monitoring."
permalink: /commands/watch
---

Live-updating process table. Aliases: `w`, `top`.

```bash
proc watch                 # watch all processes
proc top                   # same thing
proc watch node            # watch node processes
proc watch :3000           # watch process on port
proc watch :3000 :8080 node  # watch several targets together
```

Several targets are separate arguments. Comma-separated lists (`:3000,:8080`) still work but are deprecated.

Press `q`, `Esc`, or `Ctrl+C` to exit. The terminal is always restored cleanly: also when a signal ends watch (`kill`, closing the window) or when it panics. proc then ends by that same signal, so a shell shows 130 for `Ctrl+C`/SIGINT, 143 for SIGTERM, 129 for SIGHUP and 131 for SIGQUIT (130 for any Ctrl+C, Ctrl+Break or closing console on Windows).

The table has the same columns as `proc list`: PID, NAME, USER, CPU%, MEM, UPTIME and COMMAND, with the directory second under `--in`. `--verbose` adds PPID, STATUS and DIR and cuts nothing. COMMAND is cut to the width of the terminal, which watch reads again on every refresh. With `--verbose` a row can be longer than the terminal and wrap in the live view; the row cap counts processes, not screen lines.

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

## Modes

**Terminal (default)**: Alternate screen with auto-refresh. Adapts to terminal size.

**JSON** (`--json`): Newline-delimited JSON, one object per refresh cycle. Pipe to `jq` or other tools. When the reader leaves (`proc watch --json | head -n1`), watch stops at once on Linux, macOS and the BSDs, and at the next refresh on Windows.

**No terminal**: When stdout is not a terminal (piped), or stdin is not (nothing to press `q` on), prints a single snapshot and exits. If only stdin is missing, a `note:` on stderr says so.

## Examples

```bash
# watch top 10 by memory, refresh every second
proc watch --sort mem --limit 10 -n 1

# watch node processes in current project
proc watch node --in .

# stream to jq
proc watch node --json | jq '.processes[].name'

# single snapshot when piped
proc watch node | head -20
```

## See also

[list](list) for a one-time snapshot, [by](by) for filtered listing.
