---
title: "Two readers: why proc answers people and programs differently"
description: "proc is read by people at a terminal and by scripts and AI agents through --json. Here's what each one needs, the decisions that follow from it (one card for every process, no number nobody measured, who can reach a port, a word to branch on), and how to use them."
date: 2026-11-05
---

Every proc command has two kinds of reader. One is a person at a terminal who wants an answer they can read at a glance. The other is a program: a shell script, a CI job, or an AI coding agent reading proc's output through `--json`. Most command-line tools are written for one of them and put up with the other.

proc writes for both, on purpose, and in proc 2.3 we wrote down what that means in its [philosophy](https://github.com/yazeed/proc-cli/blob/main/PHILOSOPHY.md), under "Easy for People, Complete for Programs". This post explains the rule, the decisions that came out of it, and how to use them.

## What each reader needs

**A person** needs the answer first, in words, cut to fit the terminal. Detail is there when they ask for it with `-v`, which prints everything, uncut.

**A program** needs two things, and they pull in different directions:

- **A script needs it stable.** A JSON key, its type and an exit code have to mean the same thing for a whole major version, or the script breaks.
- **An agent needs it complete.** It should be able to decide what to do from one answer, without a second call to fill in what's missing.

And both need the same truth. The default output and the JSON describe the same processes; they're just written for different readers. Every new feature now has to answer one more question before it goes in: does it serve both readers?

## One card for a process, wherever you ask

Ask about a process by its port, its PID or its name, and you get the same card:

```
$ proc on :4799
✓ Port 4799 is used by 1 process, reachable from this machine only

  Process:     python3 (PID 94739)
  Command:     python3 -m http.server 4799 --bind 127.0.0.1
  Directory:   /Users/dev/code/my-app
  Path:        /Users/dev/.local/bin/python3
  User:        dev
  Parent:      zsh (PID 94738)
  Status:      running
  Resources:   0.0% CPU, 24.6MB, 0s CPU time, 45 open files
  Started:     03:51:10 (up 1s)
  Children:    none
  Listening:   :4799 TCP on 127.0.0.1 (this machine only)
  Connections: none
```

`proc info`, `proc for` and `proc why` print it too. Before 2.3, these views showed different details under different labels; even `proc on` alone printed one layout by port and another by PID or name. Now there's one layout to learn. The card answers the questions people actually ask about a process they found: what is it, who started it, where is it running from, and what is it listening on.

## Never show a number nobody measured

Some things proc can't read. On macOS, an ordinary user can't see another user's CPU, memory or sockets; on Linux, the same goes for another user's sockets unless proc runs as root. Before 2.3, proc filled those gaps with zeros: `0.0% CPU`, `0KB`, `Listening: none`. Those look like measurements, and they're not. "Listening: none" in particular reads as "this process has no open ports", when the truth is "proc couldn't look".

So proc stopped filling gaps. In the card, a line proc couldn't read is left out:

```
$ proc info 1
✓ launchd (PID 1)

  Process:     launchd (PID 1)
  Path:        /sbin/launchd
  Children:    322
```

In tables, an unknown value is a dash:

```
 PID  NAME     USER  CPU%  MEM  UPTIME  COMMAND
 1    launchd  -        -    -       -  -
```

In JSON, an unknown value is a missing key: never `0`, never `null`, never `"none"`. An agent that reads `"open_files": 0` concludes the process has no files open. An agent that finds no `open_files` key knows it doesn't know.

One honest exception: JSON keys that existed before 2.3, like `cpu_percent` and `status`, keep their old values for such a process (`0.0`, `"unknown"`), because a 2.x script may depend on them being there. The keys added in 2.3 follow the new rule. Bringing the older keys into line is a change that has to wait for a major release.

**Using it in a script:** treat a missing key as "unknown", not as zero:

```bash
proc info 1234 --json | jq '.processes[0].open_files // "unknown"'
```

## Say who can reach a port

The question behind "what's on port 3000?" is often a second one: can anyone else reach it? A dev server bound to `127.0.0.1` is only reachable from your machine. One bound to `0.0.0.0` is reachable from your network, and maybe from the internet.

So every port now says which it is. The card says "reachable from this machine only" or "reachable from the network", `proc ports` has an EXPOSURE column, and the JSON has `exposure: "local"` or `"exposed"`:

```
$ proc ports
 PORT   PROTO  ADDRESS    EXPOSURE  PID    NAME           UPTIME  COMMAND
 5000   TCP    0.0.0.0    exposed   671    ControlCenter  32d 19h /System/Library/CoreServices…
```

(That one is macOS's AirPlay receiver, which listens on port 5000 on every interface. It's also why "port 5000 is already in use" catches so many Flask developers on a Mac.)

**Using it:** list everything your machine exposes to the network.

```bash
proc ports --json | jq -r '.ports[] | select(.exposure == "exposed") | "\(.port) \(.process_name // .command)"'
```

## Show the command, not just the name

"Which `node` is this?" is the question a process table usually fails to answer, because ten processes are all called `node`. The command line tells them apart: `node server.js` and `node node_modules/.bin/vite` are different programs. So every process table now shows the same columns: `PID NAME USER CPU% MEM UPTIME COMMAND`. The uptime answers "is this the one I started a minute ago?".

One place prints the command line whole, at any terminal width: the list proc shows before it asks you to confirm a kill. You're approving exactly those processes, so you should be able to read exactly what they are.

## Give programs a word to branch on

When proc fails, the `error` field is a sentence for a person, and it can quote a process's name. A process name is text that whoever started the process chose. That makes it a bad thing for a program to parse, and a dangerous thing for an AI agent to treat as instructions.

So every failure also has a `reason`: one fixed word a program can branch on. Here's a guarded kill, where `--start-time` says "only if it's still the process I saw start at this time":

```json
{
  "action": "kill",
  "error": "No process found matching '95268' (ruled out by --start-time 1)\n  Try: proc list to list all processes",
  "exit_code": 2,
  "reason": "ruled_out",
  "ruled_out_by": ["--start-time"],
  "success": false
}
```

`reason: "ruled_out"` says a process holds that PID but a guard excluded it, and `ruled_out_by` says which guard. For an agent, `--start-time` ruling it out means "a different process now has that PID, so the one you meant is gone". That's very different from `reason: "not_found"`, and no agent should have to work it out from the wording of a sentence.

This is what "facts, never instructions" means in the philosophy: proc's JSON tells a program what is true, and leaves what to do about it to the program and the person it works for.

## How we hold ourselves to it

A rule nobody checks drifts. These are the checks:

- The philosophy's feature test has a seventh question, "Does it serve both readers?", and every new feature has to pass it.
- proc's [agent reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md) is checked against the real `--json` output of the commands it describes.
- Before the next major release, every command and every kind of output (results, previews, prompts, receipts, errors) gets audited against both halves of the rule.

If you find a view that shows a number proc never measured, or JSON that makes your script call proc twice, [open an issue](https://github.com/yazeed/proc-cli/issues/new). That's exactly the kind of gap the audit is for.

## Try it

```bash
brew install yazeed/proc/proc
proc ports
proc on :3000 --json
```

Other ways to install are on the [install page](/install), every command is in the [reference](/commands), and the full history is in the [changelog](/changelog).
