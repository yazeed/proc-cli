---
title: "Zombie (defunct), orphan and stuck processes: how to find and get rid of them"
description: "What zombie (defunct), orphan and stuck processes are, how to find them on Linux and macOS, why you can't kill a zombie, what \"There is 1 zombie process\" means, and how to clean up the processes an editor or agent left running."
date: 2026-03-06
last_modified_at: 2026-11-12
---

Your process will not die. You have run `kill` twice, maybe three times. Or `ps` is showing a state you do not recognize -- something with a `Z` or a `D` in the STAT column. You are not sure whether to escalate or wait.

Here is what is actually happening, and what to do about it.

## The short answer

- **A zombie (shown as `<defunct>`)** has already exited; only its exit status is left, waiting for its parent to collect it. You can't kill it. Find its parent (`ps -o ppid= -p <pid>`) and restart or stop the parent, and the zombie goes with it.
- **An orphan** is still running, but its parent has exited. It keeps holding ports and memory until you stop it like any process, once you've checked it's yours.
- **A stuck process in state `D`** is waiting on a disk, a network mount or a driver, and ignores even `kill -9` until that wait ends. Fix what it's waiting on.

| | Zombie | Orphan |
|---|---|---|
| Still running? | No, it has exited | Yes |
| Uses CPU or memory? | No, only a process-table slot | Yes |
| Its parent | Alive, but hasn't collected it | Gone; PID 1 adopted it |
| Can you kill it? | No: deal with the parent | Yes: `kill <pid>` |
| Usually means | A bug in the parent, if they pile up | Something that should have been stopped with its parent |

## Process states explained

Every process on a Unix system is in one of a handful of states at any given moment. You can see them with `ps aux`:

```
USER   PID  %CPU %MEM    VSZ   RSS  STAT START   TIME COMMAND
root     1   0.0  0.1 169312 13092 Ss   Feb28   0:08 /sbin/init
dev   4821   2.3  1.4 982340 11520 Sl   10:02   0:45 node server.js
dev   5190   0.0  0.0      0     0 Z    10:15   0:00 [build] <defunct>
root   312   0.1  0.0      0     0 D    09:58   0:12 [nfsd]
dev   6700   0.0  0.2 225880  3200 T    10:30   0:01 vim notes.txt
```

The STAT column is the one that matters here. The first character tells you the process state:

**R -- Running.** The process is on a CPU or in the run queue waiting for one. This is the normal "doing work" state.

**S -- Sleeping (interruptible).** The process is waiting for something -- a network packet, user input, a timer. This is the most common state. It can be woken by a signal. The vast majority of processes on your system are in this state right now.

**D -- Uninterruptible sleep.** The process is waiting on I/O and *cannot be interrupted*. Not by you, not by `kill -9`, not by anything except the I/O completing. More on this below.

**T -- Stopped.** The process received SIGTSTP (Ctrl+Z in a terminal) or SIGSTOP. It is frozen in place but still alive. Its memory is intact. It can be resumed.

**Z -- Zombie.** The process has finished executing. It is dead. But its entry remains in the process table because its parent has not acknowledged the exit. This is the one that confuses people most.

The second and subsequent characters in STAT are modifiers: `s` means session leader, `l` means multithreaded, `+` means foreground process group, `<` means high priority, `N` means low priority. These are informational -- the first character is the state that matters for debugging.

## Zombie processes

A zombie process is not running. It is not consuming CPU. It is not using memory (beyond a few bytes for its process table entry). It is, in every meaningful sense, dead.

So why is it still showing up in `ps`?

### How processes exit in Unix

When a process finishes, the kernel does not immediately remove it from the process table. Instead, the kernel keeps a small record: the process's PID, its exit status, and some resource usage statistics. This record exists so that the *parent process* can retrieve it.

The parent retrieves this information by calling `wait()` or `waitpid()`. This is called "reaping" the child. Until the parent calls `wait()`, the child remains in the process table as a zombie.

This is by design. The exit status of a child process is important information. If the kernel deleted it immediately, the parent would have no way to know whether its child succeeded or failed.

