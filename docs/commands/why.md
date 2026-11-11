---
layout: page
title: proc why
description: "Trace what started a process, or why a port is busy."
permalink: /commands/why
---

It only reads, so it never asks.

```bash
proc why :3000           # why is port 3000 busy?
proc why 1234            # where did PID 1234 come from?
proc why node            # the ancestry of each process named node
proc why :3000 1234      # several targets, one answer each
proc why :3000 --json    # the same answer for a script
```

## Output

The chain runs from the oldest ancestor down to the target, and each step shows how long that process has been running, so a long-lived parent, such as a forgotten `tmux` session, stands out. Under the chain, the target gets the same card as [`proc on`](on).

```text
$ proc why :3000
✓ Port 3000 (TCP):

? launchd [1]
    └── ● tmux [4120] new -s dev  up 2d 4h
        └── ● zsh [4121]  up 2d 4h
            └── ● node [5230] server.js  up 3h 12m  ← target

  Process:     node (PID 5230)
  Command:     node server.js
  Directory:   /Users/dev/my-app
  …
```

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--cmd` | | Match name targets as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Print every value whole and every connection (default: cut to the terminal width, 5 peers) |

## Details

- A port held by several processes gets one chain per holder.
- The card's `Resources:` needs a CPU reading taken over 0.2 seconds, so `proc why` takes about a third of a second.
- Comma-separated targets (`:3000,1234`) still work but are deprecated; write separate arguments.

## When nothing matches

A port nobody listens on is `No process listening on port 3000`, and a PID or name nothing matches is `No process found matching '…'`, both exit 2. With several targets, each missing one gets its own `⚠` line and the others are answered. [Messages](messages) explains each one.

## JSON

```text
{
  "action": "why", "success": true, "count": 1, "not_found": [],
  "results": [
    {
      "target": ":3000", "port": 3000, "protocol": "tcp",
      "process": <Process>, "ports": [<Port>, ...],
      "ancestry": [<Process>, ...]
    }
  ]
}
```

`ancestry` runs from the oldest ancestor down to the target, which has `"is_target": true`. Each entry is a whole process object, with `uptime_secs`. A port with several holders gives one result per holder, with the same `target`. Every key: [the JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#why).

## See also

[on](on) for what's on a port, [tree](tree) `--ancestors` for the chain of one process.
