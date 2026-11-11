---
layout: page
title: proc on
description: "Bidirectional port/process lookup."
permalink: /commands/on
---

Port/process lookup — works both ways. Alias: `:`.

```bash
proc on :3000          # what's on port 3000?
proc on node           # what ports is node using?
proc on 1234           # what is PID 1234, and what ports is it using?
proc on :3000 :8080    # multiple targets
```

Several targets are separate arguments. Comma-separated lists (`:3000,:8080`) still work but are deprecated.

Every answer is the same card for a process: its command line, directory, path, user, parent, status, resources (CPU, memory, CPU time, open files), start time and uptime, children, the ports it listens on with who can reach them, and the connections to those ports. A line is left out when the OS does not give the value (another user's process on macOS can leave only `Process:`, `Path:` and `Children:`: macOS gives a normal user none of the rest, and proc cannot see its sockets, so the card has no `Listening:`; `sudo` reads them all), never printed empty.

```text
$ proc on :53581
✓ Port 53581 is used by 1 process, reachable from this machine only

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
  Connections: 7 established
               127.0.0.1:53781 → python3 (PID 43581)
               127.0.0.1:53782 → python3 (PID 43581)
               127.0.0.1:53783 → python3 (PID 43581)
               127.0.0.1:53784 → python3 (PID 43581)
               127.0.0.1:53785 → python3 (PID 43581)
               … and 2 more (-v for all)
```

- **By port:** the headline counts the processes holding the port and, when they are all bound alike, says who can reach it (`reachable from this machine only` or `reachable from the network`). A card follows for each holder, with the connections that came in on that holder's own sockets. A port another user's process holds, when proc isn't root, is "in use, but proc can't see which process holds it", exit 3 (Linux and macOS), never "No process listening"; `sudo` shows the holder. On FreeBSD and NetBSD set to hide other users' sockets (`security.bsd.see_other_uids=0`, `security.curtain=1`), proc can't tell whether a port it finds no holder for is in use, and says so, exit 3.
- **By PID:** the headline names the process (`✓ python3 (PID 39158)`) and the card follows. `Listening:` lists every port it holds, or says `none`; for another user's process (unless proc runs as root) it is left out, since proc cannot see that process's sockets. The JSON `ports` is `[]` there, as before, and does not mean none.
- **By name:** the headline counts the processes (`✓ 2 processes named 'node'`, or `matching` with `--cmd`) and a card follows for each.

A port can be held by several processes at once (`127.0.0.1:3000` and `[::1]:3000` are different sockets). `proc on :3000` shows a card for each, and `--json` has them in `processes`, with `process` the holder with the lowest PID.

Connections are TCP connections to a port you listen on, counted by state, with the process at the other end when it is on this machine. The card lists 5 of them and says how many more; `-v` lists all. On FreeBSD and NetBSD proc reads no connections, so the card has no `Connections:` line there.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--cmd` | | Match name targets and `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Print every value whole instead of cutting it to the terminal width, and list every connection |

## Examples

```bash
# what's running on port 3000?
proc on :3000

# what ports is node using? (reverse lookup)
proc on node

# only node processes in current directory
proc on node --in .

# what is this PID?
proc on 1234

# JSON output for scripting
proc on :3000 --json
```

## JSON

`proc on :3000 --json` gains `exposure` (`"local"` or `"exposed"`, next to `address`) and `connections`: `total`, `by_state` and `peers` (up to 50, each with `holder_pid`, `state`, `remote_address`, `remote_port` and the `peer` process when it is on this machine), with `peers_truncated` when there are more. `on <pid>` and names carry `connections` with `total` and `by_state` only on each port. A value the OS does not give is an absent key, never `null`.

## See also

[info](info) for detailed process information, [ports](ports) to list all listening ports.
