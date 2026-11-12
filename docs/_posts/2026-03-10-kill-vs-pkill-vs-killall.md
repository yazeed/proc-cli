---
title: "kill vs pkill vs killall: when to use each"
description: "kill takes a PID, pkill a pattern, killall an exact name. What each does, why pkill sometimes kills nothing (or too much), killall on macOS, and the Windows equivalents."
date: 2026-03-10
last_modified_at: 2026-11-12
---

Three commands, three different approaches to stopping processes. They look similar but behave differently in ways that matter.

## The short answer

- **`kill 12345`** signals one process by its **PID**. Nothing else.
- **`pkill node`** finds processes whose **name matches a pattern** and signals them all. `node` also matches `nodemon`.
- **`killall node`** signals every process with **exactly that name**. `node` doesn't match `nodemon`.

All three send SIGTERM unless you name another signal (`-9` for SIGKILL). Which to use:

| You have | Use | Preview first with |
|----------|-----|--------------------|
| a PID | `kill 12345` | `ps -p 12345` |
| an exact process name | `pkill -x node` or `killall node` | `pgrep -lx node` |
| part of a name or command line | `pkill -f "npm start"` | `pgrep -lf "npm start"` |
| a port | none of them: find the PID first | [find and kill a process on a port](/posts/find-kill-process-on-port-macos-linux/) |

## kill: signal by PID

`kill` sends a signal to a specific process by PID. That's it. Despite the name, it doesn't always kill — it's really a signal delivery mechanism.

```bash
kill 12345          # send SIGTERM (default)
kill -9 12345       # send SIGKILL
kill -HUP 12345     # send SIGHUP (reload config)
kill -STOP 12345    # freeze the process
kill -CONT 12345    # resume it
```

The common signals (numbers differ by OS, so use the names in scripts):

| signal | number | catchable? | use case |
|--------|--------|------------|----------|
| SIGTERM | 15 | yes | ask process to exit cleanly |
| SIGKILL | 9 | no | force kill, no cleanup |
| SIGINT | 2 | yes | same as Ctrl+C |
| SIGHUP | 1 | yes | reload config / terminal hangup |
| SIGSTOP | 17 (macOS) / 19 (Linux) | no | freeze process |
| SIGCONT | 19 (macOS) / 18 (Linux) | yes (it resumes the process anyway) | resume frozen process |

`kill` is the safest option because you're explicit about which PID gets the signal. No pattern matching, no ambiguity. The downside: you need the PID first.

```bash
# typical workflow
ps aux | grep node
# read the PID from the output
kill 12345
```

You can signal multiple PIDs:

```bash
kill 12345 12346 12347
kill -9 $(pgrep -f "webpack-dev-server")
```

## pkill: signal by pattern

`pkill` matches process names (or command lines) against a regex pattern and signals every match.

```bash
pkill node          # SIGTERM to all processes named "node"
pkill -9 webpack    # SIGKILL to all processes matching "webpack"
pkill -f "npm start" # match against full command line
```

Useful flags:

| flag | description |
|------|-------------|
| `-f` | match against full command line, not just process name |
| `-x` | exact match only (no partial matching) |
| `-u user` | only processes owned by this user |
| `-t tty` | only processes on this terminal |
| `-P pid` | only children of this parent PID |
| `-n` | only the newest matching process |
| `-o` | only the oldest matching process |

### the regex gotcha

`pkill` uses regex matching by default, and it matches **substrings**:

```bash
pkill node
```

This kills processes named `node`, but also `nodemon`, `nodecellar`, and anything else containing "node." If you want an exact match:

```bash
pkill -x node
```

The `-f` flag is powerful but dangerous. `pkill -f "server"` matches any process whose command line contains "server" — which could be a lot of things.

### pgrep: the read-only sibling

`pgrep` uses the same matching logic as `pkill` but only prints PIDs. Use it to preview what `pkill` would hit:

```bash
pgrep -l node        # list PIDs and names
pgrep -lf "npm start" # list PIDs and full commands
```

Always run `pgrep` before `pkill` if you're unsure what will match.

### pkill by PID?

`pkill` doesn't take PIDs: `pkill 12345` treats `12345` as a name pattern and matches nothing. Use `kill 12345`. What `pkill` can do with a PID is `-P`: `pkill -P 12345` signals the *children* of process 12345, which is handy when a parent (`npm run dev`) has left its workers running.

### when pkill doesn't work

`pkill` exits `0` when it signalled at least one process and `1` when nothing matched, so check `echo $?` first. If it matched nothing, or the process survived, it's usually one of these:

