---
layout: page
title: commands
description: "Every proc command, the filters and options they share, what each exit code means, and how to set up completions and the man page."
permalink: /commands
---

{% for group in site.data.commands %}
## {{ group.title }}

{{ group.blurb }}

| Command | Aliases | What it does |
|---------|---------|--------------|
{% for cmd in group.commands %}| [{{ cmd.name }}]({{ cmd.name }}) | {% for alias in cmd.aliases %}`{{ alias }}`{% unless forloop.last %}, {% endunless %}{% endfor %} | {{ cmd.summary }} |
{% endfor %}
{% endfor %}
## Targets

Targets work the same way everywhere:

```bash
proc on :3000          # port
proc on 1234           # PID
proc on node           # name
proc kill :3000 :8080  # multiple (separate arguments)
```

Commands that take targets accept several, as separate arguments, and the kinds mix freely (`proc kill 1234 :3000 node`). A process named twice is handled once, except by the read-only `on` and `why`, which answer each target under its own header. Comma-separated targets (`:3000,:8080`) still work but are deprecated, and will be removed in a future major release. `--in` takes the next word as its directory unless that word starts with `-`, so put it after the targets or write `--in=DIR`.

A name matches the whole process name, ignoring case, the way `killall` and `pkill` read it. `--by` follows the same rule. Add `--cmd` to match any part of the name or the full command line instead, like `pkill -f`.

## It asks before it changes anything

- **Looking is free.** `on`, `for`, `by`, `in`, `list`, `info`, `ports`, `tree`, `why`, `stuck` and `orphans` only read.
- **Changing asks first.** `kill`, `stop`, `freeze`, `thaw`, `free`, `unstick`, `orphans --kill` and `stuck --kill` show what they will touch and wait for a yes. `--dry-run` shows the list and does nothing else; `--yes` skips the question.
- **`--json` is not a yes.** A script or agent that runs a changing command with `--json` and no `--yes` gets exit 4, nothing changed, and the list of processes it would have touched. The same goes for running without a terminal.
- **A name is a search, not an approval.** Preview a name with `--dry-run`, and use `--yes` only with a `:port` or a PID you have seen.

## Filters

Not every filter is on every command; each command's page lists its own. `--in`, `--by` and `--cmd` work on nearly all of them, including the ones that change processes.

| Filter | Short | What it keeps | Commands |
|--------|-------|---------------|----------|
| `--in [<dir>]` | `-i` | Processes running in this directory or a folder below it (bare `--in` is the current directory) | nearly every command |
| `--by <name>` | `-b` | Processes with this name (the whole name, like a name target) | nearly every command |
| `--cmd` | | Match names and `--by` as a part of the name or the full command line, like `pkill -f` | every command that takes a name or `--by` |
| `--start-time <secs>` | | Only the process that started then (its `start_time` from the JSON), so a PID a new process has taken matches nothing. One PID target | `kill`, `stop`, `freeze`, `thaw`, `wait`, `info`, `unstick` |
| `--path <path>` | `-p` | Processes running this executable | `list` |
| `--min-cpu <percent>` | | Using more than this much of one core (fractions allowed) | `by`, `in`, `for`, `list`, `tree`, `watch` |
| `--min-mem <MB>` | | Using more than this many megabytes | `by`, `in`, `for`, `list`, `tree`, `watch` |
| `--min-uptime <secs>` | | Running longer than this many whole seconds | `by`, `in`, `for`, `list`, `tree` |
| `--status <status>` | | `running`, `sleeping`, `stopped`, `zombie`, `idle` (Linux), `starting` (macOS, FreeBSD, NetBSD), `disk-wait` (Linux; `disk_wait` also works) | `by`, `in`, `for`, `list`, `tree` |
| `--parent <pid>` | | Children of this process | `by`, `in`, `list` |
| `--exposed` / `--local` | `-e` / `-l` | Ports open to the network / to this machine only | `ports` |
| `--range <start-end>` | `-r` | Ports in a range, such as `3000-9000` (1 to 65535) | `ports` |
| `--sort <key>` | `-s` | `cpu`, `mem` (biggest first); `pid`, `name` (ascending). `ports`: `port`, `pid`, `name`. Ties come by PID | `list`, `by`, `in`, `for`, `watch`, `ports` |
| `--limit <n>` | `-n` (`-l` on `watch`) | At most this many results | `list`, `by`, `in`, `for`, `ports`, `watch` |
| `--ancestors` | `-a` | The chain of parents up to the root, instead of the children | `tree` |
| `--depth <n>` | `-d` | How deep the tree goes (default 10) | `tree` |
| `--compact` | `-C` | PIDs only | `tree` |
| `--timeout <secs>` | `-t` | Only processes running longer than this (default 300) | `stuck`, `unstick` |

