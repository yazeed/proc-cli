---
layout: page
title: proc for
description: "The processes running a program or holding a file open."
permalink: /commands/for
---

A process counts when it runs the file or has it open. It only reads. Alias: `f`.

```bash
proc for ./server                        # what's running this program, or holding it open?
proc for /usr/bin/node                   # the processes using this executable
proc for ./data.json                     # what has this file open?
proc for /usr/bin/python3 --min-cpu 10   # busy processes running it
```

## Output

One process gets the same card as [`proc on`](on), under the line `✓ Found 1 process for ./server`: its command line, directory, path, user, parent, status, resources, start time, children, the ports it listens on and the connections to them. Several get the process table [`proc list`](list) prints, followed by the ports they listen on.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--in [<dir>]` | `-i` | Filter by working directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--cmd` | | Match `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--min-cpu <n>` | | Minimum CPU % |
| `--min-mem <n>` | | Minimum memory (MB) |
| `--status <s>` | | Filter by status: running, sleeping, stopped, zombie, idle (Linux), starting (macOS, FreeBSD, NetBSD), disk-wait (Linux; `disk_wait` also works) |
| `--min-uptime <s>` | | Minimum uptime in whole seconds |
| `--sort <key>` | `-s` | Sort by: cpu, mem (biggest first); pid, name (ascending). Ties come by PID (default: cpu) |
| `--limit <n>` | `-n` | Limit results |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Print every value whole and every connection (default: cut to the terminal width, 5 peers); a table of several hits gains PPID, STATUS and DIR |
| `--wide` | `-w` | Print the directory and the command line whole instead of fitting the terminal |

## Details

- **A process counts** when it runs the file or uses it: an open file, its working directory or root directory, or a file mapped into memory (a shared library).
- **Where proc reads it:** on Linux from `/proc`, so nothing needs installing; macOS asks `lsof`, and FreeBSD and NetBSD `fstat`. Where that program is missing, `proc for` says so and exits 4 (`not_supported`) rather than reporting no processes. Windows finds the processes running the file only.
- proc leaves itself out.
- If the system gives no port table (for example macOS without `lsof` on the PATH), the card and the table leave the ports out.

## When nothing matches

A file nothing runs or holds open is `No process found matching '<file>'`, exit 2. A path that doesn't exist is `Invalid input: File not found`, exit 4. [Messages](messages) explains each one.

## JSON

```text
{
  "action": "for", "success": true, "count": 2,
  "results": [{"process": <Process>, "ports": [<Port>, ...]}, ...]
}
```

The same for one process or several. `ports` is absent, not `[]`, when the system gives no port table. Every key: [the JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#for).

## See also

[by](by) to start from a name, [in](in) to start from a directory, [on](on) for what's on a port.
