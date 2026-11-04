---
layout: page
title: proc kill
description: "Force kill processes."
permalink: /commands/kill
---

Send SIGKILL to processes. Asks for confirmation unless `--yes`. Alias: `k`.

The question comes with the list of what it would kill, on the terminal: the same table as `proc list`, with the whole command line. If stdout is a file or a pipe, the list goes to stderr beside the question; if it cannot be shown at all, nothing is asked and nothing is killed (exit 4, `needs_confirmation`). A signal or `Ctrl+C` at the question ends proc with the terminal put back (130 for `Ctrl+C`).

```bash
proc kill :3000            # kill every process on port 3000
proc kill node             # kill the processes named node
proc kill :3000 :8080 node # kill multiple targets
```

Several targets are separate arguments. Comma-separated lists (`:3000,:8080`) still work but are deprecated.

A name is the whole process name, ignoring case, the way `killall` and `pkill` read it: `proc kill node` stops the processes named `node` and leaves `Claude Helper` alone. Add `--cmd` to match any part of the name or the full command line instead (like `pkill -f`): `proc kill serve --cmd`. A path is not a name: `proc kill ./server` finds nothing, so use `--cmd`, or [`proc for <file>`](for) for an executable. proc doesn't expand wildcards; an unquoted `node*` is expanded by your shell into file names; use `--cmd` for partial matches.

With `--json` and `--yes`, the result lists `processes`: every process it killed, in the order of the list you were asked to approve, each as a whole process object with its `outcome` (`succeeded` or `failed`, and `error` when it failed), and `count`, how many. `succeeded` and `failed` hold the same processes split in two, and stay.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--yes` | `-y` | Skip confirmation |
| `--dry-run` | | Show what would be killed |
| `--graceful` | `-g` | Send SIGTERM instead of SIGKILL. On Windows it asks the program to close its windows, so a console program refuses (exit 1); use [`proc stop`](stop) for a stop that escalates |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--start-time <secs>` | | Kill only if the PID still belongs to the process that started then (its `start_time` from the JSON, to the second). One PID target. A PID a new process holds exits 2 |
| `--cmd` | | Match name targets and `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Add PPID, STATUS and DIR to the table of processes it lists, and cut nothing |

## Examples

```bash
# preview before killing
proc kill node --dry-run

# kill without confirmation
proc kill :3000 --yes

# graceful shutdown (SIGTERM)
proc kill node --graceful

# only kill node in current project
proc kill node --in .

# kill multiple targets at once
proc kill :3000 :8080 node

# kill two PIDs and whatever is on port 8080
proc kill 1234 5678 :8080
```

## See also

[stop](stop) for graceful shutdown with timeout, [unstick](unstick) for stuck processes.