Seconds are whole seconds everywhere except `watch --interval`: `--min-uptime 7200`, not `2h` or `1.5`.

## Options

| Option | Short | What it does | Commands |
|--------|-------|--------------|----------|
| `--json` | `-j` | One JSON document instead of the table or card | all 19 |
| `--verbose` | `-v` | Every value whole. A process table adds PPID, STATUS and DIR and cuts nothing; `ports` adds USER, MEM and PATH | all 19 |
| `--wide` | `-w` | The directory and the command line whole, instead of fitting the terminal | `list`, `by`, `in`, `for`, `stuck`, `orphans`, `watch` |
| `--dry-run` | | Show what would change, change nothing | the commands that change processes |
| `--yes` | `-y` | Don't ask; needed with `--json` or without a terminal | the commands that change processes |
| `--kill` | `-k` | Kill the processes found | `stuck`, `orphans` |
| `--signal <name>` | `-S` | First signal: `HUP`, `INT`, `QUIT`, `ABRT`, `KILL`, `TERM`, `STOP`, `CONT`, `USR1` or `USR2`, any case, with or without `SIG` (no numbers) | `stop` |
| `--timeout <secs>` | `-t` | `stop`: seconds before force-killing (default 10). `wait`: seconds before giving up (default 0, no limit) | `stop`, `wait` |
| `--graceful` | `-g` | Send SIGTERM instead of SIGKILL | `kill` |
| `--force` | `-f` | End the processes that stay stuck (SIGTERM, then SIGKILL) | `unstick` |
| `--wait <secs>` | | Seconds to wait for the port to be free (default 10) | `free` |
| `--interval <secs>` | `-n` | Seconds between status lines (`wait`, whole seconds, default 5) or refreshes (`watch`, at least 0.1, default 2) | `wait`, `watch` |
| `--quiet` | `-q` | No periodic still-running lines | `wait` |
| `--help` | `-h` | Help for proc, or for one command with two examples, a link to its page and, where the exit code says whether it worked, its exit codes | all |

## Platforms

| Platform | Architectures | Not available |
|----------|---------------|---------------|
| macOS | Apple Silicon, Intel | |
| Linux | x86_64, ARM64 (static binaries, any distribution) | |
| Windows | x86_64 | `freeze`, `thaw`, `orphans` and `stop --signal`, which exit 4: Windows has no signals and doesn't re-parent processes |
| FreeBSD, NetBSD | x86_64 (other CPUs: `cargo install proc-cli`) | `in` a deleted directory exits 4: the kernel forgets the path |

On macOS `orphans` also lists every app launchd started, so narrow it with `--in`. Another user's process shows only what the system gives a normal user (on macOS its name, path and children); `sudo` reads the rest.

## Scripting and exit codes {#exit-codes}

<span id="scripting"></span>

