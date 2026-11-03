# proc

Semantic CLI tool for process management. Target by port, PID, name or path.

Find what's on a port, why it's there, and stop it, in one command you'd guess on the first try. On macOS, Linux, Windows, FreeBSD and NetBSD.

```bash
brew install yazeed/proc/proc
```

Also on [npm, cargo, Scoop, Nix, Docker, and as .deb and .rpm files](#install).

<p align="center">
  <img src="https://raw.githubusercontent.com/yazeed/proc-cli/main/demo.gif" alt="proc demo: npm run dev fails because port 3000 is taken; proc on, why and free find the old tmux session holding it and free the port; proc orphans finds and cleans up leftover processes" width="800" />
</p>

A dev server won't start because port 3000 is taken. proc finds the old tmux session holding it, frees the port, then cleans up what else was left running.

[![CI](https://github.com/yazeed/proc-cli/workflows/CI/badge.svg)](https://github.com/yazeed/proc-cli/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Crates.io](https://img.shields.io/crates/v/proc-cli.svg)](https://crates.io/crates/proc-cli)
[![npm](https://img.shields.io/npm/v/proc-cli)](https://www.npmjs.com/package/proc-cli)
[![Docker](https://img.shields.io/docker/v/yazeed/proc-cli?label=docker)](https://hub.docker.com/r/yazeed/proc-cli)
[![Open Collective](https://img.shields.io/opencollective/all/proc-cli?label=backers)](https://opencollective.com/proc-cli)
[![Website](https://img.shields.io/website?url=https%3A%2F%2Fproc-cli.com%2F&label=website)](https://proc-cli.com/)

**Docs:** [commands](https://proc-cli.com/commands) · [install](https://proc-cli.com/install) · [FAQ](https://proc-cli.com/faq) · [agent skill](https://proc-cli.com/skill) · [changelog](CHANGELOG.md)

## One command instead of a pipeline

| Task | proc | Without proc |
|------|------|-------------|
| What's on port 3000? | `proc on :3000` | `lsof -i :3000 -P -n` |
| What is PID 1234, and what does it listen on? | `proc on 1234` | `ps -p 1234 -o pid,ppid,user,etime,command` + `lsof -a -p 1234 -iTCP -sTCP:LISTEN` |
| Why is it there? | `proc why :3000` | `lsof -i :3000` + `ps -o ppid=` chain |
| Kill it | `proc kill :3000` | `lsof -i :3000 -t \| xargs kill -9` |
| Free the port and check | `proc free :3000` | `lsof -i :3000 -t \| xargs kill && sleep 1 && lsof -i :3000` |
| What ports is node using? | `proc on node` | `lsof -i -P -n \| grep node` |
| What runs this program? | `proc for ./server` | `lsof ./server` or `fuser ./server` |
| Node processes in this project | `proc by node --in .` | `ps aux \| grep node` + manual check |
| Pause a process | `proc freeze node` | `kill -STOP $(pgrep node)` |
| Wait for it to exit | `proc wait node` | `while ps aux \| grep node \| grep -v grep; do sleep 5; done` |
| Watch in real time | `proc watch node` | `watch -n2 'ps aux \| grep node'` |
| Find leftovers (not on Windows) | `proc orphans --in .` | No standard tool |

[Full comparison &rarr;](https://proc-cli.com/posts/proc-vs-lsof-fuser-killport-fkill/)

## It asks before it changes anything

- **Looking is free.** `on`, `for`, `by`, `in`, `list`, `info`, `ports`, `tree`, `why`, `stuck` and `orphans` only read.
- **Changing asks first.** `kill`, `stop`, `freeze`, `thaw`, `free`, `unstick`, `orphans --kill` and `stuck --kill` show what they will touch and wait for a yes. `--dry-run` shows the list and does nothing else; `--yes` skips the question.
- **`--json` is not a yes.** A script or agent that runs a changing command with `--json` and no `--yes` gets exit 4, nothing changed, and the list of processes it would have touched. The same goes for running without a terminal.
- **A name means the whole name.** `proc kill node` stops processes named `node` and leaves `Claude Helper` alone. Add `--cmd` for the looser, `pkill -f` style match.
- **A name is a search, not an approval.** Preview a name with `--dry-run`, and use `--yes` only with a `:port` or a PID you have seen.

```bash
proc kill node --in . --dry-run   # what would stop, in this project only
proc kill :3000                   # asks first
proc free :3000 --yes             # a port target: no question
```

## Install

| Platform | Command |
|----------|---------|
| macOS | `brew install yazeed/proc/proc`, or the install script below |
| Linux | `curl -fsSL https://raw.githubusercontent.com/yazeed/proc-cli/main/install.sh \| bash`, or `brew install yazeed/proc/proc` |
| Debian / Ubuntu | Download `proc-cli_X.Y.Z_amd64.deb` (or `_arm64.deb`) from the [latest release](https://github.com/yazeed/proc-cli/releases/latest), then `sudo apt install ./proc-cli_X.Y.Z_amd64.deb` |
| Fedora / RHEL | Download `proc-cli-X.Y.Z-1.x86_64.rpm` (or `.aarch64.rpm`) from the [latest release](https://github.com/yazeed/proc-cli/releases/latest), then `sudo dnf install ./proc-cli-X.Y.Z-1.x86_64.rpm` |
| FreeBSD / NetBSD (x86_64) | `curl -fsSL https://raw.githubusercontent.com/yazeed/proc-cli/main/install.sh \| sh` (curl is a package: `pkg install curl`), or `cargo install proc-cli` on other CPUs |
| Windows | `scoop bucket add proc https://github.com/yazeed/scoop-bucket-proc && scoop install proc` |
| npm / bun | `npm install -g proc-cli` |
| Rust | `cargo install proc-cli` (or `cargo binstall proc-cli` for the pre-built binary) |
| Nix | `nix profile install github:yazeed/proc-cli` |
| Docker | `docker run --rm -it --pid=host --net=host yazeed/proc-cli` |

`--pid=host` lets proc in Docker see the host's processes, and `--net=host` its ports (for `on :port`, `ports` and `free`).

The `.deb` and `.rpm` files install `proc`, its man page and its bash, zsh and fish completions. They come from the release page, not from an apt or dnf repository, so `apt upgrade` and `dnf upgrade` never update them: download the new file for each release.

Check it works:

```bash
proc --version
proc ports
```

<details>
<summary>More options: Arch Linux, manual download</summary>

| Platform | Method | Command |
|----------|--------|---------|
| Arch Linux | AUR (community-maintained), release binary | `yay -S proc-cli-bin` |
| Arch Linux | AUR (community-maintained), built from source | `yay -S proc-cli` |

The AUR packages are not published by this project and can lag behind releases. Check `proc --version` after installing: before 2.0, a name matched any part of a command line and `--json` counted as `--yes`.

**Manual download:**

```bash
# macOS (Apple Silicon)
curl -fsSL https://github.com/yazeed/proc-cli/releases/latest/download/proc-darwin-aarch64.tar.gz | tar xz
sudo mv proc-darwin-aarch64 /usr/local/bin/proc

# macOS (Intel)
curl -fsSL https://github.com/yazeed/proc-cli/releases/latest/download/proc-darwin-x86_64.tar.gz | tar xz
sudo mv proc-darwin-x86_64 /usr/local/bin/proc

# Linux (x86_64)
curl -fsSL https://github.com/yazeed/proc-cli/releases/latest/download/proc-linux-x86_64.tar.gz | tar xz
sudo mv proc-linux-x86_64 /usr/local/bin/proc

# Linux (ARM64)
curl -fsSL https://github.com/yazeed/proc-cli/releases/latest/download/proc-linux-aarch64.tar.gz | tar xz
sudo mv proc-linux-aarch64 /usr/local/bin/proc

# FreeBSD (x86_64); for NetBSD the file is proc-netbsd-x86_64
curl -fsSL https://github.com/yazeed/proc-cli/releases/latest/download/proc-freebsd-x86_64.tar.gz | tar xz
sudo mv proc-freebsd-x86_64 /usr/local/bin/proc

# Windows (PowerShell, run as administrator to write to System32)
Invoke-WebRequest -Uri https://github.com/yazeed/proc-cli/releases/latest/download/proc-windows-x86_64.exe.zip -OutFile proc.zip
Expand-Archive proc.zip -DestinationPath .
Move-Item proc-windows-x86_64.exe C:\Windows\System32\proc.exe
```

</details>

## Nineteen commands, three jobs

Every command takes the same targets, and almost all take `--in` to stay inside one project, `--json` for scripts, and `--dry-run` before anything changes (the changing ones).

### Find out

Read-only. Nothing is touched.

| Command | Alias | What it does |
|---------|-------|--------------|
| `on <target>` | `:` | What's on a port, or which ports a process uses |
| `why <target>` | | Trace what started a process (why is this port busy?) |
| `for <file>` | `f` | Processes running a program or holding a file open |
| `by <name>` | `b` | Processes with this name |
| `in <path>` | | Processes whose working directory is this path |
| `list` | `l`, `ps` | All processes |
| `info <target>` | `i`, `inspect` | Everything about one process |
| `ports` | `p` | All listening TCP ports |
| `tree` | `t` | Parents and children |
| `stuck` | `x` | Hung processes: busy or blocked for 2 seconds, running longer than `--timeout` (default 300) |
| `orphans` | `o` | Processes whose parent has exited (Unix only: exits 4 on Windows) |

### Change it

Asks first, or previews with `--dry-run`.

| Command | Alias | What it does |
|---------|-------|--------------|
| `stop <target>` | `s` | Ask it to exit (SIGTERM, or `--signal`), force-kill after `--timeout` (default 10) |
| `kill <target>` | `k` | End it now (SIGKILL; `--graceful` sends SIGTERM) |
| `free <:port>` | | Stop what holds the port (SIGTERM, SIGKILL after up to 5 seconds) and check the port is free |
| `freeze <target>` | | Pause it (SIGSTOP; Unix only: exits 4 on Windows) |
| `thaw <target>` | | Resume it (SIGCONT; Unix only: exits 4 on Windows) |
| `unstick` | `u` | Try to recover stuck processes; `--force` ends them if recovery fails |

### Keep watching

| Command | Alias | What it does |
|---------|-------|--------------|
| `watch` | `w`, `top` | Live process table (`--json` streams one document per refresh) |
| `wait <target>` | | Block until the process(es) exit |

```bash
proc on :3000                   # what's on port 3000?
proc on 1234                    # what is PID 1234, and what ports is it using?
proc by node --in . --min-cpu 5 # node in this directory using >5% CPU
proc info :3000 1234            # info for a port and a PID
proc tree --min-cpu 5           # process tree, busy processes only
proc watch node -n 1            # node processes, refreshed every second
proc wait :3000 --timeout 60    # wait up to 60s for the process on port 3000 to exit
proc stop worker --signal INT   # ask with SIGINT; force-killed after --timeout
proc orphans --in .             # leftovers from this project
```

## Targets

All commands accept the same targets:

| Target | Example | Matches |
|--------|---------|---------|
| Port | `:3000` | Every process listening on port 3000 |
| PID | `1234` | The process with ID 1234 |
| Name | `node` | Processes named "node" (the whole name, ignoring case) |
| Several | `:3000 :8080 node` | Separate arguments, any mix of the three |

A process named twice is handled once, except by the read-only `on` and `why`, which answer each target under its own header. Comma-separated targets (`:3000,:8080,node`) still work but are deprecated and will be removed in a future major release: a command that gets one prints a `note:` on stderr with the separate-argument form (human output only; `--json` is unchanged).

**Names.** A name matches the whole process name, ignoring case, the way `killall` and `pkill -x` read it. `proc kill node` stops the processes named `node` and leaves `Claude Helper` alone, and `--by` follows the same rule. The file name of the executable and of the first argument count as names too, so a long name that Linux cuts to 15 characters still matches, and on Windows a trailing `.exe` is optional. Add `--cmd` to match any part of the name or the full command line instead, like `pkill -f`: `proc kill serve --cmd`. A path is not a name: `proc kill ./server` finds nothing, so use `--cmd`, or `proc for <file>` for an executable. proc doesn't expand wildcards; an unquoted `node*` is expanded by your shell into file names, so use `--cmd` for partial matches.

**`--in`.** `--in` (short `-i`) takes the next word as its directory unless that word starts with `-`; a bare `--in` is the current directory. Put it after the targets (`proc kill 1234 5678 --in .`) or write `--in=DIR`.

**The process card.** `on` (by port, PID or name), `info`, `for` with one hit and `why` print the same card for a process: `Process:`, `Command:`, `Directory:`, `Path:`, `User:`, `Parent:`, `Status:`, `Resources:`, `Started:`, `Children:`, then `Listening:` (every port, with its exposure) and `Connections:` for a process that listens. `proc on 1234` is the card for a PID, `proc on :3000` the card for each holder of a port.

**Tables.** `list`, `by`, `in`, `for` (several hits), `orphans`, `stuck`, `watch` and the list in front of every `--dry-run` or question print one table: `PID NAME USER CPU% MEM UPTIME COMMAND`. A value proc cannot read is `-`. COMMAND takes the width the other columns leave and ends with `…` where it is cut; the list you approve prints it whole. With `--in`, the directory is the second column. `--verbose` keeps the table, adds `PPID STATUS DIR` and cuts nothing.

**Ports.** `ports` prints `PORT PROTO ADDRESS EXPOSURE PID NAME UPTIME COMMAND` (EXPOSURE is `local` or `exposed`), so each row says what the listener is. `--verbose` adds `USER MEM` and the executable's path (`PATH`) and cuts nothing; `--json` also gives `command`, `user`, `uptime_secs` and `memory_mb` per port, and with `-v` the connection counts.

## Filters

Not every filter is on every command. `--in`, `--by` (short `-b`) and `--cmd` work on nearly all of them, including the ones that change processes. The rest narrow what the finding commands show:

| Filter | Description | On |
|--------|-------------|----|
| `--in [<path>]`, `-i` | Working directory (bare `--in` is the current directory) | nearly every command |
| `--by <name>`, `-b` | Process name (the whole name, like a name target) | nearly every command |
| `--cmd` | Match names and `--by` as a substring of the name or full command line (like `pkill -f`) | every command that takes a name or `--by` |
| `--start-time <s>` | Only the process that started then (`start_time` from its JSON): a PID the OS has since given to a new process exits 2. One PID target | `kill`, `stop`, `freeze`, `thaw`, `wait`, `info`, `unstick` |
| `--path <path>`, `-p` | Executable path | `list` |
| `--min-cpu <n>` | Processes using >n% CPU | `by`, `in`, `for`, `list`, `tree`, `watch` |
| `--min-mem <n>` | Processes using >n MB memory | `by`, `in`, `for`, `list`, `tree`, `watch` |
| `--min-uptime <s>` | Processes running longer than s seconds | `by`, `in`, `for`, `list`, `tree` |
| `--status <s>` | running, sleeping, stopped, zombie | `by`, `in`, `for`, `list`, `tree` |
| `--parent <pid>` | Children of a specific parent PID | `by`, `in`, `list` |
| `--exposed`, `-e` / `--local`, `-l` | Only network-exposed / localhost ports | `ports` |
| `--range <s-e>`, `-r` | Ports in a range (e.g. `3000-9000`) | `ports` |
| `--sort`, `-s <key>` | `list`, `by`, `in`, `for`, `watch`: cpu, mem (biggest first); pid, name (ascending). `ports`: port, pid, name (ascending). Ties come by PID | see left |
| `--limit`, `-n <n>` | Limit the number of results (`-l` on `watch`, where `-n` is `--interval`) | `list`, `by`, `in`, `for`, `ports`, `watch` |
| `--ancestors`, `-a` | Show the ancestry path up to the root | `tree` |
| `--depth`, `-d <n>` | Maximum tree depth (default 10) | `tree` |
| `--compact`, `-C` | PIDs only | `tree` |
| `--timeout`, `-t <s>` | Only processes running longer than this (default 300) | `stuck`, `unstick` |

## Options

| Option | Short | Description |
|--------|-------|-------------|
| `--json` | `-j` | JSON output |
| `--verbose` | `-v` | Print every value whole. In a process table it adds PPID, STATUS and DIR and cuts nothing; in `ports` it adds USER, MEM and PATH (JSON: `exe_path` and `connections`) |
| `--wide` | `-w` | The directory and the command line whole, instead of fitting the terminal (`list`, `by`, `in`, `for`, `stuck`, `orphans`, `watch`) |
| `--yes` | `-y` | Skip confirmation; required with `--json` or without a terminal, where nobody can be asked |
| `--dry-run` | | Preview without executing |
| `--force` | `-f` | Force termination if recovery fails (`unstick`) |
| `--graceful` | `-g` | Send SIGTERM instead of SIGKILL (`kill`) |
| `--signal` | `-S` | First signal: HUP, INT, USR1, etc. (`stop`) |
| `--kill` | `-k` | Kill the processes found (`stuck`, `orphans`) |
| `--wait` | | Max seconds to wait for the port to free (`free`, default 10) |
| `--interval` | `-n` | Seconds between status lines (`wait`, default 5) or refreshes (`watch`, default 2) |
| `--timeout` | `-t` | Max seconds before giving up (`wait`, default 0 = no limit; `stop`, default 10 before force-kill) |
| `--quiet` | `-q` | Suppress the periodic still-running lines (`wait`) |

## For scripts and coding agents

Every process and port command prints JSON with `--json`: one document per command, with `action` (the command name) and `success`. `proc watch --json` streams one per refresh. Exit codes always mean the same thing.

```bash
proc on :8080 --json | jq '.process | {pid, cwd, uptime_secs}'
```

- **Errors are JSON too**, including invalid flags: `{"action", "success": false, "error", "exit_code", "reason"}`. `"success": false` always comes with a non-zero exit code.
- **Every error says why in `"reason"`**, one fixed word to branch on: `"system_error"` (exit 1); `"not_found"`, `"command_line_only"` (the name is only in command lines; `--cmd` matches) or `"ruled_out"` (a guard ruled the process out, listed in `"ruled_out_by"`) for exit 2; `"permission_denied"` (exit 3); and, for exit 4, where nothing was done, `"invalid_input"` (a wrong flag, value or argument), `"needs_confirmation"` or `"not_supported"` (this OS cannot do it).
- **Preview, then confirm.** A command that changes processes, run with `--json` but without `--yes`, exits 4 with `"needs_confirmation": true` and `"processes"` (the ones it would have changed), so a script can show them before running it again with `--yes`.
- **`not_found`** lists the targets that matched nothing (empty when all were found). It is in the JSON of `on`, `why`, `info`, `kill`, `stop`, `freeze`, `thaw`, `unstick`, `free` and `wait`, and in the exit 4 output above. `list` and `watch` do not have it.
- **A prompt shows what you would approve.** The list of processes goes to the terminal beside the `[y/N]` question: on stdout, or on stderr when stdout is a file or a pipe. If the list cannot be shown, nothing is asked and nothing is changed (exit 4).
- **A reader that stops early ends proc quietly.** `proc list | head -n1` exits 0 without an error, and `proc kill 1234 --yes | head -n1` still kills the process and exits with the code of the kill (0 if it worked, 2 if there was nothing to kill): a closed pipe never changes the result, on stdout or stderr. A write that fails for another reason (a full disk) is exit 1, with one line on stderr.

| Exit code | Meaning |
|-----------|---------|
| `0` | Success |
| `1` | Error, or not fully successful: a process failed, a port stayed busy, `wait` timed out |
| `2` | Process or port not found |
| `3` | Permission denied: proc itself got an I/O permission error. A signal the OS refuses (another user's process) is a `failed` entry and exit 1 |
| `4` | Invalid input (unknown flag, bad argument, invalid signal), an action this OS doesn't support (`freeze`, `thaw`, `orphans` and `stop --signal` on Windows), or a command that changes processes run without `--yes` where nobody can confirm (`--json`, or no terminal): nothing was changed |
| `129`, `130`, `131`, `143` | A signal ended proc (SIGHUP, SIGINT or Ctrl+C, SIGQUIT, SIGTERM) while `watch` had the screen or a prompt was waiting: the terminal is put back first, then proc ends by that signal, so a shell shows 128 plus its number and Ctrl+C stops a script's loop. Nothing was changed by an unanswered prompt |

Full schemas: [`skills/proc-cli/reference.md`](skills/proc-cli/reference.md).

### Agent skill

Teach your coding agent (Claude Code, Codex, Cursor, Gemini CLI and others) when to reach for proc and how to use it safely, with structured JSON output:

```bash
npx skills add yazeed/proc-cli
```

In Claude Code you can also install it as a plugin:

```text
/plugin marketplace add yazeed/proc-cli
/plugin install proc-cli@proc-cli
```

The skill lives in [`skills/proc-cli/`](skills/proc-cli/SKILL.md) and follows the [Agent Skills](https://agentskills.io) standard. [What it teaches, how to update it, and prompts to try &rarr;](https://proc-cli.com/skill)

[![Skills installs](https://skills.sh/b/yazeed/proc-cli)](https://www.skills.sh/yazeed/proc-cli)
[![SafeSkill](https://safeskill.dev/api/badge/proc-cli)](https://safeskill.dev/scan/proc-cli)
[![Socket](https://badge.socket.dev/npm/package/proc-cli)](https://socket.dev/npm/package/proc-cli)

## Platform support

| Platform | Architecture | Status |
|----------|--------------|--------|
| macOS | Apple Silicon (ARM64), Intel (x86_64) | ✅ |
| Linux | x86_64, ARM64 | ✅ |
| Windows | x86_64 | ✅ (no `freeze`, `thaw`, `orphans`, `stop --signal`) |
| FreeBSD | x86_64 (other CPUs: `cargo install proc-cli`) | ✅ (see below) |
| NetBSD | x86_64 (other CPUs: `cargo install proc-cli`) | ✅ (see below) |
| Docker | linux/amd64, linux/arm64 | ✅ |

The Linux binaries are statically linked (musl) and need no particular glibc version, so they also run on Alpine and on older releases such as Debian 10, Ubuntu 20.04 and Amazon Linux 2.

On FreeBSD and NetBSD proc reads ports with `sockstat` and open files with `fstat`, both in the base system, so it needs nothing installed. The kernel forgets the path of a working directory once the directory is deleted, so `proc in` a deleted directory exits 4 there, where Linux and macOS still find the processes in it.

proc is for processes and ports on the machine it runs on. It is not for containers, system services or remote hosts (see [PHILOSOPHY.md](PHILOSOPHY.md)).

## Shell completions and man page

The Nix package and the `.deb` and `.rpm` files install both. With any other install method, add them yourself:

```bash
# Bash
proc completions bash > /etc/bash_completion.d/proc

# Zsh
proc completions zsh > ~/.zsh/completions/_proc

# Fish
proc completions fish > ~/.config/fish/completions/proc.fish

# Elvish and PowerShell: proc completions elvish, proc completions powershell

# Man page
proc manpage | sudo tee /usr/local/share/man/man1/proc.1 > /dev/null
```

## Building from source

```bash
git clone https://github.com/yazeed/proc-cli
cd proc-cli
cargo build --release
```

The binary will be at `target/release/proc`.

## Use as a library

proc is also a Rust crate, `proc_cli`: see the [API docs on docs.rs](https://docs.rs/proc-cli) and the `Library API` entries in the [changelog](CHANGELOG.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The principles behind the commands are in [PHILOSOPHY.md](PHILOSOPHY.md).

## Star History

[![Star History Chart](https://api.star-history.com/chart?repos=yazeed/proc-cli&type=date&legend=top-left)](https://www.star-history.com/?repos=yazeed%2Fproc-cli&type=date&legend=top-left)

## License

MIT
