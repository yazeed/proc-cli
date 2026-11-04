---
layout: page
title: proc unstick
description: "Attempt to recover stuck processes."
permalink: /commands/unstick
---

Try to recover stuck processes before resorting to killing. Alias: `u`.

Sends SIGCONT then SIGINT recovery sequence. Use `--force` to terminate if recovery fails.

```bash
proc unstick               # recover all stuck processes
proc unstick node          # recover stuck node processes
proc unstick 1234 :3000    # recover several targets
proc unstick --force       # terminate if recovery fails
```

Several targets are separate arguments. Comma-separated lists (`1234,:3000`) still work but are deprecated.

With `--json`, each process in `processes` is a whole process object (its `outcome` is beside the usual fields), as `list` prints it. `count` is how many were found, and `recovered_count`, `not_stuck_count`, `still_stuck_count`, `terminated_count` and `failed_count` count the outcomes. The bare numbers `found`, `recovered`, `not_stuck`, `still_stuck`, `terminated` and `failed` are deprecated, use the `_count` names (and `count` for `found`); removed in 3.0.

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--timeout <secs>` | `-t` | Without a target: only stuck processes running longer than this (default: 300) |
| `--force` | `-f` | Terminate if recovery fails |
| `--yes` | `-y` | Skip confirmation |
| `--dry-run` | | Show what would be done |
| `--in [<dir>]` | `-i` | Filter by directory. Takes the next word as its directory unless it starts with `-` (bare `--in` is the current directory, `--in=DIR` is explicit) |
| `--by <name>` | `-b` | Filter by process name (the whole name, ignoring case) |
| `--start-time <secs>` | | Act only if the PID still belongs to the process that started then (its `start_time` from the JSON, to the second). One PID target (not with the search for stuck processes). A PID a new process holds exits 2 |
| `--cmd` | | Match name targets and `--by` as a substring of the process name or full command line (like `pkill -f`) |
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Show extra details |

## Examples

```bash
# try to recover all stuck processes
proc unstick

# recover specific process
proc unstick 1234

# force kill if recovery fails
proc unstick node --force

# preview actions without executing
proc unstick --dry-run
```

## See also

[stuck](stuck) for finding stuck processes, [stop](stop) for graceful shutdown.
