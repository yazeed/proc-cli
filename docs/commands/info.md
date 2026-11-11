---
layout: page
title: proc info
description: "Everything about one process."
permalink: /commands/info
---

The same card as [`proc on`](on), for each process you name. It only reads. Aliases: `i`, `inspect`.

```bash
proc info 1234              # everything about PID 1234
proc info :3000             # ... about the process on port 3000
proc info node              # ... about each process named node
proc info :3000 1234 node   # several targets
proc info node --json       # the same for a script
```

## Output

Each process gets the same card as [`proc on`](on): the headline names it, and the card has its command line, directory, path, user, parent, status, resources (CPU, memory, CPU time, open files), start time and uptime, children, and the ports it listens on (`Listening:`, or `none`). A process that listens shows `Connections:` too, a count by state and then 5 peers.

```text
$ proc info 39158
✓ python3 (PID 39158)

  Process:     python3 (PID 39158)
  Command:     python3 -m http.server 0 --bind 127.0.0.1
  Directory:   /private/tmp/…/srv
  Path:        /Users/dev/.local/bin/python3
  User:        dev
  Parent:      zsh (PID 38120)
  Status:      running
  Resources:   0.0% CPU, 26.8MB, 0s CPU time, 9 open files
  Started:     21:14:03 (up 13s)
  Children:    none
  Listening:   :53581 TCP on 127.0.0.1 (this machine only)
  Connections: 1 established
               127.0.0.1:53781 → python3 (PID 43581)
```

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--start-time <secs>` | | Show it only if the PID still belongs to the process that started then (its `start_time` from the JSON, to the second). One PID target. A PID a new process holds exits 2, so this checks a PID you recorded |
| `--cmd` | | Match name targets and `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Print every value whole and every connection (default: cut to the terminal width, 5 peers) |

## Details

- **A line is left out when the system doesn't give the value**, never printed empty. For another user's process (`proc info 1` as a user on macOS) the card can be as short as `Process:`, `Path:` and `Children:`, because macOS gives a normal user no command line, directory, user, parent, status, resources or start time for it, and proc can't see that process's sockets. The card then ends with `Try: sudo proc info 1 to see the rest`.
- **No port table** (for example macOS without `lsof` on the PATH) leaves out `Listening:` and `Connections:`; the rest is as usual.
- **Several targets** are separate arguments. Comma-separated lists (`:3000,:8080`) still work but are deprecated.

## When nothing matches

`No process found matching '…'` (or `No process listening on port …`), exit 2. With several targets, the ones found are shown and `⚠ Not found:` names the rest. [Messages](messages) explains each one.

## JSON

```text
{
  "action": "info", "success": true, "count": 1,
  "processes": [<Process>, ...], "not_found": [], "found_count": 1,
  "not_found_count": 0
}
```

Each process has `ports`: the ports it listens on, each with `exposure` and `connections` (`total` and `by_state`, no peers), or `[]`. `ports` is absent, not `[]`, when the system gives no port table, and for another user's process when proc isn't root, since an empty list would claim a `none` proc can't back. With every target missing, the document stays, with `count: 0` and `reason: "not_found"`, exit 2. `found_count` is deprecated (use `count`) and goes in 3.0. Every key: [the JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#info).

## See also

[on](on) for a port or a process's ports, [why](why) for what started it, [list](list) for a table.
