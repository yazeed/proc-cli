---
title: "How to kill a process in Linux: by PID, by name, and when it won't die"
description: "Kill a process in Linux from the terminal (kill, pkill, killall) or the desktop, kill a whole process tree, stop one that keeps coming back, and what to do when kill -9 doesn't work."
date: 2026-11-12
---

A program hangs, a script runs away, or something holds the port your server needs. Linux gives you several ways to stop it. The right one depends on what you know about it (its PID, its name, its port) and on what started it.

## The short answer

```bash
# find the PID
pgrep -a firefox

# ask it to exit (SIGTERM)
kill 12345

# still running? force it (SIGKILL)
kill -9 12345
```

To stop every process with that name at once: `pkill -x firefox`. If it comes straight back, a service manager is restarting it: `sudo systemctl stop <service>` instead.

## Find the process

You need its PID, the number that identifies it. Any of these finds it:

```bash
# PID and full command line
pgrep -a firefox

# PIDs only, exact name
pidof firefox

# everything, filtered
ps aux | grep -i firefox
```

If you only know it's slow, sort by CPU: `top` (press `Shift+P` to sort by CPU, `q` to quit), or `ps aux --sort=-%cpu | head`. If it holds a port, `sudo ss -tlnp 'sport = :3000'` names it; see [find and kill a process on a port](/posts/find-kill-process-on-port-macos-linux/).

## Kill it by PID

```bash
# SIGTERM: please exit
kill 12345

# SIGKILL: exit now
kill -9 12345
```

`kill` sends **SIGTERM** by default, which asks the program to shut down: it can save files, close connections and remove its lock files. **SIGKILL** (`-9`) can't be caught or ignored; the kernel ends the process immediately, so nothing gets cleaned up. Start with plain `kill`, wait a few seconds, and use `-9` only if it's still running. `kill -0 12345` checks without sending anything: it succeeds while the process exists.

## Kill it by name

```bash
# every process named firefox
pkill -x firefox

# the same: exact names by default
killall firefox

# "npm run" anywhere in the command
pkill -f "npm run"
```

Without `-x`, `pkill node` also matches `nodemon`, and `-f` matches far more than you might expect, so preview with `pgrep -a` first: it uses the same matching and only prints. If `killall` isn't found, it's in the `psmisc` package (`sudo apt install psmisc`). The differences are in [kill vs pkill vs killall](/posts/kill-vs-pkill-vs-killall/).

## From the desktop or a monitor

- **top:** press `k`, type the PID, press Enter, then Enter again for SIGTERM (or type `9`).
- **htop:** select the process with the arrow keys, press `F9`, choose a signal, press Enter. `F4` filters by name.
- **GNOME:** open **System Monitor**, right-click the process, choose **End** (SIGTERM) or **Kill** (SIGKILL).
- **KDE Plasma:** **Ctrl+Esc** opens the process list; select it and click **End Process**.
- **A frozen window (X11):** run `xkill` and click the window. It doesn't work on Wayland.

## Kill a process and its children

`kill` signals only the PID you give it. Stopping `npm run dev` can leave the `node` server it started running. To see what a process started, and stop all of it:

```bash
# its children
pgrep -P 12345

# stop the children, then the parent
pkill -P 12345
kill 12345
```

A shell job usually runs as its own process group, and a negative number sends the signal to the whole group:

```bash
# its group; say it prints 12340
ps -o pgid= -p 12345

# signal all of group 12340
kill -- -12340
```

## Everything a user runs

```bash
# every process of user dev
sudo pkill -u dev

# only dev's node processes
sudo pkill -u dev -x node
```

On a shared machine, check with `pgrep -u dev -a` first.

## It keeps coming back

If the process reappears with a new PID moments after you kill it, something is restarting it, and killing it again won't help:

- **A systemd service:** `systemctl status nginx` names the service; stop it with `sudo systemctl stop nginx` (and `sudo systemctl disable nginx` to keep it from starting at boot).
- **A container:** `docker ps` lists it; stop it with `docker stop <name>`. Killing the container's main process from the host stops the container, and a restart policy starts it again.
- **A supervisor or watcher:** `pm2`, `supervisord`, `nodemon` and similar restart what they watch. Stop them first.

To find out which: look at its parent, with `ps -o ppid= -p 12345` and then `ps -p <that PID>`. A parent of `systemd` (PID 1) usually means a service.

## When kill -9 doesn't work

- **`Operation not permitted`:** it belongs to another user or to root. Use `sudo kill 12345` if you're sure. Kernel threads (names in square brackets in `ps`) can't be killed at all.
- **It shows `Z` or `<defunct>`:** it's a zombie. It has already exited; its parent hasn't collected its exit status. Restart the parent instead. See [zombie, orphan and stuck processes](/posts/zombie-orphan-stuck-processes-guide/).
- **It shows `D`:** it's waiting on a disk or a network mount that stopped answering, and no signal reaches it until that wait ends. Fix the mount or the device; sometimes only a reboot clears it.

## With proc

[proc](https://github.com/yazeed/proc-cli) takes a PID, a name or a port in the same place, shows you what matched, and asks before it stops anything:

```bash
# SIGTERM, SIGKILL after 10s
proc stop firefox

# preview what a name matches first
proc kill node --dry-run

# what it started, what started it
proc tree 12345
proc why 12345
```

`proc why` shows the chain of parents, so a service or a supervisor that would restart the process is visible before you stop it. A name matches the whole process name, and proc never matches the shell it runs in, so `proc kill bash` can't end your own terminal.

## Install

```bash
# npm or bun
npm install -g proc-cli

# Rust
cargo install proc-cli
```

It's also in apt, dnf, Nix, pkgsrc and more: the [install page](/install) lists every way, by system.
