---
layout: page
title: proc tree
description: "Show process tree."
permalink: /commands/tree
---

Display process hierarchy. Alias: `t`.

```bash
proc tree                  # full process tree
proc tree node             # tree rooted at node
proc tree :3000            # tree from port 3000's process
proc tree 1234 --ancestors # path up to root
```

Each line shows the process's status, name, PID, CPU, memory and uptime (`up 3m 12s`); `--json` includes `uptime_secs`. Each process in the JSON is a whole process object, the same one `list` and `on` print: every node of the tree, and every entry of the `--ancestors` chain, has `cwd`, `command`, `exe_path`, `user`, `parent_pid`, `start_time`, `parent_name`, `cpu_time_secs`, `open_files` and `child_count`, left out where the OS does not give the value.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--ancestors` | `-a` | Show path up to root instead of descendants |
| `--depth <n>` | `-d` | Max depth to display (default: 10) |
| `--compact` | `-C` | Show PIDs only |
| `--min-cpu <n>` | | Minimum CPU % |
| `--min-mem <n>` | | Minimum memory (MB) |
| `--status <s>` | | Filter by status |
| `--min-uptime <s>` | | Minimum uptime in seconds |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit). With no target, shows the processes in that directory, each with its subtree |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case). With no target, shows the processes with that name, each with its subtree |
| `--cmd` | | Match name targets and `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Show extra details |

With no target, the filters narrow the view, as in `list`: when nothing passes them, `tree` says so and exits 0. A target is a question, so a target that matches nothing, or that `--in` or `--by` rule out, exits 2, as in `by` and `info`. A process that passes the filters and is already drawn inside another one's tree is not drawn again as a root of its own.

## Examples

```bash
# show node's process tree
proc tree node

# ancestry of process on port 3000
proc tree :3000 --ancestors

# shallow tree (2 levels deep)
proc tree node --depth 2

# compact view (PIDs only)
proc tree node --compact
```

## See also

[list](list) for flat process listing, [info](info) for detailed single-process view.
