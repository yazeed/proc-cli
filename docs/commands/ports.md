---
layout: page
title: proc ports
description: "List all listening ports."
permalink: /commands/ports
---

Show all listening ports on the system. Alias: `p`.

```bash
proc ports                 # all listening ports
proc ports --by node       # ports used by processes named node
proc ports --exposed       # network-exposed only
```

```text
 PORT   PROTO  ADDRESS    EXPOSURE  PID    NAME      UPTIME  COMMAND
 60915  TCP    127.0.0.1  local     26584  demo-api     33s  ./demo-api
 60916  TCP    0.0.0.0    exposed   26582  demo-web     33s  ./demo-web --serve --port-hint 3000
```

Each row says what the listener is. EXPOSURE is `local` (bound to loopback only) or `exposed` (all interfaces, another address, or none known). NAME, UPTIME and COMMAND are the holder's, as in every other table. COMMAND takes the width the other columns leave and ends with `…` where it is cut. A value proc cannot read shows as `-`: a process that ended after its port was listed keeps its port cells and the name its socket gave, and `-` in the rest.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--cmd` | | Match `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--in [<dir>]` | `-i` | Filter by directory |
| `--exposed` | `-e` | Only network-exposed ports (0.0.0.0, ::) |
| `--local` | `-l` | Only localhost ports (127.0.0.1, ::1) |
| `--range <start-end>` | `-r` | Filter by port range (e.g., 3000-9000) |
| `--sort <key>` | `-s` | Sort by: port, pid, name; all ascending, ties by port, then PID (default: port) |
| `--limit <n>` | `-n` | Limit results |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add USER, MEM and PATH (the executable) and cut nothing; JSON adds the connection counts |

## Examples

```bash
# all ports, sorted by process name
proc ports --sort name

# ports in the 8000-9000 range
proc ports --range 8000-9000

# only externally accessible ports
proc ports --exposed

# ports from processes in current project
proc ports --in .
```

## JSON

Each port has its listener (`port`, `protocol`, `pid`, `process_name`, `address`, `exposure`) and its holder's `command`, `user`, `uptime_secs` and `memory_mb`; a key is left out when the OS does not give it. With `--verbose` it also has `exe_path` and `connections` (`total` and `by_state`).

```json
{ "port": 3000, "protocol": "tcp", "pid": 1234, "process_name": "node", "address": "127.0.0.1",
  "exposure": "local", "command": "node server.js", "user": "zee", "uptime_secs": 34, "memory_mb": 41.5 }
```

## See also

[on](on) for looking up a specific port.
