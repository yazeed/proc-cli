---
layout: page
title: proc why
description: "Trace why a port is busy or show process ancestry."
permalink: /commands/why
---

Explain why a port is in use or trace the ancestry of a process. Read-only — no confirmation needed.

```bash
proc why :3000     # trace why port 3000 is busy
proc why node      # show ancestry of the processes named node
proc why 1234      # show ancestry of PID 1234
proc why :3000 1234  # several targets, one answer each
```

Several targets are separate arguments. Comma-separated lists (`:3000,1234`) still work but are deprecated.

Each step of the chain shows how long that process has been running (`up 2d 4h`), so a long-lived parent such as a forgotten `tmux` session stands out; `--json` includes `uptime_secs`.

Under the chain, the target gets the same card as `proc on`: its command line, directory, path, user, parent, status, resources, start time, children, the ports it listens on and the connections to them. The chain, the `← target` line and the JSON are unchanged. The card's `Resources:` needs a CPU reading taken over 0.2 seconds, so `proc why <pid>` waits for it: about 0.32 s where the chain alone took about 0.11 s (macOS, release build). The ports and connections are read during that wait, and a port target reads the socket table once.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--cmd` | | Match name targets as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Print every value whole and every connection (default: cut to the terminal width, 5 peers) |

## Examples

```bash
# find out why port 3000 is busy
proc why :3000

# trace ancestry of the processes named node
proc why node

# trace ancestry of a specific PID
proc why 1234

# output as JSON for scripting
proc why :3000 --json
```

## See also

[on](on) for port/process lookup, [tree](tree) with `--ancestors` for full process tree.
