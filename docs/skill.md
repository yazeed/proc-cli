---
layout: page
title: Agent skill
description: "Teach your coding agent to use proc: install the proc-cli skill for Claude Code, Codex, Cursor, Gemini CLI and others."
permalink: /skill
---

The proc-cli skill teaches coding agents (Claude Code, Codex, Cursor, Gemini CLI and others) when to reach for proc and how to use it safely. Instead of `lsof -ti :3000 | xargs kill -9`, your agent previews with `proc free :3000 --dry-run`, reads structured JSON, and checks with you before touching a process it didn't start. It adds `--yes` only for a process it started itself or after you've seen the list and agreed, and never with a name. proc enforces its side too: nothing that stops, kills or pauses a process runs without `--yes` unless a person confirms at the prompt, `--json` never counts as a yes, and a name matches only the whole process name.

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

## Update

```bash
npx skills update
```

Installs always fetch the current skill from the repository, so a new install is up to date too. The skill is written for proc 2.0 or later. It tells your agent to check `proc --version`, and to ask you to upgrade rather than run destructive commands on an older proc.

## What your agent learns

- **When to use proc**: a busy port (`EADDRINUSE`, "address already in use"), what's listening on a port or running in a directory, stopping or freeing something, a dev server that won't start, a stuck process or a slow machine, and cleaning up the servers and browsers it started. Not containers, system services or remote hosts
- **A safety model on the first screen**: which commands only read, which can be undone (`freeze`, `thaw`), and which end a process (`stop`, `kill`, `free`); a process is its own only if it started it and recorded the PID; a name is a search, not approval
- **The contract**: `--json` on every command, one JSON document per command (`watch` streams one per refresh), and fixed exit codes (0 success, 1 error, 2 not found, 3 permission denied, 4 invalid input or confirmation needed). Every error says why in `reason` (and, for `ruled_out`, which guard in `ruled_out_by`, or which of its own ancestors in `left_out`), and the agent branches on those, never on the text; exit 4 means nothing changed (`invalid_input`, `needs_confirmation` or `not_supported`); with `needs_confirmation` it shows you the list, and it doesn't retry with `--yes` on its own
- **Playbooks**: a busy port, stopping its own servers by PID with a guard (`--start-time`, `--in` and `--by`), stopping something it didn't start only after you approve the PIDs, cleaning up only what it recorded, and reading `stuck` and `orphans` as leads rather than verdicts
- **Safety**: preview with `--dry-run`, show you the PID, name, directory, command and parent, and wait for a yes; prefer `stop` over `kill`; never `--kill` on `orphans` or `stuck`; ask before `sudo`
- **Gotchas**: `kill` and `stop` don't signal child processes, `:3000` is a port but `3000` is a PID, a port can have several holders, `--in` narrows a search but doesn't prove a process is the agent's, and an orphaned process isn't necessarily abandoned

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

- Source: [`skills/proc-cli/`](https://github.com/yazeed/proc-cli/tree/main/skills/proc-cli) in the repository ([`SKILL.md`](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/SKILL.md), and [`reference.md`](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md) for every JSON output)
- Listing: [skills.sh/yazeed/proc-cli/proc-cli](https://www.skills.sh/yazeed/proc-cli/proc-cli), where it passes the security audits from Gen Agent Trust Hub, Socket and Snyk
- Evals: [`evals.md`](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/evals.md) lists the tasks the skill is checked against, each with what the agent must and must never do
- Format: the [Agent Skills](https://agentskills.io) standard, so any agent that supports skills can use it
