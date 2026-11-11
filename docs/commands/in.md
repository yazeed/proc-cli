---
layout: page
title: proc in
description: "The processes running in a directory or a folder below it."
permalink: /commands/in
---

`proc in ~/projects` also lists a server started in `~/projects/my-app/api`. It only reads.

```bash
proc in .                                 # everything running in this project
proc in ~/projects/my-app                 # ... in another one
proc in . --by node                       # the node processes in it
proc in . --by node --cmd                 # ... or with node anywhere in their command line
proc in ~/projects/my-app --min-cpu 10    # the busy ones
proc in . --json                          # the same list for a script
```

## Output

The process table [`proc list`](list) prints, with the directory as its second column: `PID DIR NAME USER CPU% MEM UPTIME COMMAND`. `--verbose` adds PPID and STATUS and cuts nothing.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--cmd` | | Match `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--min-cpu <n>` | | Minimum CPU % |
| `--min-mem <n>` | | Minimum memory (MB) |
| `--status <s>` | | Filter by status: running, sleeping, stopped, zombie, idle (Linux), starting (macOS, FreeBSD, NetBSD), disk-wait (Linux; `disk_wait` also works) |
| `--min-uptime <s>` | | Minimum uptime in whole seconds |
| `--parent <pid>` | | Only children of this parent PID |
| `--sort <key>` | `-s` | Sort by: cpu, mem (biggest first); pid, name (ascending). Ties come by PID (default: cpu) |
| `--limit <n>` | `-n` | Limit results |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add PPID and STATUS to the table, and cut nothing (DIR is always there) |
| `--wide` | `-w` | Print the directory and the command line whole instead of fitting the terminal |

## Details

- **A deleted directory works**, such as a removed git worktree: the processes still running there are listed, so they can be cleaned up. FreeBSD and NetBSD forget a deleted directory's path, so there it exits 4.
- **Symlinks are followed:** `/tmp/x` on macOS finds the processes the system reports in `/private/tmp/x`, and the output shows the resolved path.

## When nothing matches

`⚠ No processes found in /path`, exit 0. A path that doesn't exist and has nothing running in it is treated as a typo: `Invalid input: Directory not found`, exit 4. [Messages](messages) explains each one.

## JSON

```text
{
  "action": "in", "success": true, "count": 2,
  "processes": [<Process>, ...]
}
```

Each process has its `cwd`. Every key: [the JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#list-by-in-stuck-orphans).

## See also

[by](by) to start from a name, [list](list) for all processes, [kill](kill) `--in .` to stop only this project's processes.
