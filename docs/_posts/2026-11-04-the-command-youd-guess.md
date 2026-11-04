---
title: "The command you'd guess: how proc decides what your guess means"
description: "proc's one rule is that the command you'd guess is the command that works. Here's where it still falls short, the rule we chose for every guess instead of patching error messages one at a time, why proc never acts on a guess, and what to type today."
date: 2026-11-04
---

proc is a command-line tool for finding and stopping processes and ports: what's running, what's holding port 3000, how to make it stop. It's built on one rule, written down in its [philosophy](https://github.com/yazeed/proc-cli/blob/main/PHILOSOPHY.md): **the command you'd guess should be the command that works.** You shouldn't need the docs to free a port.

proc doesn't fully live up to that yet. This post covers:

- the guesses proc still gets wrong, and what each one does today;
- the rule we decided on for every guess, and why it's one rule instead of many fixes;
- the one thing a guess will never make proc do;
- what to type today.

## How proc reads what you type

The failures below make sense once you know proc's one piece of grammar. Every command takes the same kinds of target:

```bash
proc kill :3000    # a leading colon is a port
proc kill 1234     # a plain number is a process ID (PID)
proc kill node     # anything else is a process name
```

That's what lets one word do one job everywhere: `proc on :3000` tells you what's on port 3000, `proc on 1234` tells you about process 1234, and `proc on node` about every process named node. The catch is that the colon is proc's own convention. Nobody arrives knowing it.

## The guesses proc gets wrong

We collected the commands people type out of habit from the tools they already use (`kill`, `pkill`, `lsof`, `ps`, `top`, `kill-port`) and from AI coding agents trying proc for the first time, and ran each one through proc. The outputs below are from proc 2.3.

### "Free port 3000": `proc kill 3000`

The most common habit by far is a bare port number. The npm package `kill-port` works that way (`npx kill-port 3000`), and it was downloaded 2,616,686 times last week. In proc, a bare number is a PID, so this is what you get:

```
⚠ Not found: 3000
No process found matching '3000'
  Try: proc list to list all processes
```

Nothing was killed, which is the right outcome. But the hint sends you to the wrong place. It doesn't mention that you probably meant the port, `:3000`.

### "List everything": `proc ps aux`

`ps aux` is how many people list every process. proc has a `ps` command that already lists everything, so it reads `aux` as the name of a process to look for. There's no process named `aux`, so it prints `⚠ No processes found` and exits with status 0, which means success. A person can tell something's off. A script can't: it carries on as if the machine had no processes. A wrong answer that looks like a right one is the worst kind of failure, and it's the kind we fix first.

### "Force it": `proc kill -9 node`

`kill -9` sends SIGKILL, the signal a process can't ignore. Typing it into proc stops at:

```
error: unexpected argument '-9' found
```

`proc stop --force node` fails the same way. The honest part is that you don't need either: `proc kill` always sends SIGKILL, and `proc stop` asks the process to exit cleanly first. proc just doesn't tell you that.

### A tip that matches letters, not meaning: `proc what :5000`

When you type a command proc doesn't have, it suggests the closest names:

```
tip: did you mean 'wait' or 'watch' or 'why'?
```

They're close in spelling, because they all start with a w. None of them answers "what's on port 5000?". The command you wanted is `proc on :5000`.

### One letter, two meanings: `-n`

`proc list -n 5` shows five processes: `-n` is the limit. `proc watch -n 2` refreshes every two seconds: there, `-n` is the interval. Same letter, two jobs, depending on the command. You find out which by getting it wrong.

## One rule instead of fifty fixes

The obvious fix is to patch each message when someone trips over it. Each patch would make sense on its own. Together they'd be a pile of special cases that nobody could predict, and predictable is the whole point of proc. So we decided on a single rule, and every guess gets one of three answers:

