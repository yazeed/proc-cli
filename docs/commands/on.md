---
layout: page
title: proc on
description: "What's on a port, or which ports a process uses."
permalink: /commands/on
---

One lookup that works both ways: a port gives the processes holding it, a PID or a name gives the ports it uses. It only reads. Alias: `:`.

```bash
proc on :3000            # what's on port 3000?
proc on 1234             # what is PID 1234, and what ports is it using?
proc on node             # what ports are the processes named node using?
proc on node --in .      # ... only the ones running in this project
proc on :3000 :8080      # several targets, as separate arguments
proc on :3000 --json     # the same answer for a script
```

## Output

Every answer is the same card for a process: its command line, directory, path, user, parent, status, resources (CPU, memory, CPU time, open files), start time and uptime, children, the ports it listens on with who can reach them, and the connections to those ports.

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

- **By port:** the headline counts the processes holding the port and, when they are all bound alike, says who can reach it (`reachable from this machine only` or `reachable from the network`). A card follows for each holder, with the connections that came in on that holder's own sockets.
- **By PID:** the headline names the process (`✓ python3 (PID 39158)`) and the card follows. `Listening:` lists every port it holds, or says `none`.
- **By name:** the headline counts the processes (`✓ 2 processes named 'node'`, or `matching` with `--cmd`) and a card follows for each.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--cmd` | | Match name targets and `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Print every value whole instead of cutting it to the terminal width, and list every connection |

## Details

- **A line is left out when the system doesn't give the value**, never printed empty. Another user's process on macOS can leave only `Process:`, `Path:` and `Children:`: macOS gives a normal user none of the rest, and proc can't see its sockets, so the card has no `Listening:`. The card then ends with `Try: sudo proc info <pid> to see the rest`.
- **A port can have several holders.** `127.0.0.1:3000` and `[::1]:3000` are different sockets, so two processes can hold one port number. `proc on :3000` shows a card for each.
- **Connections** are the TCP connections to a port the process listens on, counted by state, with the process at the other end when it is on this machine. The card lists 5 and says how many more; `-v` lists all. On FreeBSD and NetBSD proc reads no connections, so the card has no `Connections:` line there.
- **Comma-separated targets** (`:3000,:8080`) still work but are deprecated; write separate arguments.

## When nothing matches

A port nobody listens on is `No process listening on port 3000`, and a PID or name nothing matches is `No process found matching '…'`, both exit 2. With several targets, the ones found are shown and `⚠ Not found:` names the rest. A port another user's process holds, when proc isn't root, is `Port 80 is in use, but proc can't see which process holds it`, exit 3, never "No process listening"; `sudo` shows the holder. On FreeBSD and NetBSD set to hide other users' sockets (`security.bsd.see_other_uids=0`, `security.curtain=1`), proc can't tell whether such a port is in use and says so, exit 3. [Messages](messages) explains each one.

## JSON

The shape follows the question:

```text
{
  "action": "on", "query_type": "port_to_process", "success": true,
  "port": 3000, "protocol": "tcp", "address": "127.0.0.1",
  "exposure": "local", "connections": {...}, "process": <Process>,
  "processes": [<Process>, ...]
}

{
  "action": "on", "query_type": "process_to_ports",
  "success": true, "process": <Process>, "ports": [<Port>, ...]
}

{
  "action": "on", "success": true, "count": 1,
  "results": [{"process": <Process>, "ports": [<Port>, ...]}]
}
```

The first is a port, the second a PID, the third a name or several targets (each result then has its `target`, and the document has `not_found`). `processes` lists every holder of a port; `process` is the one with the lowest PID, and `address` and `exposure` (`"local"` or `"exposed"`) are its. `connections` has `total`, `by_state` and `peers` (up to 50, each with `holder_pid`, `state`, `remote_address`, `remote_port` and the `peer` process when it is on this machine), with `peers_truncated` when there are more; on each port of a PID or name it has `total` and `by_state` only. For another user's process, when proc isn't root, `ports` is `[]` because proc can't see its sockets, not because it listens on nothing. A value the system doesn't give is an absent key, never `null`. Every key: [the JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#on).

## See also

[info](info) for detailed process information, [ports](ports) to list all listening ports, [why](why) for what started it.
