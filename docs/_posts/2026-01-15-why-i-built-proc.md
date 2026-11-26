---
title: "Why I built proc-cli"
description: "I kept forgetting how to find what's on port 3000. So I built a tool where the command you'd guess is the command that works."
date: 2026-01-15
---

I couldn't remember the flags.

Every day I'd need to find what's running on a port, what process is running from the current folder, or figure out which node instance was eating CPU. And every time, I'd end up looking up the same `lsof` incantation:

```bash
lsof -i :3000 -P -n
```

The `-P` disables port name resolution. The `-n` disables host resolution. I know this now because I've looked it up dozens of times. And if I wanted to kill it:

```bash
lsof -i :3000 -t | xargs kill -9
```

That's two flags I need to remember just to get a PID, a pipe to xargs, and a signal number. For the most basic operation in development: "something is on my port, make it stop."

## The problem isn't the tools

`lsof`, `ps`, `grep`, `awk`, `kill` — these are good tools. They're composable, they're powerful, and they've been around for decades. The Unix philosophy works.

But for process management specifically, the composability becomes a tax. You're not piping together tools to build something new. You're assembling the same pipeline every time, slightly differently, and hoping you remember the right flags.

```bash
ps aux | grep node | grep -v grep | awk '{print $2}' | xargs kill -9
```

That's five commands to kill a process by name. And if you want to filter by directory, or check what ports it's using, or see a process tree — it's a different pipeline each time, with different tools, different flags, different output formats.

I wanted one tool where the command I'd guess is the command that works.

## The semantic idea

The core insight was that process management has a small, fixed vocabulary. You're always doing one of a few things:

- What's **on** this port?
- Show processes **by** this name
- Kill/stop this target
- What **ports** are in use?
- What's the **info** on this process?

So the commands should just be those words:

```bash
proc on :3000
proc by node --in .
proc kill :3000 :8080 node
proc ports
proc info 1234
```

And targets should work the same everywhere. A port is `:3000`. A PID is `1234`. A name is `node`. You learn it once. Every command accepts the same syntax.

This is the entire design. There's no DSL, no config file, no learning curve beyond the first command.

## Why Rust

I wanted it to be fast, and I wanted a single binary with no runtime dependencies. No "install Node.js first" or "make sure Python 3.8+ is available." Just download and run.

Rust gave me that, plus cross-platform compilation. The same codebase builds for macOS, Linux, and Windows, and the platform-specific parts (lsof on macOS, /proc on Linux, netstat on Windows) are abstracted behind a common interface.

## What I chose not to build

Scope discipline matters more than features. proc-cli does process and port management. It doesn't do:

- Service management (use systemd)
- Containers (use docker)
- Remote processes (use ssh)
- Interactive TUI with fuzzy search (use fkill)
- Historical monitoring (use proper observability)

Every feature has to pass a simple test: does it fit "process and port management," and can it be expressed in one obvious command? If not, it doesn't go in.

## Putting it out there

I posted proc-cli on Hacker News and Reddit. HN was receptive — people understood the pitch and had good technical feedback. Reddit was harder. The Rust subreddit and r/commandline had seen enough port-killing tools that another one didn't register. "Just another ports tool" was the general sentiment.

And honestly, that's fair. If you glance at proc-cli and only see `proc kill :3000`, it does look like killport. The difference is what happens when you need more: filtering by directory, process trees, stuck process recovery, multi-target operations, real-time monitoring, JSON output for scripting. That's where a semantic tool with a unified interface starts to pay off versus a collection of single-purpose scripts.

Whether that difference matters enough is something the community decides, not me. I built the tool I wanted. I use it every day. Where it goes from here, who knows.

## Install

```bash
brew install yazeed/proc/proc     # macOS
cargo install proc-cli            # Rust
npm install -g proc-cli           # npm/bun
```

See the [GitHub repo](https://github.com/yazeed/proc-cli) for all installation options.
