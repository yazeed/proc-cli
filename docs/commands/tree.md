---
layout: page
title: proc tree
description: "Parents and children."
permalink: /commands/tree
---

The tree under a process, or with `--ancestors` the chain of parents above it. It only reads. Alias: `t`.

```bash
proc tree                       # every process, as a tree
proc tree node                  # the tree under each process named node
proc tree :3000                 # the tree under the process on port 3000
proc tree :3000 --ancestors     # the chain of parents above it, up to the root
proc tree node --depth 2        # two levels deep
proc tree node --compact        # PIDs only
```

## Output

Each line shows the process's status, name, PID, CPU, memory and uptime (`up 3m 12s`).

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--ancestors` | `-a` | Show path up to root instead of descendants |
| `--depth <n>` | `-d` | Max depth to display (default: 10) |
| `--compact` | `-C` | Show PIDs only |
| `--min-cpu <n>` | | Minimum CPU % |
| `--min-mem <n>` | | Minimum memory (MB) |
| `--status <s>` | | Filter by status: running, sleeping, stopped, zombie, idle (Linux), starting (macOS, FreeBSD, NetBSD), disk-wait (Linux; `disk_wait` also works) |
| `--min-uptime <s>` | | Minimum uptime in whole seconds |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit). With no target, shows the processes in that directory, each with its subtree |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case). With no target, shows the processes with that name, each with its subtree |
| `--cmd` | | Match name targets and `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Accepted, as on every command; the tree looks the same with it |

## Details

- **With no target, the filters narrow the view**, as in `list`. A process that passes them and is already drawn inside another one's tree is not drawn again as a root of its own.
- **`--compact`** changes only what you see; the JSON is the same. `--depth` cuts the JSON too.

## When nothing matches

With no target, `⚠ No processes match the specified filters`, exit 0. A target is a question, so a target that matches nothing, or that `--in` or `--by` rule out, is exit 2, as in `by` and `info`. `--ancestors` needs a target (exit 4 without one). [Messages](messages) explains each one.

## JSON

```text
{"action": "tree", "success": true, "tree": [<Process with "children": [...]>, ...]}

{
  "action": "tree", "success": true,
  "ancestry": [
    {
      "target_pid": 1234, "target_name": "node", "depth": 2,
      "chain": [<Process>, ...]
    }
  ]
}
```

The second is `--ancestors`, with `chain` from the top ancestor down to the target. Every node and every chain entry is a whole process object, the same one `list` and `on` print, with `"runs_proc": true` on a process proc runs under. Every key: [the JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#tree).

## See also

[why](why) for what started a process, with its card, [list](list) for a flat table.