- **The name is longer than 15 characters (Linux).** Linux keeps only the first 15 characters of a process name, so `pkill my-long-server-name` matches nothing (newer `pkill` warns about it). Use `pkill -f my-long-server-name`, which matches the full command line.
- **The process is named after its interpreter.** `python3 manage.py runserver` is named `python3`, and `node server.js` is `node`. Match the script with `-f`: `pkill -f manage.py`.
- **It belongs to another user, or to root.** You get "Operation not permitted". Use `sudo pkill …` if you mean to.
- **It ignores SIGTERM.** Some programs catch it and keep going. Wait a few seconds, then `pkill -9 …`. A process stuck in the kernel, waiting on a disk or a network mount (state `D` in `ps`), ignores even that until the wait ends.
- **It's a zombie.** A `<defunct>` process has already exited and can't be killed; its parent has to reap it. See [zombie processes, orphans and stuck processes](/posts/zombie-orphan-stuck-processes-guide/).
- **`-f` matched more than you meant, including your own shell.** `pkill` never matches itself, but run as `ssh host 'pkill -f myapp'` or `bash -c 'pkill -f myapp'`, the shell's own command line contains `myapp`, so the shell that ran it can be killed too.

## killall: signal by exact name

`killall` matches by process name — but unlike `pkill`, it matches the **exact process name** (on Linux and macOS), not a regex substring.

```bash
killall node         # all processes exactly named "node"
killall -9 firefox   # force kill firefox
```

Useful flags:

| flag | description |
|------|-------------|
| `-w` | wait for processes to die before returning (Linux only) |
| `-I` | case-insensitive matching (Linux); on macOS, ask for confirmation before each signal |
| `-v` | report whether the signal was sent successfully |
| `-r` | use regex matching (Linux, makes it behave more like pkill; macOS uses `-m`) |

### the macOS warning

On Solaris and some System V systems, `killall` without arguments kills **all processes**. This is not a theoretical risk — it will log you out or worse. macOS and Linux both require a process name, but the reputation sticks.

More importantly, the flags differ between Linux and macOS, even though matching is exact on both by default:

```bash
killall node    # Linux: only "node", not "nodemon"
killall node    # macOS: also only "node"; -m switches to a regex
```

Linux has `-r`, `-I` (case-insensitive) and `-w`; macOS has `-m`, `-I` (confirm each signal) and no `-w`. If you write scripts that run on both platforms, `pkill -x` is more predictable.

### the -w flag (Linux)

One genuinely useful `killall` feature on Linux: the `-w` (wait) flag blocks until all matched processes have exited. Useful in scripts where you need to know the process is truly gone before continuing. macOS `killall` has no `-w`; use `proc wait node` there.

```bash
killall -w node && echo "all node processes stopped"
```

Neither `kill` nor `pkill` has this built in.

### killall by PID?

Like `pkill`, `killall` takes names, not PIDs: `killall 12345` looks for a process named `12345`. Use `kill 12345`.

### killall on macOS: the everyday uses

Most people meet `killall` on a Mac, restarting a part of the system that's misbehaving. macOS starts each of these again by itself:

```bash
# Dock or Mission Control stuck
killall Dock

# Finder frozen or not showing changes
killall Finder

# no sound, or the wrong output device
sudo killall coreaudiod

# flush the DNS cache
sudo dscacheutil -flushcache
sudo killall -HUP mDNSResponder
```

Names are case-sensitive here: `killall dock` finds nothing, because the process is called `Dock`. For a frozen app, Force Quit and Activity Monitor are usually quicker: see [how to force quit an app or kill a process on a Mac](/posts/force-quit-kill-process-mac/).

## when to use each

**Use `kill`** when you know the exact PID and want maximum control. Best for interactive use where you've already identified the process.

**Use `pkill`** when you want to match by name, command line, user, or parent. Best for scripts and automation. Always preview with `pgrep` first.

**Use `killall`** when you want exact name matching and, on Linux, the `-w` (wait) flag. Be aware of macOS/Linux differences. Avoid in cross-platform scripts.

## on Windows

Windows has none of the three. Its equivalents:

```bat
:: like kill -9 12345
taskkill /PID 12345 /F

:: like killall -9 node
taskkill /IM node.exe /F
```

```powershell
Stop-Process -Id 12345 -Force
# like killall node; no .exe here
Stop-Process -Name node -Force
```

Add `/T` to end each one's child processes too. Without `/F`, `taskkill` asks the program to close, which console programs like `node` usually ignore.

## with proc

[proc](https://github.com/yazeed/proc-cli) takes a PID, a name or a port in the same place, and shows you the list before anything is signalled:

```bash
# preview: processes named node
proc kill node --dry-run

# SIGTERM, SIGKILL after 10s
proc stop node

# node anywhere, like pkill -f
proc kill node --cmd

# a port and a name at once
proc kill :3000 webpack

# wait for exit, like killall -w
proc wait node
```

A name matches the whole name (`node`, never `nodemon`), and proc leaves out the shell and agent it runs under, so `proc kill zsh` can't end the terminal you typed it in. The same commands work on macOS, Linux and Windows. No regex surprises, and nothing is signalled until you've seen the list.

## Install

```bash
# macOS
brew install yazeed/proc/proc

# npm or bun, any OS
npm install -g proc-cli

# Rust
cargo install proc-cli
```

On Windows: `scoop bucket add proc https://github.com/yazeed/scoop-bucket-proc` and `scoop install proc`. The [install page](/install) lists every other way.
