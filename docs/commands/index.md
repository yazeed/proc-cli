---
layout: page
title: commands
description: "Complete reference for all proc commands."
permalink: /commands
---

## Discovery

| Command | Aliases | Description |
|---------|---------|-------------|
| [on](on) | `:` | Port/process lookup (bidirectional) |
| [why](why) | | Trace why a port is busy or show process ancestry |
| [for](for) | `f` | Find processes by file path |
| [by](by) | `b` | Filter processes by name |
| [in](in) | | Processes running in a directory or a folder below it |
| [list](list) | `l`, `ps` | List processes |
| [info](info) | `i`, `inspect` | Show detailed process information |
| [ports](ports) | `p` | List all listening ports |
| [tree](tree) | `t` | Show process tree |
| [watch](watch) | `w`, `top` | Real-time process monitoring |
| [stuck](stuck) | `x` | Find stuck/hung processes |
| [orphans](orphans) | `o` | Find orphaned processes |

## Lifecycle

| Command | Aliases | Description |
|---------|---------|-------------|
| [kill](kill) | `k` | Force kill processes (SIGKILL) |
| [stop](stop) | `s` | Graceful stop with --signal support (SIGTERM, then SIGKILL) |
| [freeze](freeze) | | Pause processes (SIGSTOP) |
| [thaw](thaw) | | Resume frozen processes (SIGCONT) |
| [free](free) | | Free a port: SIGTERM, SIGKILL after up to 5 seconds, then check the port is free |
| [wait](wait) | | Block until process(es) exit |
| [unstick](unstick) | `u` | Recover stuck processes (SIGCONT, then SIGINT, which ends most programs); `--force` ends what stays stuck |

## Common options

Most commands support these flags (check individual command pages for details):

| Flag | Short | Description |
|------|-------|-------------|
| `--json` | `-j` | Output as JSON |
| `--verbose` | `-v` | Print every value whole. In a process table it adds PPID, STATUS and DIR and cuts nothing |
| `--in [<dir>]` | `-i` | Only processes running in this directory or a folder below it (bare `--in` is the current directory) |
| `--by <name>` | `-b` | Filter by process name |
| `--help` | `-h` | Show help |

## Target syntax

Targets work the same way everywhere:

```bash
proc on :3000          # port
proc on 1234           # PID
proc on node           # name
proc kill :3000 :8080  # multiple (separate arguments)
```

Commands that take targets accept several, as separate arguments, and the kinds mix freely (`proc kill 1234 :3000 node`). A process named twice is handled once, except by the read-only `on` and `why`, which answer each target under its own header. Comma-separated targets (`:3000,:8080`) still work but are deprecated, and will be removed in a future major release. `--in` takes the next word as its directory unless that word starts with `-`, so put it after the targets or write `--in=DIR`.

A name matches the whole process name, ignoring case, the way `killall` and `pkill` read it. `--by` follows the same rule. Add `--cmd` to match any part of the name or the full command line instead, like `pkill -f`.

## Scripting

With `--json`, every command prints one JSON document with `action` (the command name) and `success`. `proc watch --json` streams one per refresh. Errors, including invalid flags, are JSON too, and every error says why in `"reason"`, one fixed word: `"system_error"` (exit 1); `"not_found"`, `"command_line_only"` or `"ruled_out"` (with `"ruled_out_by"`, the guards that ruled the process out, or `"left_out"`, the processes proc runs under that were all a name matched) for exit 2; `"permission_denied"` (exit 3); and for exit 4, which means nothing was done, `"invalid_input"` (a wrong flag, value or argument), `"needs_confirmation"` or `"not_supported"` (this OS cannot do it). A command that changes processes run with `--json` but without `--yes` exits 4 and adds `"needs_confirmation": true` and `"processes"` (the ones it would have changed); with `--yes`, the receipt lists the same processes as `"processes"`, in the same order, each with an `"outcome"` (`"succeeded"` or `"failed"`, and `"error"` when it failed), and a `"count"`. `why`, `info`, `kill`, `stop`, `freeze`, `thaw`, `unstick`, `free`, `wait`, and `on` given several targets, also put `"not_found"` in their JSON, an array of the targets that matched nothing (empty when all were found); the found ones are still handled, and when every target is missing the command exits 2 (`free` exits 0: a port nobody holds is already free).

| Exit code | Meaning |
|-----------|---------|
| `0` | Success |
| `1` | Error, or not fully successful: a process failed, a port stayed busy, `wait` timed out |
| `2` | Process or port not found |
| `3` | Permission denied: proc itself got an I/O permission error, or a `:port` has a holder proc can't see (another user's process when proc isn't root, or, on FreeBSD and NetBSD, a system that hides other users' sockets), so the port is not known to be free. A signal the OS refuses (another user's process) is a `failed` entry and exit 1 |
| `4` | Invalid input (unknown flag, bad or empty argument, invalid signal, an `--in` directory that doesn't exist), an action this OS doesn't support (`freeze`, `thaw`, `orphans` and `stop --signal` on Windows), or a command that changes processes run without `--yes` where nobody can confirm (`--json`, or no terminal): nothing was changed |
| `129`, `130`, `131`, `143` | A signal ended proc (SIGHUP, SIGINT or Ctrl+C, SIGQUIT, SIGTERM) while `watch` had the screen or a prompt was waiting: the terminal is put back first, then proc ends by that signal, so a shell shows 128 plus its number and Ctrl+C stops a script's loop. Nothing was changed by an unanswered prompt |

`"success": false` always comes with a non-zero exit code.

Every JSON shape, the values each flag accepts, what each command prints when nothing matches, and what a release may change are in the [JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md). In short: within 2.x keys are only added, never removed or given another meaning, and `error`, `message` and the human output are text for people, so branch on `reason`, `exit_code` and `count`.
