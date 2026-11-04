---
layout: page
title: proc free
description: "Free ports by killing the occupying process."
permalink: /commands/free
---

Kill the process on a port and verify the port becomes available. A port held by several processes (one on `127.0.0.1`, one on `[::1]`) has all of them killed, and it counts as free only once none is left, seen in two looks a quarter of a second apart (a copy that takes the port back in between is reported as still in use). Port-only targets. Asks for confirmation unless `--yes`.

```bash
proc free :3000            # free port 3000
proc free :3000 :8080      # free multiple ports
proc free :3000 --wait 30  # wait up to 30s for port to free
```

Several ports are separate arguments. Comma-separated lists (`:3000,:8080`) still work but are deprecated.

With `--json` and `--yes`, `results` says, for each port, whether it `freed`, and `processes` lists the holders it stopped, in the order of the list you were asked to approve, each as a whole process object with its `outcome` (`succeeded`, or `failed` with `error` when the process was still running afterwards), and `count`, how many.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--yes` | `-y` | Skip confirmation |
| `--dry-run` | | Show what would be freed |
| `--wait <secs>` | | Seconds to wait for port availability (default: 10) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add PPID, STATUS and DIR to the table of processes it lists, and cut nothing |

## Examples

```bash
# free a single port
proc free :3000

# free multiple ports without confirmation
proc free :3000 :8080 --yes

# wait longer for port to become available
proc free :3000 --wait 30

# preview before freeing
proc free :3000 --dry-run
```

## See also

[kill](kill) for force kill without port verification, [stop](stop) for graceful shutdown, [on](on) to check what's on a port.
