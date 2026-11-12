---
title: "How to force quit an app or kill a process on a Mac"
description: "Force quit a frozen Mac app, find the Mac's Task Manager (Activity Monitor) and Ctrl+Alt+Delete, and kill any process from Terminal with kill, killall and pkill, including one holding a port. What to do when Force Quit doesn't work."
date: 2026-11-12
---

A Mac app stops responding, the cursor turns into a spinning wheel, and clicking does nothing. Or a process you can't even see is eating the CPU or holding the port your server needs. macOS has three ways to stop it, from the gentlest to the most direct.

## The short answer

1. **A frozen app:** press **Option-Command-Esc**, pick the app, and click **Force Quit**.
2. **A process with no window** (a helper, a background job, something using all the CPU): open **Activity Monitor**, select it, click the **Stop** button (the octagon with an ✕) at the top, then **Force Quit**.
3. **From Terminal:** `pkill -x Safari` or `killall Safari` by name, or `kill 12345` by PID.
4. **Something holding a port:** `lsof -i :3000 -sTCP:LISTEN`, then `kill` the PID it shows.

## Force quit a frozen app

There are three ways to reach Force Quit. Each one only ends apps, the programs with a window and a Dock icon.

- **Keyboard:** **Option-Command-Esc** opens the Force Quit Applications window. An app that stopped responding is marked *(Not Responding)*. Select it and click **Force Quit**.
- **Apple menu:** click the Apple logo, then **Force Quit…**: the same window.
- **Dock:** Control-click (or right-click) the app's icon, then hold **Option**: **Quit** turns into **Force Quit**.

Force Quit doesn't let the app save, so anything unsaved in it is lost. If the app is only slow, give it a minute first.

To quit an app normally from Terminal, the same as pressing Command-Q in it (it may ask to save):

```bash
osascript -e 'quit app "Safari"'
```

## The Mac's Task Manager: Activity Monitor

There's no Ctrl+Alt+Delete on a Mac. Option-Command-Esc is the closest shortcut, and **Activity Monitor** is the equivalent of the Windows Task Manager: it lists every process, not just apps, with its CPU, memory, energy, disk and network use.

To open it, press **Command-Space**, type **Activity Monitor**, and press Return. It lives in **Applications → Utilities**. To keep it handy, Control-click its Dock icon while it's open and choose **Options → Keep in Dock**.

To stop a process in it:

1. Click the **CPU** tab and the **% CPU** column to put the busiest processes at the top, or type a name in the search field at the top right.
2. Select the process.
3. Click the **Stop** button (the octagon with an ✕) in the toolbar.
4. Choose **Quit** to ask it to exit, or **Force Quit** to end it at once.

The **PID** column is the process ID that the Terminal commands below take. If you can't see it, Control-click the column headers and turn on **PID**.

## Kill a process from Terminal

Terminal reaches every process, including the ones Activity Monitor shows but Force Quit doesn't. First find it, then signal it.

```bash
# find it, in any case: PID, name
pgrep -il safari

# or with the full command line
ps aux | grep -i safari
```

Then stop it:

```bash
# by PID: ask it to exit (SIGTERM)
kill 12345

# by PID: force it (SIGKILL)
kill -9 12345

# by exact name
killall Safari
pkill -x Safari
```

App names with spaces need quotes: `killall "Google Chrome"`. Names are case-sensitive for `killall` and `pkill -x` (`killall safari` finds nothing; proc ignores case), and an app often runs helper processes under other names (`Google Chrome Helper (Renderer)`), which `pgrep -il chrome` lists. Try `kill` before `kill -9`: SIGTERM lets the program save and clean up, SIGKILL doesn't. For the differences between the three commands, see [kill vs pkill vs killall](/posts/kill-vs-pkill-vs-killall/).

A process that belongs to another user or to the system answers `Operation not permitted`. Run the same command with `sudo` if you're sure.

## Kill the process on a port

When a dev server says the port is already in use, find what's listening there and stop it:

```bash
lsof -i :3000 -sTCP:LISTEN
kill 12345
```

If that's port 5000 or 7000, it's probably the AirPlay Receiver, which comes back when killed: turn it off in **System Settings → General → AirDrop & Handoff**. The [port guide](/posts/find-kill-process-on-port-macos-linux/) covers the rest, and the [address already in use](/posts/eaddrinuse-address-already-in-use-fix/) post explains the error in each language.

## When Force Quit doesn't work

- **The whole Mac is frozen**, and Option-Command-Esc does nothing: try the Terminal commands over SSH from another machine if you have one; otherwise hold the power button for about 10 seconds to turn the Mac off. Unsaved work in every app is lost.
- **It comes straight back.** Some processes are restarted by macOS itself (launchd) or by the app that owns them. That's how `Dock`, `Finder` and many helpers behave, and it's how [`killall Dock` restarts the Dock](/posts/kill-vs-pkill-vs-killall/#killall-on-macos-the-everyday-uses). Quit the app that starts the helper, or turn off the feature behind it.
- **`kernel_task` is using a lot of CPU.** That's the kernel, and it can't be killed. It takes CPU on purpose to keep a hot Mac from overheating: close what's heating it, or let it cool.
- **Don't force quit `WindowServer` or `loginwindow`.** Ending either logs you out and closes every app.
- **A process won't die even with `kill -9`.** It's waiting on a disk or a network drive that stopped answering. See [stuck processes](/posts/zombie-orphan-stuck-processes-guide/).

## With proc

[proc](https://github.com/yazeed/proc-cli) does the Terminal part in commands you'd guess, and shows you what it found before it stops anything:

```bash
# the 10 busiest processes
proc list --sort cpu --limit 10

# what's on port 3000, then free it
proc on :3000
proc free :3000

# stop every process named Safari
proc stop Safari
```

`proc stop` asks the process to exit and forces it after 10 seconds if it hasn't. Every command that stops something lists what it found and asks first. proc runs on macOS, Linux and Windows.

## Install

```bash
# Homebrew
brew install yazeed/proc/proc

# npm or bun
npm install -g proc-cli
```

The [install page](/install) lists every other way.
