---
layout: page
title: proc info
description: "Show detailed process information."
permalink: /commands/info
---

Detailed information about specific processes. Aliases: `i`, `inspect`.

```bash
proc info :3000            # info for process on port 3000
proc info 1234             # info for PID
proc info node             # info for the processes named node
proc info :3000 :8080      # multiple targets
```

Several targets are separate arguments. Comma-separated lists (`:3000,:8080`) still work but are deprecated.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--start-time <secs>` | | Show it only if the PID still belongs to the process that started then (its `start_time` from the JSON, to the second). One PID target. A PID a new process holds exits 2, so this checks a PID you recorded |
| `--cmd` | | Match name targets and `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Print every value whole and every connection (default: cut to the terminal width, 5 peers) |

## Output

Each process gets the same card as `proc on`: the headline names it (`✓ python3 (PID 39158)`), and the card follows with its command line, directory, path, user, parent, status, resources (CPU, memory, CPU time, open files), start time and uptime, children, and the ports it listens on (`Listening:`, or `none`). A process that listens shows `Connections:` too, a count by state and then 5 peers; a process that listens on nothing has no `Connections:` line. A line is left out when the OS does not give the value, never printed empty: for another user's process (`proc info 1` as a user on macOS) the card can be as short as `Process:`, `Path:` and `Children:`, because macOS gives a normal user no command line, directory, user, parent, status, resources or start time for it, and proc cannot see that process's sockets. Run it with `sudo` to read them all. `-v` prints every value whole and every connection.

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

With `--json`, each process in `processes` has `ports`: the ports it listens on, each with `exposure` and `connections` (`total` and `by_state`, no peers), or an empty list. The keys it had are unchanged. `count` is the number of processes found (`found_count` is the same number, deprecated, use `count`; removed in 3.0). If the OS gives no port table (for example macOS without `lsof` on the PATH), the card has no `Listening:` or `Connections:` line and the JSON has no `ports` key; the rest is as usual. The same holds for another user's process when proc is not root: its sockets are out of sight, so an empty list would claim a `none` proc cannot back.

## Examples

```bash
# what's on port 3000? (detailed)
proc info :3000

# info for multiple targets at once
proc info :3000 1234 node

# JSON for scripting
proc info node --json
```

## See also

[on](on) for quick port lookup, [list](list) for tabular overview.
