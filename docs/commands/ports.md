---
layout: page
title: proc ports
description: "Every listening TCP port, and who can reach it."
permalink: /commands/ports
---

Each row names the process holding the port. It only reads. Alias: `p`.

```bash
proc ports                      # every listening port
proc ports --by node            # the ports the node processes hold
proc ports --exposed            # only the ones open to the network
proc ports --range 8000-9000    # ports 8000 to 9000
proc ports --in .               # ports held by processes in this project
proc ports --sort name          # by the holder's name
```

## Output

```text
 PORT   PROTO  ADDRESS    EXPOSURE  PID    NAME      UPTIME  COMMAND
 60915  TCP    127.0.0.1  local     26584  demo-api     33s  ./demo-api
 60916  TCP    0.0.0.0    exposed   26582  demo-web     33s  ./demo-web --serve --port-hint 3000
```

Each row says what the listener is. EXPOSURE is `local` (bound to loopback only) or `exposed` (all interfaces, another address, or none known). NAME, UPTIME and COMMAND are the holder's, as in every other table, and COMMAND ends with `…` where it is cut. A value proc can't read shows as `-`.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--cmd` | | Match `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--in [<dir>]` | `-i` | Filter by directory |
| `--exposed` | `-e` | Only network-exposed ports (0.0.0.0, ::) |
| `--local` | `-l` | Only localhost ports (127.0.0.1, ::1) |
| `--range <start-end>` | `-r` | Filter by port range (e.g., 3000-9000; ports go from 1 to 65535) |
| `--sort <key>` | `-s` | Sort by: port, pid, name; all ascending, ties by port, then PID (default: port) |
| `--limit <n>` | `-n` | Limit results |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add USER, MEM and PATH (the executable) and cut nothing; JSON adds `exe_path` and the connection counts |

## Details

- **Other users' ports.** When proc isn't root it can't see other users' processes' sockets, so their ports are left out, and on Linux and macOS a line after the table names them: `⚠ 2 ports in use by other users' processes, which proc can't see: 22, 80 (run with sudo to see them)`. The line is left out with `--by`, `--in`, `--exposed` or `--local`, since proc can't tell whether a holder it can't see would match; `--range` applies to it.
- **FreeBSD and NetBSD** set to hide other users' sockets show only your own ports, and a line after the table says so.
- **Only TCP listeners** appear; a UDP socket isn't listed.

## When nothing matches

`⚠ No listening ports found`, exit 0. [Messages](messages) explains each message.

## JSON

```text
{
  "action": "ports", "success": true, "count": 1,
  "ports": [<Port>, ...], "unseen_ports": []
}
```

Each port:

```text
{
  "port": 3000, "protocol": "tcp", "pid": 1234,
  "process_name": "node", "address": "127.0.0.1",
  "exposure": "local", "command": "node server.js", "user": "dev",
  "uptime_secs": 34, "memory_mb": 41.5
}
```

Each port has its listener and its holder's `command`, `user`, `uptime_secs` and `memory_mb`, a key left out when the system doesn't give it. `--verbose` adds `exe_path` and `connections` (`total` and `by_state`). `unseen_ports` lists the ports other users' processes hold (`[]` for none); it is absent on Windows and the BSDs, which aren't checked, and with `--by`, `--in`, `--exposed` or `--local`. On FreeBSD and NetBSD set to hide other users' sockets, `hidden_by` names the setting (`"security.bsd.see_other_uids=0"`, `"security.curtain=1"`). Every key: [the JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#ports).

## See also

[on](on) for one port, [free](free) to free one.