### How to find zombies

The direct approach:

```bash
ps aux | awk '$8 ~ /Z/'
```

This filters `ps aux` output where column 8 (the STAT column) matches the pattern `/Z/`. The `~` operator in awk means "matches regex." You will see output like:

```
dev   5190  0.0  0.0      0  0 Z+   10:15   0:00 [build] <defunct>
```

Note the `<defunct>` tag -- that is `ps` telling you this is a zombie.

For more detail, including the parent PID:

```bash
ps -eo pid,ppid,stat,comm | awk '$3 ~ /Z/'
```

```
  PID  PPID STAT COMMAND
 5190  4821 Z+   build
```

Now you know the zombie's PID (5190) and its parent's PID (4821). The parent PID is the key piece of information.

### Can you kill a zombie?

No. You cannot kill what is already dead. Running `kill -9 5190` on a zombie does nothing. The process has already exited. There is nothing left to signal.

What you need to do is get the *parent* to reap the zombie. You have two options:

**Option 1: Signal the parent to reap its children.**

```bash
kill -SIGCHLD <parent_pid>
```

SIGCHLD is the signal the kernel sends to a parent when a child exits. Sending it manually can nudge a parent that has a signal handler but has not gotten around to calling `wait()`. This works when the parent is well-behaved but slow.

**Option 2: Kill the parent.**

```bash
kill <parent_pid>
```

When the parent dies, the zombie is reparented to PID 1 (init or systemd). PID 1 is specifically designed to reap orphaned zombies. It calls `wait()` in a loop. The zombie will be cleaned up almost immediately.

This is the nuclear option, but it is sometimes the only one. If the parent is ignoring SIGCHLD or has a bug in its wait logic, no amount of signaling will fix it.

### When to worry about zombies

A handful of zombies are normal and harmless. They take up one slot in the process table (a PID) and a few bytes of kernel memory. On modern systems with PID limits in the tens of thousands, this is nothing.

*Thousands* of zombies are a problem. They indicate a parent process that is spawning children and never reaping them. Eventually this can exhaust the PID space, preventing new processes from being created. If you see zombie counts climbing, the fix is to find and fix (or restart) the buggy parent.

### "There is 1 zombie process" at login

Ubuntu prints a short system summary when you log in over SSH, and it includes a line like `There is 1 zombie process.` It's a count, not an alarm: one zombie is harmless. To see which process it is and who its parent is:

```bash
ps -eo pid,ppid,stat,comm | awk '$3 ~ /Z/'
```

If the number grows every time you log in, the parent it names isn't collecting its children: restart that parent.

## Orphan processes

An orphan is the inverse of a zombie. Where a zombie is a dead child with a living parent, an orphan is a living child whose parent has died.

### What happens when a parent exits

When a process exits, all of its children are "reparented" to PID 1. The kernel walks the exiting process's children and sets their parent PID to 1. PID 1 (init, systemd, or launchd on macOS) adopts them.

This is not inherently a problem. In fact, it is a deliberate Unix pattern.

### The double fork

Daemons use this intentionally. The classic double-fork pattern works like this:

1. Original process forks a child.
2. Child forks a grandchild.
3. Child exits immediately (becomes a zombie, quickly reaped by original process).
4. Grandchild is now orphaned, reparented to PID 1.
5. Grandchild continues running as a daemon, detached from any terminal.

This is how processes intentionally become background services. So not every process with PPID 1 is a problem.

### How to find orphans

```bash
ps -eo pid,ppid,comm | awk '$2 == 1'
```

This finds every process whose parent PID is 1. But the output will include legitimate system services, daemons, and anything launched by your init system. You need context to tell the difference.

On macOS this matters even more: `launchd`, PID 1, is the parent of every app you open, so parent PID 1 alone means almost nothing there. On one developer's Mac, 73 processes had parent PID 1, and 69 of them were apps and system services launchd had started on purpose. Narrow the search to your own work instead: processes running from your projects folder, or named like your tools.

