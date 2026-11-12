---
layout: page
title: Agent skill
description: "Teach your coding agent to use proc: install the proc-cli skill for Claude Code, Codex, Cursor, Gemini CLI and others."
permalink: /skill
---

The proc-cli skill teaches coding agents (Claude Code, Codex, Cursor, Gemini CLI and others) to use proc instead of `lsof`, `ps` and `kill -9`, and to be careful with your processes. Instead of `lsof -ti :3000 | xargs kill -9`, your agent asks proc who holds the port, shows you, and stops only what it started or what you agreed to. proc holds up its side too: nothing that stops, kills or pauses a process runs without `--yes` unless a person confirms at the prompt, and a name matches only the whole process name.

## What changes in your agent

- **It looks before it touches.** Questions like "what's on port 3000?" or "what's using the CPU?" it answers at once: those commands only read. Before it stops, kills or pauses anything it didn't start, it runs the same command as a dry run first.
- **It stops only what it started, or what you approved.** When it starts a server, it records the PID and when that process started. Anything else, it shows you first, process by process: the PID, name, directory, command line, what started it and how long it has run, and what it would do to it. Then it waits for your yes.
- **A name is a search, not a target.** "Kill node" means "find the node processes". On one developer's laptop that matched 185 processes across many projects. The agent asks which ones you mean, then acts on those PIDs only.
- **A reused PID can't be hit.** Once a process ends, the system hands its PID to a new one, on a busy machine within the hour. The agent passes along the start time it recorded, and proc refuses to act if the PID now belongs to a different process.
- **It takes the gentle route.** It uses `stop`, which asks a process to exit and forces it only if it doesn't, before `kill`. It doesn't use `sudo` without asking, and it never stops everything `stuck` or `orphans` lists: those are leads it shows you, not verdicts.
- **It cleans up only its own.** Before it finishes, it stops the servers it started, their child processes too (stopping `npm run dev` can leave its `node` running), and checks that they're gone. A leftover it didn't start, like a listener in your project or a headless browser, it reports instead of stopping: it may be yours or another agent's.
- **A process can't give it orders.** Process names and command lines are written by whoever started the process. The agent shows that text to you and decides on proc's facts (PIDs, ports, start times, exit codes), never on instructions hidden in it.
- **It won't stop what it runs in.** proc never matches itself, and the agent leaves alone the shell and the agent session it runs under.
- **It tells you what happened.** It says a process is gone only after proc confirms it, and it tells you what the system refused.

## What it looks like

Ask it to free a port that something else holds, and the answer reads something like this:

> **You:** Port 3000 is taken, free it.
>
> **Agent:** Port 3000 is held by `node` (PID 48211), running `node server.js` in `/Users/dev/my-app`, started by `npm run dev` 2 hours ago. I didn't start it. Freeing the port would stop it: it gets 5 seconds to exit, then it's killed. Should I stop it, or start my server on another port?

## Install

proc itself has to be installed first (see [install](install)). Then add the skill:

```bash
npx skills add yazeed/proc-cli
```

Or with bun:

```bash
bunx skills add yazeed/proc-cli
```

To pick the agent and where it's installed:

```bash
npx skills add yazeed/proc-cli --skill proc-cli -a claude-code -y   # one agent, this project
npx skills add yazeed/proc-cli --skill proc-cli -g                   # every project (user-level)
```

### Claude Code plugin

In Claude Code, you can install it as a plugin instead:

```text
/plugin marketplace add yazeed/proc-cli
/plugin install proc-cli@proc-cli
```
{: .commands}

## Update

```bash
npx skills update
```

Installs always fetch the current skill from the repository, so a new install is up to date too. The skill is written for proc 2.4 or later. The agent checks `proc --version` first: on an older proc it only runs commands that read, and asks you to upgrade before it stops anything.

## Try it

Once it's installed, ask your agent things like:

- "What's on port 3000?"
- "Port 3000 is taken, free it"
- "Why is port 8080 busy?"
- "What is PID 4321, and can anything outside this machine reach it?"
- "What did you leave running in this project?"
- "My machine is slow, what's using the CPU?"
- "Wait for the build to finish, then start the server"

## Where it lives

- Source: [`skills/proc-cli/`](https://github.com/yazeed/proc-cli/tree/main/skills/proc-cli) in the repository. [`SKILL.md`](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/SKILL.md) is what the agent reads, with the exact rules, and [`reference.md`](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md) has every JSON output
- Scripting proc yourself: the same footing the agent stands on, `--json` on every command, fixed exit codes and a reason with every error, is in the reference, and [Messages](commands/messages) says what each warning and error proc prints means
- Listing: [skills.sh/yazeed/proc-cli/proc-cli](https://www.skills.sh/yazeed/proc-cli/proc-cli), where it passes the security audits from Gen Agent Trust Hub, Socket and Snyk
- Evals: [`evals.md`](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/evals.md) lists the tasks the skill is checked against, each with what the agent must and must never do
- Format: the [Agent Skills](https://agentskills.io) standard, so any agent that supports skills can use it