1. **It works.** If a habit has one sensible meaning in proc, proc accepts it and tells you in its own words what it did.
2. **It teaches.** If proc can't safely take the guess, the error names the command you meant and why. For `proc kill 3000`, that's telling you a port is written `:3000`.
3. **It's a plain error,** when there's nothing sensible to suggest.

A guess can only work if it passes every one of these tests:

- it means one thing in the tool it comes from, and that thing is exactly something proc already does;
- it's an error in proc today, so accepting it changes nothing that already works;
- it doesn't make a command that stops processes looser: no wider match, no stronger signal, no less confirmation than proc's own spelling.

`kill -9` passes all three, because `proc kill` already sends SIGKILL. A guess that fails one of them can only teach.

Tips are chosen by what you meant first and by spelling second, which is what went wrong with `what :5000`. And when no tip would be right, proc gives none: no tip beats a wrong tip.

Each guess and its answer goes into one table, with a test, so a guess that's fixed stays fixed and every new command is checked against the same list.

## The one thing a guess never does

A guess never makes proc act on a reading you didn't type. `proc kill 3000` will never kill whatever is listening on port 3000.

That can look stubborn, since proc could check whether something is on port 3000 and kill it. But a PID and a port number can be the same number. If process 3000 exists and something else holds port 3000, either guess kills a process you might not have meant. proc's other rule is that nothing is stopped by accident: destructive commands ask first, and `--json` or a script without a terminal never counts as a yes. Acting on a guess would go around all of that. So the hint tells you what it thinks you meant, and you decide.

## Changes that add, and changes that break

Most of this only adds: better hints, notes, habits that start working. Those arrive first, and nothing you rely on changes.

A few fixes change what something already means: `-n` meaning one thing everywhere is one, and a name that matches nothing being "not found" in every command is another. A script might depend on the current behavior, so those wait for a major release, and every one of them prints a notice first, each time you use the spelling that will change. Nothing breaks without proc telling you in advance. The [roadmap](https://github.com/yazeed/proc-cli/blob/main/ROADMAP.md) keeps the decided list.

## If you use proc in scripts or with AI agents

Every proc command can answer in JSON with `--json`, and its exit codes have fixed meanings (0 success, 2 not found, 4 nothing was done, and so on), so scripts and AI agents can rely on it instead of parsing text. The same rule applies there: one spelling, one meaning. Two places where the JSON still has two rules, and will get one:

- **`proc on` returns four different shapes of JSON,** depending on whether you gave it a port, a PID, one name or several.
- **"Nothing found" means different things in different commands.** In `list`, `in`, `stuck`, `orphans` and `free` it's a success (exit 0). In `by`, `kill` and `on` it's an error (exit 2).

To be ready: use the current JSON field names ([listed in the reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md#names-deprecated-in-230)), write `--limit` and `--interval` in full rather than `-n`, and pass several targets as separate arguments (`proc kill :3000 :8080`) rather than with commas. A script that does those things has nothing to change.

## What to type today

| What you want | Instead of | Type this |
|---|---|---|
| Free port 3000 | `proc kill 3000` | `proc kill :3000` or `proc free :3000` |
| See what's on a port | `proc what :5000` | `proc on :5000` |
| List every process | `proc ps aux` | `proc ps` |
| Force a process to stop | `proc kill -9 node` | `proc kill node` (always SIGKILL) |
| Stop it cleanly | `proc stop --force node` | `proc stop node` |
| Watch every 2 seconds | `proc watch -n 2` | `proc watch --interval 2` |

## Tell us what you typed

Our list came from other tools' habits and from AI agents. It's missing yours. If you tried proc and your first guess didn't work, [open an issue](https://github.com/yazeed/proc-cli/issues/new) with what you typed and what you expected. It goes into the table, gets an answer and a test, and stays fixed.

## Try it

```bash
brew install yazeed/proc/proc
proc on :3000
```

Other ways to install are on the [install page](/install), every command is in the [reference](/commands), and the full history is in the [changelog](/changelog).