Suspicious orphans are usually:

- Processes with names matching development tools (node, python, ruby, webpack, esbuild)
- Processes whose working directory is inside a project folder
- Processes that have been running since you last restarted a dev server

### When orphans are a problem

The most common case in development: you start a process that spawns children, then kill the parent without the children getting the signal. The children keep running, holding onto ports and consuming resources, and you have no obvious way to find them because the parent is gone.

Build tools are frequent offenders. A webpack-dev-server might spawn file watchers. A test runner might spawn worker processes. If you kill the parent with `kill -9` (which cannot be caught or forwarded), the children are orphaned.

This is why `kill -9` should be a last resort, not a reflex. `SIGTERM` gives the parent a chance to clean up its children. `SIGKILL` does not.

### An editor that quit and left its servers running

Code editors and AI coding agents start helper processes: language servers, file watchers and MCP servers, the small programs that give an AI assistant its tools. When the editor quits or crashes without stopping them, they become orphans and keep running.

One of us found six processes for the same MCP server: three `uv` launchers, each with the `python3` server it had started. One pair was 47 minutes old. The other two had been running for 19 days, left by editor sessions closed long before, and their launchers' parent was PID 1. They use little CPU, which is why nobody notices, but each holds memory and whatever files, connections or API sessions it opened, and they add up across editor restarts.

To find them, search by where they run rather than by parent PID alone:

```bash
ps -eo pid,ppid,etime,command \
  | awk '$2 == 1' \
  | grep -i -e mcp -e node -e python
```

`etime` is how long each has run: something your editor started weeks ago, with no parent, is a leftover, while one started with your current window is still in use. Check the command line names a project of yours, then stop it with `kill <pid>`. Stopping a launcher like `uv` doesn't always stop the server it started, so check the list again afterwards.

## Stuck processes (uninterruptible sleep)

The D state is the most frustrating situation you can encounter, because there is almost nothing you can do about it from userspace.

### What D state means

When a process enters uninterruptible sleep, it is waiting on an I/O operation that the kernel cannot safely interrupt. The most common case is a disk read or write that has been submitted to the hardware. The kernel has committed to completing this operation -- the process's data structures are in a state that cannot be rolled back.

This is different from interruptible sleep (S), where the kernel can wake the process with a signal, let it handle the signal, and resume. In D state, the process is in the middle of something that must complete atomically.

### How to find D-state processes

```bash
ps aux | awk '$8 == "D"'
```

Or to catch variant states like `D+` or `Ds`:

```bash
ps aux | awk '$8 ~ /D/'
```

If this returns results, you have processes stuck in I/O waits.

### Can you kill a D-state process?

Usually not. A process in uninterruptible sleep ignores most signals until its I/O finishes. The signal is queued, and it will be delivered *when the I/O completes*. If the I/O never completes, the signal is never delivered. Some waits are marked killable on Linux (since kernel 2.6.25, an NFS hard mount is one example): `ps` still shows D, but SIGKILL works. Truly uninterruptible waits do not respond to anything.

This is the one case where Unix process management often cannot help you.

### Common causes

**Stale NFS mounts.** By far the most common cause. If an NFS server goes down or the network path to it is broken, any process accessing that mount will enter D state and stay there. The fix is to resolve the NFS issue -- bring the server back, fix the network, or force-unmount the stale mount with `umount -f` or `umount -l` (lazy unmount).

**Disk hardware failure.** If the underlying storage device is failing, I/O requests may never complete. The kernel is waiting for a response from hardware that cannot respond.

**Kernel driver bugs.** A driver that never completes an I/O request will leave processes stuck in D state. This is most common with third-party filesystem drivers or storage controllers.

**FUSE filesystems.** A FUSE daemon that has crashed or hung will leave any process that was accessing the filesystem in D state, since the kernel is waiting for the FUSE daemon to respond.

### What to do