With `--json`, every command prints one JSON document with `action` (the command name) and `success`. `proc watch --json` streams one per refresh. Errors, including invalid flags, are JSON too, and every error says why in `"reason"`, one fixed word: `"system_error"` (exit 1); `"not_found"`, `"command_line_only"` or `"ruled_out"` (with `"ruled_out_by"`, the guards that ruled the process out, or `"left_out"`, the processes proc runs under that were all a name matched) for exit 2; `"permission_denied"` (exit 3); and for exit 4, which means nothing was done, `"invalid_input"` (a wrong flag, value or argument), `"needs_confirmation"` or `"not_supported"` (this OS cannot do it). A command that changes processes run with `--json` but without `--yes` exits 4 and adds `"needs_confirmation": true` and `"processes"` (the ones it would have changed); with `--yes`, the receipt lists the same processes as `"processes"`, in the same order, each with an `"outcome"` (`"succeeded"` or `"failed"`, and `"error"` when it failed), and a `"count"`. `why`, `info`, `kill`, `stop`, `freeze`, `thaw`, `unstick`, `free`, `wait`, and `on` given several targets, also put `"not_found"` in their JSON, an array of the targets that matched nothing (empty when all were found); the found ones are still handled, and when every target is missing the command exits 2, with all of them in the error's `not_found` (`free` exits 0: a port nobody holds is already free).

| Exit code | Meaning |
|-----------|---------|
| `0` | Success |
| `1` | Error, or not fully successful: a process failed, a port stayed busy, `wait` timed out |
| `2` | Process or port not found |
| `3` | Permission denied: proc itself got an I/O permission error, or a `:port` has a holder proc can't see (another user's process when proc isn't root, or, on FreeBSD and NetBSD, a system that hides other users' sockets), so the port is not known to be free. A signal the OS refuses (another user's process) is a `failed` entry and exit 1 |
| `4` | Invalid input (unknown flag, bad or empty argument, invalid signal, an `--in` directory that doesn't exist), an action this OS doesn't support (`freeze`, `thaw`, `orphans` and `stop --signal` on Windows), or a command that changes processes run without `--yes` where nobody can confirm (`--json`, or no terminal): nothing was changed |
| `129`, `130`, `131`, `143` | A signal ended proc (SIGHUP, SIGINT or Ctrl+C, SIGQUIT, SIGTERM) while `watch` had the screen or a prompt was waiting: the terminal is put back first, then proc ends by that signal, so a shell shows 128 plus its number and Ctrl+C stops a script's loop. Nothing was changed by an unanswered prompt |

`"success": false` always comes with a non-zero exit code.

### When nothing matches

| Command | Exit code |
|---------|-----------|
| `list`, `in`, `ports`, `stuck`, `orphans`, `tree` without a target | `0`, with a line such as `⚠ No processes found`: an empty answer is still an answer |
| `on`, `by`, `for`, `info`, `why`, `tree <target>`, `wait`, and `kill`, `stop`, `freeze`, `thaw`, `unstick` with targets | `2` when every target matched nothing; with some found, the others are handled and `⚠ Not found:` names the rest |
| `free` | `0`: a port nobody holds is already free |
| `watch` | `2` for PIDs nothing holds; a name or port is watched until something matches |

[Messages](/commands/messages) explains each warning and error proc prints, and what to do about it.

Every JSON shape, the values each flag accepts, what each command prints when nothing matches, and what a release may change are in the [JSON reference](https://github.com/yazeed/proc-cli/blob/main/skills/proc-cli/reference.md). In short: within 2.x keys are only added, never removed or given another meaning, and `error`, `message` and the human output are text for people, so branch on `reason`, `exit_code` and `count`.

## Completions and man page {#setup}

The Nix package and the `.deb` and `.rpm` files install both. With any other install method, add them yourself:

```bash
proc completions bash > /etc/bash_completion.d/proc
proc completions zsh > ~/.zsh/completions/_proc
proc completions fish > ~/.config/fish/completions/proc.fish
proc completions elvish        # also: powershell
sudo proc manpage --dir /usr/local/share/man/man1   # proc.1, and proc-kill.1 for man proc-kill
```

`proc manpage` alone prints `proc.1`; `--dir` writes it and a page for each command into the directory. `completions` and `manpage` print text, never JSON.
