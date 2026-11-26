---
title: "What's new in proc-cli: nineteen releases since March"
description: "Freeing ports and tracing where they came from, finding the processes you left behind, a JSON contract for scripts and coding agents, and proc-cli's new home at proc-cli.com."
date: 2026-10-25
---

The last post here was in March, when proc-cli was at 1.8. Since then it has had nineteen releases, up to 1.14.5. Most of what changed falls into four things: dealing with a busy port end to end, finding processes you forgot about, being dependable for scripts and coding agents, and being easier to install. Here's what you can do now that you couldn't before.

## A busy port, from start to finish

The question proc-cli started with is "what's on port 3000?". `proc on :3000` still answers it. The more useful questions turned out to be the next two: where did it come from, and how do I get the port back?

```bash
proc why :3000
```

`proc why` traces the chain of parent processes behind whatever is listening, with how long each one has been running:

```
✓ Port 3000 (TCP):

○ systemd [1]  up 3d 2h
    └── ○ tmux [172] new-session -d  up 2d 4h
        └── ○ npm [173] run dev  up 2d 4h
            └── ○ node [202] server.js  up 2d 4h  ← target
                dir: /home/dev/code/shop
```

That's a dev server you started two days ago in a tmux session and forgot. To get the port back:

```bash
proc free :3000
```

`proc free` asks the process to exit, forces it if it hasn't within five seconds, and then waits until the port is really free before saying so. That last step matters, because a port can stay busy briefly after its process is gone.

## The processes you left behind

Closing a terminal doesn't always stop what you started in it. Dev servers, file watchers and headless browsers keep running with no parent, using memory and holding ports. `proc orphans` finds them, and `--in .` keeps it to the current project:

```bash
proc orphans --in .
proc orphans --in . --kill
```

`--kill` shows the list and asks before doing anything. If you'd rather pause something than end it, `proc freeze` and `proc thaw` stop and resume a process, which is handy for a heavy build while you need the machine back. `proc wait` blocks until a process exits, replacing the `while ps aux | grep` loop.

Every process view now shows how long each process has been running, in tables, in `proc why` and `proc tree`, and as `uptime_secs` in JSON. An uptime of three days next to a dev server is usually the whole story.

## Built for scripts and coding agents

A lot of the work since March went into making proc-cli predictable for anything that isn't a person reading a table:

- **One JSON document per command.** With `--json`, every command prints exactly one document with an `action` and a `success` field, including errors and empty results.
- **Exit codes that always mean the same thing.** `0` success, `1` error, `2` not found, `3` permission denied, `4` invalid input. A typo in a flag is `4`, and so is an unknown `--status` value, instead of quietly matching everything.
- **Dry runs that say what they are.** `--dry-run --json` reports the command it previews and `"dry_run": true`.

That made proc-cli a good fit for coding agents, which are often the ones starting and forgetting dev servers. The [agent skill](/skill) teaches Claude Code, Codex, Cursor, Gemini CLI and others when to reach for proc-cli instead of piping `lsof` into `kill -9`, how to clean up after themselves, and to leave alone the processes they didn't start:

```bash
npx skills add yazeed/proc-cli
```

## Fixes worth knowing about

- **Linux binaries are static.** They used to need a recent glibc, so they failed on Debian 12 and older. They now run anywhere from Debian 10 and Ubuntu 20.04 to Alpine.
- **Linux lists processes, not threads.** proc-cli used to list each thread of a process as if it were a separate process, so a Node server could appear a dozen times. That's fixed in 1.14.4.
- **macOS uptimes are honest.** Processes owned by other users used to report an uptime of about 56 years, because their start time can't be read. They now show as unknown.
- **Processes in a deleted folder are still found.** `proc in` lists processes still running in a directory you've since removed, such as an old git worktree, which is where forgotten servers tend to live.

## A new name and a new home

The repository is now [yazeed/proc-cli](https://github.com/yazeed/proc-cli), matching the crate, the npm package and the agent skill, and this site moved to proc-cli.com. Links to the old repository redirect to the new one. The Docker image is `yazeed/proc-cli`, and the npm package no longer runs an install script: it downloads and checks the binary the first time you run it.

## Try it

```bash
brew install yazeed/proc/proc
proc ports
```

Other ways to install are on the [install page](/install), every command is in the [reference](/commands), and the [FAQ](/faq) covers the questions people search for most, from "address already in use" to zombie processes. The full history is in the [changelog](/changelog).
