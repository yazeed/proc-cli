---
layout: page
title: proc free
description: "Free a port: stop what holds it, then check it's free."
permalink: /commands/free
---

It asks each holder to exit (SIGTERM), force-kills (SIGKILL) whatever is still running after up to 5 seconds (or `--wait`, if that is shorter), then checks the port. It asks first. Ports only.

```bash
proc free :3000              # free port 3000, after asking
proc free :3000 --dry-run    # what would it stop?
proc free :3000 :8080 --yes  # two ports you've seen: no question
proc free :3000 --wait 30    # give the port up to 30 seconds to be free
```

## Output

The list of what holds the port, then the question. After a yes, a line per port: free, or `Port 3000 still in use (may be in TIME_WAIT)`, exit 1.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--yes` | `-y` | Skip confirmation |
| `--dry-run` | | Show what would be freed |
| `--wait <secs>` | | Whole seconds to wait for port availability (default: 10) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add PPID, STATUS and DIR to the table of processes it lists, and cut nothing |

## Details

- **Every holder is stopped.** A port held by several processes (one on `127.0.0.1`, one on `[::1]`) has all of them stopped, and it counts as free only once none is left, seen in two looks a quarter of a second apart: a copy that takes the port back in between is reported as still in use.
- **`--wait` covers the whole run**, the polite wait included; after a force kill the port still gets a second to be released.
- **A port still in use afterwards** usually means a child process inherited the socket, or a supervisor started the server again. `proc why :3000` shows who holds it now.
- **It asks first**, with the list of holders and their whole command lines. If the list can't be shown, nothing is asked and nothing changes (exit 4, `needs_confirmation`).
- **Ports only:** `proc free 1234` or `proc free node` exits 4. Use [`stop`](stop) or [`kill`](kill) for those.
- **Several ports** are separate arguments. Comma-separated lists (`:3000,:8080`) still work but are deprecated.

## When nothing matches

`⚠ No process listening on port 3000` and `✓ All specified ports are already free`, exit 0: a port nobody holds is already free. A port another user's process holds, when proc isn't root, is exit 3: it is in use, not free. [Messages](messages) explains each one.

## JSON

```text
{
  "action": "free", "success": true, "dry_run": true, "count": 1,
  "not_found": [], "processes": [<Process>, ...]
}

{
  "action": "free", "success": true,
  "results": [{"port": 3000, "freed": true}],
  "processes": [<Process>, ...], "count": 1, "not_found": []
}
```

The first is `--dry-run`, the second the receipt with `--yes`: `results` says for each port whether it is `freed`, and `processes` lists the holders it stopped, in the order you approved, each with its `outcome` (`succeeded`, or `failed` with `error` when it was still running afterwards). A port nobody holds is `{"action": "free", "success": true, "count": 0, "message": "All specified ports are already free", "not_found": [":3000"]}`, with or without `--dry-run`. Every key: [the JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#free).

## See also

[kill](kill) and [stop](stop) for processes by PID or name, [on](on) to see what's on a port first.
