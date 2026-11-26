---
title: "nohup vs disown vs &: running background processes"
description: "Why closing your terminal kills background processes, and three ways to prevent it: &, nohup, and disown. Plus setsid, tmux, and when to use each."
date: 2026-03-10
---

You start a long-running task, close your laptop, and come back to find the process is gone. Or you SSH into a server, start a build, your connection drops, and the build dies with it.

The problem is SIGHUP. Here's what's happening and three ways to deal with it.

## why closing the terminal kills your process

When a terminal closes, the kernel sends SIGHUP (signal hangup) to the session leader — usually your shell. The shell then forwards SIGHUP to all its child processes. The default action for SIGHUP is to terminate.

This is a chain reaction:

```
terminal closes
  → kernel sends SIGHUP to shell
    → shell sends SIGHUP to all its jobs
      → child processes terminate
```

SSH disconnects trigger the same chain. The pseudo-terminal on the remote end closes, SIGHUP propagates, and your processes die.

## & (background operator)

```bash
./long-task.sh &
```

The `&` operator moves a command to the background. Your shell gets control back immediately, and you can keep typing commands.

But the process is still a child of your shell. Still in the same session. When the terminal closes:

```
terminal closes → SIGHUP → shell → SIGHUP → long-task.sh dies
```

`&` gives you your prompt back. It does **not** protect the process from terminal close.

You can check background jobs with `jobs`:

```bash
./task1.sh &
./task2.sh &
jobs
# [1]  Running    ./task1.sh
# [2]  Running    ./task2.sh
```

## nohup: ignore SIGHUP

```bash
nohup ./long-task.sh &
```

`nohup` does two things:

1. Sets the SIGHUP disposition to "ignore" for the child process
2. Redirects stdout and stderr to `nohup.out` (if not already redirected)

Now when the terminal closes:

```
terminal closes → SIGHUP → shell → SIGHUP → long-task.sh ignores it
```

The process survives. Its output goes to `nohup.out` in the current directory (or `$HOME/nohup.out` if the current directory isn't writable).

### nohup gotchas

**The nohup.out file grows forever.** If your process produces a lot of output, `nohup.out` will fill your disk. Redirect explicitly:

```bash
nohup ./long-task.sh > /dev/null 2>&1 &     # discard output
nohup ./long-task.sh > task.log 2>&1 &       # log to specific file
```

**nohup doesn't background.** You still need `&`. Without it, `nohup` runs in the foreground and you can't use your terminal.

**nohup doesn't detach from the session.** The process is still in the same session and process group. Some edge cases can still reach it.

## disown: remove from shell's job table

```bash
./long-task.sh &
disown
```

`disown` is a bash builtin (also in zsh and ksh). It removes the most recent background job from the shell's internal job table. Since the shell only sends SIGHUP to jobs it knows about, the disowned process is safe:

```
terminal closes → SIGHUP → shell → iterates job table → task not there → task survives
```

`disown` works **after** the fact. You can start a process, realize it's going to take hours, and disown it:

```bash
./big-build.sh           # started in foreground
# Ctrl+Z to suspend
bg                        # resume in background
disown %1                 # remove from job table
```

### disown variants

```bash
disown %1          # disown job 1
disown %2 %3       # disown specific jobs
disown -a           # disown all jobs
disown -h %1       # don't remove, but don't send SIGHUP on exit
```

The `-h` flag is a softer version — the job stays in your `jobs` list but won't receive SIGHUP when the shell exits.

### disown vs nohup

The key difference: `nohup` makes the process itself immune to SIGHUP (the process ignores the signal). `disown` prevents the shell from sending SIGHUP in the first place (the signal is never sent).

Both achieve the same result but through different mechanisms. `nohup` is more robust — if something other than your shell sends SIGHUP, `nohup` still protects the process. `disown` only prevents your shell from sending it.

## belt and suspenders

```bash
nohup ./long-task.sh > task.log 2>&1 &
disown
```

Use both. `nohup` ignores the signal, `disown` prevents it from being sent. Covers all bases.

## setsid: full detachment (Linux)

macOS has no `setsid` command; there, use `nohup` plus `disown`, or tmux.

```bash
setsid ./long-task.sh > task.log 2>&1
```

`setsid` creates a new session. The process becomes a session leader with no controlling terminal. SIGHUP from your terminal cannot reach it — it's in a completely separate session.

This is the most thorough disconnection. Daemons use this technique to fully detach from the terminal that started them.

## when to use each

| situation | use |
|-----------|-----|
| "I need my terminal back" | `&` |
| "I'm about to start a long task" | `nohup cmd &` |
| "I already started it and forgot nohup" | `Ctrl+Z`, `bg`, `disown` |
| "I'm writing a daemon" | `setsid` |
| "I need to reconnect later and see output" | `tmux` or `screen` |

## tmux and screen: the better way

If you need to **reconnect** to the process later and see its output, `nohup` and `disown` won't help — they disconnect you permanently. Use a terminal multiplexer:

```bash
tmux new -s build        # start named session
./long-task.sh           # run your task
# Ctrl+B, then D         # detach from session
# close terminal, go home, SSH back in
tmux attach -t build     # reattach and see output
```

The process runs inside tmux's virtual terminal, which survives your SSH disconnect. When you reattach, you see the full output as if you never left.

For one-off commands where you don't need to reconnect, `nohup` is simpler. For interactive sessions you want to resume, use tmux.

## with proc-cli

If you've lost track of background processes, `proc list` shows every process on the machine. `proc by node --in .` narrows it to a specific project directory. And `proc tree` shows parent-child relationships to find orphaned background processes.

```bash
proc list --sort cpu           # what's running?
proc by node --in .            # node processes in this project
proc tree 12345                # see the process tree
```

## Install

```bash
brew install yazeed/proc/proc     # macOS
cargo install proc-cli            # Rust
npm install -g proc-cli           # npm/bun
```

See the [GitHub repo](https://github.com/yazeed/proc-cli) for all installation options.
