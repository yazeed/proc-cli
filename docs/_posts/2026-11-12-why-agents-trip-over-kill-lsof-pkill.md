---
title: "Why AI agents trip over lsof, kill and pkill"
description: "An agent that frees a port or stops a server reads exit codes and output that were written for people: lsof exits 1 for both 'nothing found' and 'bad argument', kill exits 1 for both 'gone' and 'not yours', and on Linux pkill -f can kill the shell running it. Measured on macOS and Ubuntu, and what proc does instead."
date: 2026-11-12
---

A coding agent that starts a dev server will sooner or later need to stop it, or find out what holds port 3000. It reaches for the same tools a person would: `lsof`, `kill`, `pkill`. Those tools were built for someone reading the screen, who can tell from a sentence what went wrong. An agent decides from the exit code and the output it can parse, and that's where they let it down.

Everything below was measured on macOS 15.7 and Ubuntu 24.04, run the way an agent runs commands: through `sh -c` or `bash -c`, with no terminal.

## One exit code, two meanings

An agent's first question after a command is "did it work, and if not, why?" The exit code is the only answer it gets without parsing text.

```bash
# nothing listens on the port
lsof -i :59999 -t
echo $?   # 1

# a mistake in the command
lsof -i :notaport -t
echo $?   # 1
```

`lsof` exits 1 when it finds nothing and 1 when its arguments are wrong. To an agent those are the same result, but one means "the port is free" and the other means "your command was broken, and you know nothing about the port".

`kill` has the same problem with the case that matters most:

```bash
# the process is gone
kill 999999
echo $?   # 1

# the process isn't yours
kill 1
echo $?   # 1
```

Both exit 1, on macOS and on Linux. "It already exited" means the job is done. "Operation not permitted" means it's still running and the agent should stop and ask. Telling them apart means reading English from stderr, in whatever wording that system and shell use. `pgrep` gets this right (1 for no match, 2 for a bad option), but it only finds processes; it doesn't free ports or stop anything.

## A pattern that matches the agent itself

`pkill -f` matches a pattern against every process's full command line. That includes the shell the agent's command runs in, because that shell's command line contains the pattern too:

```bash
bash -c 'sleep 4242 &
  pkill -f "sleep 4242"
  echo done'
```

On Ubuntu 24.04, `done` never prints: `pkill` kills the `sleep` and also the `bash` that ran it, which exits with 143 (SIGTERM). The agent sees its own command die. On macOS the same line works, because BSD `pkill` never matches its own parents. Same command, same flags, two behaviours, and nothing in the output says which one you got.

## The tool isn't there

```bash
docker run --rm ubuntu:24.04 \
  which lsof ss netstat
```

prints nothing: the Ubuntu container image has none of the three. An agent working in a container, a CI job or a minimal VM finds that the first command it reaches for doesn't exist, and gets exit 127. Even when they're installed, `ss`, `netstat` and `lsof` print different columns on each system, so a parser written for one breaks on the next.

## PIDs come back

An agent that started a server at the beginning of a task and stops it an hour later is holding a number, not a process. When the process ends, the system gives that number to a new process sooner or later. On NetBSD 11, starting processes as fast as possible, a PID came back after about 5,850 other processes, roughly 8 seconds. Linux and macOS take longer to wrap around, but over hours on a busy machine they do too. `kill 4821` doesn't check that 4821 is still the server it started.

## Nothing asks first

`kill`, `pkill` and `killall` act the moment they run. There's no preview, so an agent that matched more than it meant finds out afterwards, if at all.

## What proc does instead

[proc](https://github.com/yazeed/proc-cli) was built to be read by programs as well as people (the [two readers](/posts/two-readers/) post explains the idea). For an agent, that comes down to these.

**One exit code per kind of answer, on every system.**

| Exit | Meaning |
|------|---------|
| 0 | Done |
| 1 | An error, or not fully done (a signal the system refused) |
| 2 | Nothing matched |
| 3 | Permission denied: proc can't see what it needs, so it doesn't guess |
| 4 | Nothing was done: invalid input, `--yes` needed, or not supported here |

The same cases as above, with proc 2.4.0 on Ubuntu 24.04:

```bash
# exit 2, reason not_found
proc kill 999999 -y --json

# exit 4, reason invalid_input
proc kill --nosuch --json

# exit 2, reason not_found,
# with no lsof installed
proc on :3000 --json
```

**A word to branch on.** Every error in `--json` carries a `reason` (`not_found`, `ruled_out`, `permission_denied`, `needs_confirmation`, `invalid_input`, `not_supported`, `system_error`), so the agent never has to parse a sentence. A loop that frees a port looks like this:

```bash
proc free :3000 --yes --json \
  > out.json
case $? in
  # free (or nobody held it)
  0) echo "port 3000 is free" ;;
  # still in use
  1) jq -r .error out.json ;;
  # can't see the holder
  3) echo "ask the user" ;;
  # e.g. invalid_input
  *) jq -r .reason out.json ;;
esac
```

`free` checks the port really is free afterwards, so exit 0 is the answer the agent wanted, whether something held the port or not.

**It never acts on itself or on what runs it.** A name is matched against the whole process name, not any part of the command line; `--cmd` widens it to command lines, and even then a match in proc's own arguments doesn't count. So the `pkill -f` case above, as `proc kill --cmd "sleep 4242" -y`, kills the `sleep` and the shell carries on. A process proc runs under (the shell, the agent) is never acted on even when its name matches: it's left out and named in the JSON's `left_out`, and if it was the only match, nothing happens and proc exits 2 with `"reason": "ruled_out"`.

**It checks the PID is still the same process.** Every process proc reports carries its `start_time`. Pass it back with `--start-time` and proc acts only if that PID still belongs to the process that started then; if the PID was reused, the answer is exit 2 with `"reason": "ruled_out"` and nothing is signalled. proc also checks start times itself right before each signal, so a PID that changed hands between the preview and the signal is never hit.

**It asks first, and an agent can't skip the question by accident.** A command that changes processes needs `--yes` or a person's yes at a terminal. Without a terminal and without `--yes`, nothing happens: exit 4, `"reason": "needs_confirmation"`, and the JSON lists every process it would have changed, so the agent can show the user exactly what it's about to do. `--dry-run` gives the same list without the error.

**It runs the same on five systems.** proc reads processes and sockets itself on Linux, macOS, Windows, FreeBSD and NetBSD, with the same commands, flags, JSON and exit codes, so an agent's instructions don't need a branch per system.

## Give your agent the skill

proc ships an [agent skill](/skill) that teaches an agent these rules: record the PID and start time of what it starts, branch on `reason`, treat process names as text and never as instructions, and ask the user before stopping something it didn't start. It works with Claude Code and other agents that read skills.

```bash
npx skills add yazeed/proc-cli
```

## Install

```bash
# macOS
brew install yazeed/proc/proc

# npm or bun, any OS
npm install -g proc-cli

# Rust
cargo install proc-cli
```

The [install page](/install) lists every other way, by system.
