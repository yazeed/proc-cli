---
layout: page
title: proc in
description: "Filter processes by working directory."
permalink: /commands/in
---

Find processes running in a specific directory.

```bash
proc in .                  # processes in current directory
proc in ~/projects/myapp   # processes in a specific path
proc in . --by node        # processes named node in cwd
proc in . --by node --cmd  # ... or with node anywhere in their command line
```

The directory can be one that was deleted, such as a removed git worktree: processes still running there are listed. A path that doesn't exist and has nothing running in it is treated as a typo and exits 4. An existing directory is resolved first (symlinks followed), so `/tmp/x` on macOS finds processes the OS reports in `/private/tmp/x`, and the output shows the resolved path.

The table is the one `proc list` prints, with the directory as its second column: `PID DIR NAME USER CPU% MEM UPTIME COMMAND`. `--verbose` adds PPID and STATUS and cuts nothing.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--cmd` | | Match `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--min-cpu <n>` | | Minimum CPU % |
| `--min-mem <n>` | | Minimum memory (MB) |
| `--status <s>` | | Filter by status: running, sleeping, stopped, zombie, idle (Linux), starting (macOS, FreeBSD, NetBSD), disk-wait (Linux; `disk_wait` also works) |
| `--min-uptime <s>` | | Minimum uptime in seconds |
| `--parent <pid>` | | Only children of this parent PID |
| `--sort <key>` | `-s` | Sort by: cpu, mem (biggest first); pid, name (ascending). Ties come by PID (default: cpu) |
| `--limit <n>` | `-n` | Limit results |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add PPID and STATUS to the table, and cut nothing (DIR is always there) |
| `--wide` | `-w` | Print the directory and the command line whole instead of fitting the terminal |

## Examples

```bash
# everything running in current project
proc in .

# only python processes in this directory
proc in . --by python

# high-CPU processes in a project
proc in ~/Sites/myapp --min-cpu 10

# JSON output for scripting
proc in . --json
```

## See also

[by](by) to start from a name, [list](list) for all processes.
