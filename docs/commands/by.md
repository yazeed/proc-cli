---
layout: page
title: proc by
description: "The processes with a name."
permalink: /commands/by
---

One name, matched whole and ignoring case. It only reads. Alias: `b`.

```bash
proc by node                        # processes named node
proc by node --cmd                  # ... or with node anywhere in their command line
proc by node --in .                 # node processes in this project
proc by python --min-mem 500        # python using over 500 MB
proc by worker --status sleeping    # sleeping processes named worker
proc by node --sort mem --limit 5   # the 5 node processes using the most memory
```

## Output

The process table [`proc list`](list) prints: PID, NAME, USER, CPU%, MEM, UPTIME and COMMAND. `--in` puts the directory second; `--verbose` adds PPID, STATUS and DIR and cuts nothing.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--cmd` | | Match the name as a substring of the process name or full command line (like `pkill -f`) |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--min-cpu <n>` | | Minimum CPU % |
| `--min-mem <n>` | | Minimum memory (MB) |
| `--status <s>` | | Filter by status: running, sleeping, stopped, zombie, idle (Linux), starting (macOS, FreeBSD, NetBSD), disk-wait (Linux; `disk_wait` also works) |
| `--min-uptime <s>` | | Minimum uptime in whole seconds |
| `--parent <pid>` | | Only children of this parent PID |
| `--sort <key>` | `-s` | Sort by: cpu, mem (biggest first); pid, name (ascending). Ties come by PID (default: cpu) |
| `--limit <n>` | `-n` | Limit results |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add PPID, STATUS and DIR to the table, and cut nothing |
| `--wide` | `-w` | Print the directory and the command line whole instead of fitting the terminal |

## Details

- **One name**, matched against the whole process name, ignoring case. The file name of its executable and of its first argument count too, so a long name Linux cuts to 15 characters still matches, and on Windows a trailing `.exe` is optional.
- **`--cmd`** matches any part of the name or the full command line instead, like `pkill -f`.
- **A path is not a name:** `proc by ./server` finds nothing, so use `--cmd`, or [`proc for <file>`](for) for an executable.
- **Several names:** use `proc list node python`.

## When nothing matches

A name no process has is `No process found matching 'node'`, exit 2, and when it is in some command lines the error says so: `3 processes have 'serve' in their command line (add --cmd to match those)`, then `Try: proc by serve --cmd to see them`. Every command that takes a name says it this way. A name that matches, but whose processes the filters leave out, is `⚠ No processes found by 'node'`, exit 0. [Messages](messages) explains each one.

## JSON

```text
{
  "action": "by", "success": true, "count": 6,
  "processes": [<Process>, ...]
}
```

Every key: [the JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#list-by-in-stuck-orphans).

## See also

[in](in) to start from a directory, [list](list) for all processes, [for](for) for the processes running a file.