1. **Identify what the process is waiting on.** On Linux, check `/proc/<pid>/wchan` to see the kernel function it is blocked in, and `/proc/<pid>/stack` for the full kernel stack trace.
2. **Fix the underlying I/O issue.** Bring the NFS server back, unmount the stale filesystem, restart the FUSE daemon.
3. **If nothing else works, reboot.** This is sometimes the only option for D-state processes. It is not satisfying, but it is reality.

### Stopped processes (T state)

Stopped processes are much simpler. A process in T state received SIGTSTP (usually from Ctrl+Z in a terminal) or SIGSTOP (which cannot be caught). It is frozen but alive.

To resume a stopped process:

```bash
kill -CONT <pid>
```

Or, if it is the most recently stopped job in your current shell:

```bash
fg
```

Stopped processes are not stuck. They are waiting for you to tell them to continue. But if you forget about them, they sit in the process table consuming memory indefinitely.

## The systematic approach

When you suspect problem processes, here is a step-by-step method:

**Step 1: Find them.**

```bash
ps aux | awk '$8 ~ /Z|D|T/' | head -20
```

This filters for zombies, uninterruptible sleep, and stopped processes in one pass. The regex `Z|D|T` matches any of the three problem states.

**Step 2: Check the parent.**

For each problem process, find who spawned it:

```bash
ps -o ppid= -p <pid>
```

The `ppid=` syntax (with the trailing `=`) suppresses the header, giving you just the parent PID. This tells you whether the problem is in the process itself or in the process that created it.

**Step 3: Investigate what it is waiting on (Linux).**

```bash
# What kernel function is it blocked in?
cat /proc/<pid>/wchan

# What files does it have open?
ls -la /proc/<pid>/fd

# Full kernel stack trace (requires root)
cat /proc/<pid>/stack
```

On macOS, the equivalent is `sudo sample <pid>` or `sudo spindump <pid>`.

**Step 4: Act based on state.**

| State | Action |
|-------|--------|
| Z (zombie) | Signal or kill the parent. The zombie itself is already dead. |
| D (uninterruptible) | Fix the I/O issue. Usually cannot be killed. May require reboot. |
| T (stopped) | `kill -CONT <pid>` to resume, or kill it if unwanted. |

## proc's approach

[proc](https://github.com/yazeed/proc-cli) has commands for each of these. For orphans, `proc orphans` lists processes whose parent has exited; on macOS, where launchd is every app's parent, narrow it to your projects:

```bash
# orphans in your projects folder
proc orphans --in ~/projects

# only the uv launchers
proc orphans --in ~/projects --by uv
```

The table shows each one's directory, command and uptime, so you can tell an MCP server from 19 days ago from something you meant to leave running. Stop the ones that are yours by PID: `proc stop <pid>`. (`proc orphans` is Unix only: Windows doesn't re-parent processes.)

For zombies, `proc list --status zombie` lists them, and `proc why <pid>` shows the parent chain that leads to each.

For stuck processes, `proc stuck` finds processes that have run for over five minutes and are using more than 50% CPU or are blocked in uninterruptible sleep (D), and `proc unstick` attempts to recover them.

Find all stuck processes (default threshold is 5 minutes). It does not list zombies or stopped processes; use `proc list --status zombie` or `--status stopped` for those:

```bash
proc stuck
```

```
⚠ Found 1 potentially stuck process

 PID   NAME  USER   CPU%    MEM  UPTIME  COMMAND
 4821  node  dev   475.7  5.9GB  1h 12m  node /Users/dev/my-agent/node_modules/.bin/ts-node index.ts
```

Attempt recovery: SIGCONT, then SIGINT for a process still busy. SIGINT is what Ctrl+C sends, so it ends most programs:

```bash
proc unstick
```

If recovery does not work, `--force` escalates through SIGTERM and SIGKILL:

```bash
proc unstick --force
```

You can also target specific processes by PID, port, or name:

```bash
proc unstick :3000           # Unstick process on port 3000
proc unstick node --in .     # Unstick node processes in current directory
proc stuck --by webpack      # Find stuck webpack processes
```

Both commands support `--dry-run`, `--json`, and `--yes` for scripting.

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
