# proc reference

Everything here is for lookup. The safety model, the playbooks and the exit-code actions are in [SKILL.md](SKILL.md); read them first.

## Contents

- [Exit codes](#exit-codes)
- [Errors and needs_confirmation](#errors-and-needs_confirmation)
- [Commands and flags](#commands-and-flags)
- [Compatibility](#compatibility)
- [Target and filter semantics](#target-and-filter-semantics)
- [Order of results](#order-of-results)
- [Process and port objects](#process-and-port-objects)
- [Output shapes](#output-shapes)
- [Names deprecated in 2.3.0](#names-deprecated-in-230)
- [Heuristics: stuck and orphans](#heuristics-stuck-and-orphans)
- [Edge cases](#edge-cases)
- [Platform differences](#platform-differences)
- [Signals](#signals)
- [Older versions](#older-versions)

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success. A dry run that found its targets is also 0. |
| 1 | The command ran but did not fully succeed: a process failed (listed in `failed`), a port stayed busy, `wait` timed out. Also unexpected system errors. |
| 2 | Not found: no target matched. |
| 3 | Permission denied: proc itself got an I/O permission error, or a `:port` target has a holder proc can't see (see `permission_denied` below). It is not what a refused signal produces (see below). |
| 4 | Invalid input (unknown flag, missing argument, bad signal name, missing directory or file), an action this OS does not support (`freeze`, `thaw`, `orphans` and `stop --signal` on Windows), or `needs_confirmation`. |

A signal the OS refuses, such as `kill` on another user's process, is not exit 3. It is a `failed` entry for that process with `error` text such as `Signal failed: Failed to kill process 123` (SIGKILL) or `Signal failed: EPERM: Operation not permitted` (other signals), and the exit code is 1. If the process is gone or its PID was reused, the entry reads `No process found matching '123'`. Treat any unexpected `failed` entry as a process that is not yours: stop and ask the user.

`"success": false` always comes with a non-zero exit code. Every command prints exactly one JSON document with `--json`, except `watch`, which streams one per refresh. `--json` exists on the 19 commands only: `--help` and `--version` print text even with `--json`, and `completions` and `manpage` reject it (exit 4, `invalid_input`).

A signal that ends proc (SIGINT, SIGTERM, SIGHUP, SIGQUIT, or Ctrl+C / Ctrl+Break on Windows) while `watch` has the screen or a prompt is waiting for a key is not one of these codes and not an error: proc puts the terminal back (raw mode off, cursor shown, normal screen, echo and line editing on), prints nothing, and ends by that same signal, as a program without a handler would. A shell shows 128 plus its number: 129 SIGHUP, 130 SIGINT and Ctrl+C, 131 SIGQUIT, 143 SIGTERM. A program that ran proc directly sees it ended by signal 1, 2, 3 or 15 instead of an exit code (Python's `returncode` is minus the number). Windows has no signals: there every Ctrl+C or Ctrl+Break exits with 130. A kill still running when the signal came is not undone: the prompt had not been answered, so nothing had been changed.

A prompt (the `[y/N]` question of `kill`, `stop`, `freeze`, `thaw`, `free`, `unstick`, `stuck --kill` and `orphans --kill` without `--yes`) always comes with the list of what would change, shown where the person will see it: on stdout when that is a terminal, on stderr when stdout is a file or a pipe. If the list cannot be written there, proc asks nothing and changes nothing: exit 4, `reason: needs_confirmation`, with the `processes` it would have changed. An agent never needs to answer a prompt: use `--dry-run --json`, then `--yes`.

`watch` without `--json` draws a screen only when both stdin and stdout are terminals; otherwise it prints one snapshot (with a `note:` on stderr when only stdin is missing). `watch --json` stops as soon as its reader leaves on Linux, macOS and the BSDs (on Windows at the next refresh). `wait` is different on purpose: its exit code (0 exited, 1 timed out) is its result, so it keeps waiting after its reader leaves and exits with that code.

If the reader of proc's output goes away (`proc ... | head -n1`), proc stops writing and exits with the code the command's result calls for: a `kill --yes` that worked is still 0 and a not-found is still 2, and nothing is said about the pipe. The action is done either way, so when stdout may be cut short, read the exit code, not the document. A write that fails for another reason (a full disk) is exit 1 with `proc: cannot write to stdout: ...` on stderr.

## Errors and needs_confirmation

On failure with `--json`, stdout carries the five fields below. `action` is the canonical command name even when an alias was used (`proc k` gives `"kill"`); for an unknown command it is the word typed (`proc frobnicate --json` gives `"frobnicate"`), and with no command it is `"proc"`. `reason` says why in one fixed word (see below). Unknown flags and arguments produce the same document with exit 4. Without `--json` the message goes to stderr.

```json
{
  "action": "kill",
  "success": false,
  "error": "No process found matching '99999999'\n  Try: proc list to list all processes",
  "exit_code": 2,
  "reason": "not_found"
}
```

`on`, `by` and `why` say so when a name matches no process name but appears in some command lines: `reason` is `command_line_only` and the error text says `2 processes have 'serve' in their command line (add --cmd to match those)`. The commands that change processes (and `wait`, `info`, `tree`) report `not_found`; run `proc by <name> --json` to see the hint.

A command that changes processes (`kill`, `stop`, `freeze`, `thaw`, `free`, `unstick`, `orphans --kill`, `stuck --kill`) run with `--json` or with no terminal, and without `--yes`, changes nothing and exits 4:

```json
{
  "action": "stop",
  "success": false,
  "error": "stop changes processes, so it needs --yes, with --json too (or --dry-run to preview)",
  "exit_code": 4,
  "needs_confirmation": true,
  "processes": [<Process>, ...],
  "not_found": ["99999999"]
}
```

`processes` is what a retry would touch, with CPU measured. Only this case has a `needs_confirmation` key, and it is always `true`: every other error, exit 4 included, has no such key. To tell the exit-4 cases apart, read `reason` (next section) instead; `needs_confirmation` stays for agents written against 2.0.0. `not_found` is on commands that take targets (`kill`, `stop`, `freeze`, `thaw`, `unstick`, `free`) and is `[]` when every target was found. `orphans` and `stuck` take no targets and have no `not_found`. If every target is missing, the result is the plain exit 2 error with no `not_found` (the `error` text names them). With a terminal and no `--json`, the command asks `[y/N]` instead.

How to respond is in SKILL.md, rule 6: show `processes` to the user, and re-run with `--yes` only after approval, by the PIDs you showed.

### reason

Every error carries `reason`, one fixed word: branch on it, never on the text of `error`. A result that did not fully succeed (a kill with `failed` entries, `freed: false`, a `wait` that timed out) is not an error and has none. Exit 4 always means nothing was done:

| Exit | `reason` | Meaning | What to do |
|------|----------|---------|------------|
| 1 | `system_error` | proc itself could not do its part, such as reading the process or port table. | Report it; retrying may work. |
| 2 | `not_found` | No process holds that PID, name or port (any more). `info` keeps its document (`count: 0`) and adds `reason` to it (see info). | It is gone, or never was. |
| 2 | `command_line_only` | No process has that name, but some have it in their command line. | Add `--cmd` if a command-line match is what you meant. |
| 2 | `ruled_out` | A process holds the target, but a guard rules it out. `ruled_out_by` lists the guards' flags (`["--start-time"]`, or several: `["--by", "--start-time"]`), and `error` names them with their values. Or every process a name matched is one proc runs under, which the command leaves out: `left_out` lists them (`[{"pid", "name"}]`), and there is no `ruled_out_by`. Branch on these fields; the text is for the user. | `--start-time` among them: your process is gone and a new one has the PID. `--by` or `--in` alone: check the PID with `proc info` before treating it as gone. `left_out`: the name matched your own shell or agent; leave it alone. |
| 3 | `permission_denied` | proc itself got an I/O permission error, or a `:port` target has a holder proc can't see: another user's process, when proc isn't root (Linux and macOS; `error` says "is in use, but proc can't see which process holds it"), or, on FreeBSD and NetBSD set to hide other users' sockets, possibly one (`error` says "proc can't tell whether port N is in use"). The port is not known to be free. | Ask the user; `sudo` sees the holder. Never treat the port as free. |
| 4 | `invalid_input` | A flag, value or argument is wrong: an unknown flag (`kill --name node`), a bad value (`--sort bogus`, `--signal BOGUS`), a missing argument, a directory or file that does not exist. | Fix the command from `error`. Retrying unchanged fails the same way. |
| 4 | `needs_confirmation` | The command changes processes and did not get `--yes`. The error also has `needs_confirmation: true` and `processes`. | SKILL.md, rule 6. |
| 4 | `not_supported` | This OS cannot do it: `freeze`, `thaw`, `orphans` and `stop --signal` on Windows. | Do not retry; use another approach. |

```json
{
  "action": "kill",
  "success": false,
  "error": "unexpected argument '--name' found",
  "exit_code": 4,
  "reason": "invalid_input"
}
```

For a usage error, `error` is clap's whole message on one line: its lines joined with a space, without the `Usage:` block, the `--help` pointer and the `tip:` lines. A missing argument reads `the following required arguments were not provided: <TARGET>...` and a bad value reads `invalid value 'bogus' for '--sort <SORT>' [possible values: cpu, mem, pid, name]`. A suggestion or hint follows on its own indented line after `\n`: `\n  Try: did you mean '--json'?` for a mistyped flag, `\n  Try: did you mean 'kill'?` for a subcommand or value (up to three names, closest first: `'free' or 'freeze' or 'for'`), or, when a command that needs targets got `--by` or `--in` and no target, `\n  kill needs at least one target; --by filters the targets, it doesn't select them`. An argument that is not valid UTF-8 is `invalid_input` too, with the bad bytes shown as U+FFFD in `error`.

A target its guards ruled out:

```json
{
  "action": "stop",
  "success": false,
  "error": "No process found matching '1234' (ruled out by --start-time 1700000000)\n  Try: proc list to list all processes",
  "exit_code": 2,
  "reason": "ruled_out",
  "ruled_out_by": ["--start-time"]
}
```

## Commands and flags

Aliases: `on` `:`; `for` `f`; `by` `b`; `list` `l` `ps`; `info` `i` `inspect`; `ports` `p`; `kill` `k`; `stop` `s`; `tree` `t`; `watch` `w` `top`; `stuck` `x`; `unstick` `u`; `orphans` `o`. JSON `action` is always the full name (`inspect` gives `"info"`). Every command takes `--json`/`-j` and `--verbose`/`-v`.

| Command | Takes | Does |
|---------|-------|------|
| `on` | targets | Port to process (`:3000`), or process to ports (PID, name). |
| `for` | one file | Processes running this file, or holding it open. |
| `by` | one name | Processes with this name. |
| `in` | one directory | Processes whose cwd is under the directory. |
| `list` | optional names | All processes, or those named. |
| `info` | targets | Full detail for each target. |
| `ports` | none | Listening TCP ports, each with its holder's name, uptime and command. |
| `why` | targets | Ancestry of a port holder or process. |
| `tree` | optional target | Descendants (default) or `--ancestors`. |
| `watch` | optional targets | Refreshing table, or NDJSON with `--json`. |
| `wait` | targets | Block until the targets exit. Default `--timeout 0` means forever, so always set one. |
| `stuck` | none | Processes that look hung. `--kill` acts on them. |
| `orphans` | none | Processes reparented to PID 1. `--kill` acts on them. Unix only: exits 4 on Windows. |
| `kill` | targets | SIGKILL. `--graceful` sends SIGTERM instead. |
| `stop` | targets | SIGTERM (or `--signal`), then SIGKILL after `--timeout` (default 10). All targets are asked at once and share the timeout. |
| `freeze`, `thaw` | targets | SIGSTOP, SIGCONT. Unix only. |
| `free` | ports only | Stops every holder (SIGTERM, SIGKILL after at most 5 s), then waits up to `--wait` seconds (default 10) for the port to be free. |
| `unstick` | optional targets | SIGCONT, then SIGINT. `--force` adds SIGTERM, then SIGKILL. With no target it finds stuck processes first. |

Changing commands (`kill`, `stop`, `freeze`, `thaw`, `free`, `unstick`, and `orphans`/`stuck` with `--kill`) take `--yes`/`-y` and `--dry-run`. `orphans --dry-run` without `--kill` is accepted and changes nothing.

Filters (a command takes only those its `--help` lists):

| Filter | Commands |
|--------|----------|
| `--in [dir]` (`-i`; bare means `.`): the directory and every folder below it | all except `why`, `free`, `in` |
| `--by <name>` (`-b`) | all except `why`, `free`, `by`, `list` |
| `--cmd` (substring of name or command line) | every command that takes a name or `--by` |
| `--start-time <secs>` (one PID target; the `start_time` from its JSON) | `kill`, `stop`, `freeze`, `thaw`, `wait`, `info`, `unstick` |
| `--min-cpu <pct>`, `--min-mem <MB>` | `list`, `by`, `in`, `for`, `tree`, `watch` |
| `--status <s>` (running, sleeping, stopped, zombie, idle, starting, disk-wait), `--min-uptime <secs>` | `list`, `by`, `in`, `for`, `tree` |
| `--parent <pid>` | `list`, `by`, `in` |
| `--path <path>` (`-p`) | `list` |
| `--sort` (`-s`): cpu, mem (biggest first), pid, name (`ports`: port, pid, name; ascending) | `list`, `by`, `in`, `for`, `watch`, `ports` |
| `--limit <n>` (`-n`; `-l` on `watch`) | `list`, `by`, `in`, `for`, `ports`, `watch` |
| `--wide` (`-w`) | `list`, `by`, `in`, `for`, `watch`, `stuck`, `orphans` |
| `--exposed` (`-e`; 0.0.0.0, `::`), `--local` (`-l`; 127.0.0.1, `::1`), `--range <from>-<to>` (`-r`) | `ports` |
| `--depth <n>` (`-d`; default 10), `--compact` (`-C`), `--ancestors` (`-a`) | `tree` |
| `--interval <secs>` (`-n`) | `watch`, `wait` (`wait --quiet` hides the periodic still-running lines) |
| `--timeout <secs>` (`-t`) | `stop` (default 10), `wait` (0 = forever), `stuck` and `unstick` (default 300) |

Flags of the commands that change processes:

| Flag | Commands |
|------|----------|
| `--yes` (`-y`), `--dry-run` | `kill`, `stop`, `freeze`, `thaw`, `free`, `unstick`, and `orphans`/`stuck` with `--kill` |
| `--kill` (`-k`) | `stuck`, `orphans` |
| `--signal <name>` (`-S`) | `stop` |
| `--graceful` (`-g`) | `kill` |
| `--force` (`-f`) | `unstick` |
| `--wait <secs>` (default 10) | `free` |
| `--quiet` (`-q`) | `wait` |

Short flags differ in two places: `-l` is `--limit` on `watch` but `--local` on `ports`, and `-n` is `--limit` everywhere else but `--interval` on `watch` and `wait`. Scripts and agents should use the long forms.

### Values

| Flag | Accepts | Rejected (exit 4, `invalid_input`) |
|------|---------|-------------------------------------|
| `--timeout`, `--wait`, `--min-uptime`, `--start-time`, and `--interval` on `wait` | whole seconds, `0` or more | a fraction (`1.5`) or a unit (`1m`): `invalid digit found in string` |
| `--interval` on `watch` | seconds, fractions allowed, at least `0.1` | `0.05`: `must be at least 0.1 seconds` |
| `--min-cpu` | percent of one core, fractions allowed (over 100 with several cores) | text |
| `--min-mem` | megabytes, fractions allowed | text |
| `--limit`, `--depth`, `--parent` | a whole number (`--parent` is a PID) | a fraction |
| `--range` | `START-END`, from 1 to 65535 (`9000-8000` is refused with a `did you mean 8000-9000?`) | `0-10`, `70000-80000`, `8000` |
| `--signal` | `HUP`, `INT`, `QUIT`, `ABRT`, `KILL`, `TERM`, `STOP`, `CONT`, `USR1`, `USR2`, any case, with or without `SIG` | a number (`10`) or another name (`WINCH`) |
| `--status` | `running`, `sleeping`, `stopped`, `zombie`, `idle`, `starting`, `disk-wait` (`disk_wait` too) | `dead` and `unknown`, which only appear in output |
| `--sort` | see the table above | another word, with the list of valid ones |

A number with a leading `-` is read as a flag: `--timeout -1` exits 4 with `unexpected argument '-1' found`.

### Flags that change only the human output

`--verbose`, `--wide` and `--compact` change what a person sees, never the JSON, with one exception: `ports -v` adds `exe_path` and `connections` to each port. `wait -v` measures `cpu_percent` for the lines it prints. Every other flag filters or acts, so it changes the JSON too (`tree --depth` cuts the tree at that depth).

## Compatibility

What a release may change in the JSON, so a script knows what to rely on:

- **Within a major version (2.x),** keys are added, never removed or given another meaning, and `reason` words keep their meaning. A key that is replaced stays beside its new name until the next major version, listed under "Names deprecated" below. An exit code changes only to fix a wrong answer, and the CHANGELOG says so in bold (2.4.0: a port another user's process holds became exit 3, where 2 had called it free).
- **Words may be added** to the sets a value is drawn from (`status`, `reason`, `outcome`, `exposure`). Handle a word you don't know as "other", not as an error.
- **Key order is not promised.** Errors print their keys in alphabetical order; other documents keep a fixed order, but read keys by name.
- **Text is for people**: `error`, `message`, `elapsed_human`, each `failed[].error` and `processes[].error`, and the human output. Its wording may change in any release. Branch on `reason`, `exit_code`, `count` and the other fields.

## Target and filter semantics

- **Names** equal the process name, the file name of its executable, or the file name of argv[0], ignoring case. `node` matches `node` and not `Claude Helper`. On Windows `.exe` is optional on either side. An empty name matches nothing. Exact does not mean yours: on one laptop `proc kill node --dry-run` still listed 185 processes from many projects.
- **`--cmd`** compares the text as a substring of the name or the full command line, like `pkill -f`. On the same laptop `node --cmd` listed 195, `python --cmd` 48 against 12 exact, and `nc --cmd` 379 against 4. Use it only after a dry run.
- **A path is not a name.** `./server` and `/usr/bin/node` find nothing. Use `--cmd`, or `proc for <file>` for an executable.
- **proc never matches itself. A process proc runs under** (its parent, up to 10 levels: your shell, your agent) **has `"runs_proc": true`** in every process object, in every command; the key is absent on every other process. A name (or `--by`) that matches one: the commands that only read (`by`, `list`, `in`, `info`, `why`, `on`, `tree`, `watch`) list it, marked, with the human line `ℹ Includes zsh (PID 81340), which runs this proc`; `--cmd` (under which such a process counts only when its name matches, or its executable or argv[0] exactly: a hit in its command line alone is proc's own arguments, `sh -c 'proc by --cmd X'`, and is no match), the commands that act (`kill`, `stop`, `freeze`, `thaw`, `unstick`) and `wait` leave it out and name it, `⚠ Left out: zsh (PID 81340), which runs this proc`, and the document gets `left_out: [{"pid", "name"}]` (absent when nothing was left out). With every match left out the error is exit 2, `reason: "ruled_out"`, with `left_out`: `sshd (PID 1329) matches 'sshd', but proc runs under it, so it's left out`. PID and port targets are not protected: `proc kill <pid> --dry-run` on your own agent host lists it, with `runs_proc: true`.
- **`:3000`** means every process listening on that port, workers sharing a socket included. A PID or name means all of that process's ports for `on`.
- **Several targets** are separate arguments, deduplicated by PID (a process named by port and PID is handled once). `on` and `why` answer each target under its own entry. `by`, `tree`, `in` and `for` take one value. Commas (`:3000,:8080`) still split a target but are deprecated and will be removed in a future major release. A command that gets one prints a `note:` on stderr in human mode only (never with `--json`, whose output is unchanged). Use them only to read old scripts.
- **`--in`** matches the process's current working directory, including subdirectories, not prefixes (`/a/t1` does not match `/a/t1x`). Paths resolve through symlinks. A broad directory scopes nothing (`/` kept all 185 `node` processes). Processes with no readable cwd never match. A process whose cwd is `/` matches only `--in=/`. It takes the next word as its value unless that starts with `-`; `proc kill --in 123 456` reads `123` as the directory, so write targets first or `--in=<dir>`.
- **`--in` with a missing directory:** every command with `--in` exits 4 (`reason: invalid_input`, `Directory not found`, or `Not a directory` for a file), `on` included. A deleted directory that processes still run in is not missing: it finds them, so leftovers can be cleaned up, also through a symlinked spelling such as `/tmp/x` on macOS. A relative `--in` when the current directory has been deleted exits 4 (`the current directory no longer exists`). `--in` compares the real directory, so a name shown with an escaped control character (`a\nb`) still matches only its own directory.
- **An empty argument** exits 4 on every command: a target (`""`, `","`: `a target can't be empty`), a name, a path, or a flag's value (`--in ""`, `--in=`). An unset `"$DIR"` is an error, never the current directory. `tree <name>` that matches nothing exits 2, and `tree -a` with no target exits 4.
- **A PID with `--start-time <its start_time>`, `--in <its cwd>` or `--by <its name>`** is a guard: a mismatch exits 2 with the PID in the error, which names the guards that ruled it out: `No process found matching '1234' (ruled out by --by node)`. `--start-time` is the exact one (a new process on a reused PID can have the same name and directory): it takes one PID target (anything else exits 4), matches to within a second, and rules out a process whose start time can't be read (on macOS, another user's). A PID that no process holds reads `No process found matching '1234'` with nothing after it. The same holds for `kill`, `stop`, `freeze`, `thaw`, `wait`, `info` and `unstick` with targets, and for `--in` and `--by` on `on` and `tree`. The JSON has `reason: "ruled_out"` and `ruled_out_by`, the guards' flags (`["--by"]`; with two guards failing, `["--by", "--start-time"]` and `ruled out by --by node and --start-time 1700000000` in `error`).
- **Plain patterns** for `list` are exact names (or substrings with `--cmd`), never regular expressions.

## Order of results

The same machine gives the same order on every run, so a list can be compared with an earlier one and `--limit` returns the same rows.

- **Processes** from a name, `list`, `by`, `in`, `for`, `orphans`, `stuck` and `tree` start PID ascending.
- **`--sort`** (`list`, `by`, `in`, `for`, `watch`): `cpu` and `mem` put the biggest first, `pid` and `name` go ascending. Ties come by PID.
- **Ports** (`ports`): port ascending, ties by PID. `--sort pid` and `--sort name` break ties by port, then PID. A process's `ports` array (`on`, `for`, `why`) is ascending, and the holders of one port are by PID.
- **The list a command asks you to approve** (`kill`, `stop`, `freeze`, `thaw`, `free`, `unstick`, `orphans --kill`, `stuck --kill`) is the targets in the order typed, then PID ascending within each target. A process that two targets name is listed once, under the first. The preview, the prompt, `--dry-run --json` `processes`, the `processes` of `needs_confirmation` and the receipt (`processes`, `succeeded`, `failed`) all use this order, `stop`'s `failed` included.
- **`tree`**: roots and every parent's children are PID ascending, in the human tree and in `--json` alike. With `--ancestors` the targets of a name are PID ascending.

## Process and port objects

```json
{
  "pid": 1234,
  "name": "node",
  "exe_path": "/usr/local/bin/node",
  "cwd": "/home/user/project",
  "command": "node server.js --port 3000",
  "cpu_percent": 12.5,
  "memory_mb": 256.0,
  "status": "running",
  "user": "alice",
  "parent_pid": 1000,
  "start_time": 1700000000,
  "uptime_secs": 3600,
  "parent_name": "zsh",
  "cpu_time_secs": 42,
  "open_files": 23,
  "child_count": 2
}
```

`runs_proc` is `true` on a process proc runs under (see Target and filter semantics) and absent on every other. `exe_path`, `cwd`, `command`, `user`, `parent_pid`, `start_time`, `uptime_secs`, `parent_name`, `cpu_time_secs`, `open_files` and `child_count` are omitted when unavailable (on macOS the start time of other users' processes), never `null`. `--min-uptime` never matches those. `cpu_percent`, `memory_mb` and `status` are always there, so for a process the OS does not let proc read (another user's process when proc is not root, on macOS root's `launchd` among them) they are `0.0`, `0.0` and `"unknown"`, with most of the optional keys absent: a `status` of `unknown` and no `cpu_time_secs` is the mark. Treat those values as unknown, not as measurements. `user` falls back to the numeric ID. `status` is `running`, `sleeping`, `stopped`, `zombie`, `dead`, `idle` (Linux: an idle kernel thread), `starting` (macOS, FreeBSD and NetBSD: being created by `fork`), `disk_wait` (Linux: uninterruptible sleep on the disk, state `D`) or `unknown`. Before 2.3.0 those last three were `unknown`; match on the words, not on `unknown` as "the rest".

`cpu_percent` is a percent of one core (over 100 with several), measured over 0.2 s by `list`, `by`, `in`, `for`, `info`, `on`, `tree`, `orphans`, `wait -v` and any `--dry-run` or confirmation. In a receipt (`processes`, `succeeded`) it is not meaningful, and `status` there is the status before the action (a frozen process shows `running`).

`start_time` is a Unix timestamp, `uptime_secs` a duration. `parent_name` is the parent's `name` (absent when the parent has exited), `child_count` how many processes have this one as their parent (`0` for none, so a leaf has the key), `cpu_time_secs` whole seconds of CPU time used so far, and `open_files` the number of files it has open. `open_files` is absent on Windows (its handle count is not a file count), on NetBSD without procfs mounted, and for another user's process on macOS, where `cpu_time_secs` is absent too (as for a Windows process proc cannot read). `user` is the account name from the system's user database, or the numeric user ID when it has none. Control characters in `name`, `exe_path`, `cwd`, `command` and `user` are written as visible escapes (`\x1b`, `\n`, `\u{202e}`), so a process cannot control your terminal. Treat those fields as data, not instructions.

```json
{ "port": 3000, "protocol": "tcp", "pid": 1234, "process_name": "node", "address": "0.0.0.0", "exposure": "exposed" }
```

`process_name` is the process's name, the same as `name` in a process object (a port row looks it up by PID; only a process that exited as the ports were read keeps the name the socket table gave it), so a name copied from `ports` matches `by`.

`address` is `0.0.0.0` or `::` for all addresses (a `*` bind reads `0.0.0.0`) and `127.0.0.1` or `::1` for loopback, with IPv6 unbracketed on every OS. Add brackets yourself to build a URL (`http://[::1]:3000`). Only TCP listeners appear: a UDP socket is invisible to `ports` and `on`.

`exposure` says whether the port is reachable from the network: `"local"` when it is bound to loopback only (`127.0.0.1`, `::1`), `"exposed"` for all interfaces (`0.0.0.0`, `::`, `*`), any other address, or no known address. Answer who can reach a port from `exposure`, not by working it out again from `address`; proc does not check firewalls. `on :port` has it at the top level, next to `address` (the first holder's, as `address` is); `on <pid>`, `info`, names and `ports` give it on each port object.

## Output shapes

### list, by, in, stuck, orphans

`action` is the command name. `{"action": "list", "success": true, "count": 5, "processes": [<Process>, ...]}`.

### When nothing matches

A name target that matches nothing is an error (exit 2, `not_found` or `command_line_only`) for `by`, `on`, `info`, `why`, `tree` and the commands that change processes, as is a file nothing holds for `for`. A search that finds nothing is not: it is exit 0 with `count: 0`, in a shape that differs per command:

| Command | Nothing found (exit 0) |
|---------|------------------------|
| `list <name>`, `in <dir>` | `{"action", "success": true, "count": 0, "processes": []}` |
| `stuck` | `{"action": "stuck", "success": true, "count": 0, "message": "No stuck processes found (threshold: 300s)"}`, no `processes`; `--kill` and `--dry-run` give the same |
| `orphans` | `{"action": "orphans", "success": true, "count": 0, "message": "No orphaned processes found"}`, no `processes`; `--kill` and `--dry-run` give the same |
| `ports` (a `--range` or a filter with no listener) | `{"action": "ports", "success": true, "count": 0, "ports": [], "unseen_ports": []}` |
| `tree --in <dir>` | `{"action": "tree", "success": true, "tree": []}` |
| `unstick` (nothing stuck) | the usual document, with `processes: []` and every count `0` |
| `free :port` (nobody holds it) | `{"action": "free", "success": true, "count": 0, "message": "All specified ports are already free", "not_found": [":port"]}`, with or without `--dry-run`: no `dry_run` and no `processes` |
| `watch <name>` or `watch :<port>` | `{"action": "watch", "success": true, "count": 0, "processes": []}` every refresh, until a match appears: bound it yourself. `watch <pid>` with no PID held, or with `--in`/`--by` ruling out every PID, is the exit 2 error instead |

`message` is text for people: branch on `count`.

### info

```json
{ "action": "info", "success": true, "found_count": 1, "not_found_count": 0, "processes": [<Process> + "ports": [<Port>, ...], ...], "not_found": [], "count": 1 }
```

Each process object has the process's keys and, beside them, `ports`: the ports it listens on (an empty list for none; the key is absent when the OS gave no port table, for example macOS without `lsof` on the PATH, and for another user's process when proc does not run as root, because proc cannot see its sockets, so no ports found is no answer), each with `exposure` and `connections` (`total` and `by_state` only, no `peers`; absent where the OS reads none, as on FreeBSD and NetBSD), as for `on <pid>`.

`count` is the number of processes found, as in every other command; `found_count` is the same number, deprecated, use `count`; removed in 3.0. `not_found_count` stays (it counts `not_found`).

No target found: exit 2 with `success: false`, `count: 0` (and `found_count: 0`), `not_found_count` and `processes: []`, the targets in `not_found`. The same document carries `error` (`Not found: 99999999`), `exit_code: 2` and `reason: "not_found"`, so it reads as the error the other commands print (a PID that a guard rules out is the `ruled_out` error). Some found: `success: true`, the rest in `not_found`.

### ports

`{"action": "ports", "success": true, "count": 12, "ports": [<Port>, ...], "unseen_ports": [22, 80]}`

`unseen_ports` (Linux and macOS): the ports in use by other users' processes, which proc can't see unless it runs as root, so `ports` doesn't list them; `[]` for none. Absent on Windows and the BSDs (not checked) and with `--by`, `--in`, `--exposed` or `--local`, where proc can't tell whether an unseen holder would match. A port in it is in use: never treat it as free.

`hidden_by` (FreeBSD and NetBSD): the setting through which the system hides other users' sockets (`"security.bsd.see_other_uids=0"`, `"security.curtain=1"`), when it is on and proc isn't root. The list then has only proc's own user's ports, and proc can't name the others: a port missing from it may still be in use. Absent otherwise.

Each port object is the port (as above) and what holds it, from the holder's process: `command`, `user`, `uptime_secs` and `memory_mb`. A key is absent when the OS does not give the value, or when the holder exited between listing the port and reading it (the port keys stay), never `null`. With `--verbose` a port also has `exe_path` and `connections` (`total` and `by_state` only, no `peers`: the connections that came in on this process's own sockets; absent where the OS reads none, as on FreeBSD and NetBSD). `memory_mb` is absent (and MEM shows `-`) when the OS refused the reading, which `Process` records as 0. A process listening on one port number at two addresses (`127.0.0.1` and `::1`) has two rows, and each row carries that process's connection counts for the port, not a share per address, so summing `connections.total` over the rows counts them twice: count each (`pid`, `port`) once.

```json
{ "port": 3000, "protocol": "tcp", "pid": 1234, "process_name": "node", "address": "127.0.0.1", "exposure": "local",
  "command": "node server.js", "user": "dev", "uptime_secs": 34, "memory_mb": 41.5 }
```

### on

One port (`proc on :3000`):

```json
{
  "action": "on", "query_type": "port_to_process", "success": true,
  "port": 3000, "protocol": "tcp", "address": "0.0.0.0", "exposure": "exposed",
  "connections": {
    "total": 3, "by_state": {"established": 3},
    "peers": [{"holder_pid": 1234, "state": "established", "remote_address": "127.0.0.1", "remote_port": 53781, "peer": {"pid": 4321, "name": "curl"}}, ...],
    "peers_truncated": true
  },
  "process": <Process>,
  "processes": [<Process>, ...]
}
```

`127.0.0.1:3000` and `[::1]:3000` are different sockets. `processes` lists every holder once. `process` is the holder with the lowest PID on every OS, and `protocol`, `address` and `exposure` are that holder's. Read `processes`.

`connections` counts every TCP connection to the port, whichever holder's socket it came in on: `total`, `by_state` (`established`, `close_wait`, `time_wait` and so on, a state with none is left out) and `peers`, at most 50, each with the `holder_pid` whose socket it came in on, its `state`, the client's `remote_address` and `remote_port`, and `peer`, the process at the client's end (`pid`, and `name` when known) when that end is on this machine. `peers_truncated` is present, `true`, only when there are more connections than `peers` lists. `connections` is absent where proc reads none (FreeBSD and NetBSD) or the reading failed, never `null`; a port with none has `total: 0`.

One PID (`proc on <pid>`): `{"action": "on", "query_type": "process_to_ports", "success": true, "process": <Process>, "ports": [<Port>, ...]}`. Each port object also has `connections` with `total` and `by_state` only (no `peers`): the connections that came in on this process's own sockets, so a port held by two processes counts each one's share. For another user's process, when proc does not run as root, `ports` is `[]` because proc cannot see that process's sockets, not because it listens on nothing; the human card leaves out `Listening:` then. `status` is `"unknown"` and `memory_mb` and `cpu_percent` are `0.0` for a process the OS did not let proc read (a status of `unknown` and no `cpu_time_secs` is the mark): treat them as unknown, not as measurements.

One name, or several targets (`proc on :3000 1234 node`):

```json
{
  "action": "on", "success": true, "count": 2,
  "results": [
    {"target": ":3000", "process": <Process>, "ports": [<Port>]},
    {"target": "node", "process": <Process>, "ports": [<Port>, ...]}
  ],
  "not_found": ["1234"]
}
```

A single name gives `results` entries of `{process, ports}` without `target`; each port has `exposure` and `connections` (`total` and `by_state`) as for one PID. Port targets list that port only. If no target is found the plain error (exit 2) is printed.

### kill, stop, freeze, thaw, orphans --kill, stuck --kill

```json
{
  "action": "kill", "success": true,
  "succeeded_count": 1, "failed_count": 0,
  "not_found": ["99999999"],
  "succeeded": [<Process>, ...],
  "failed": [{"process": <Process>, "error": "Signal failed: Failed to kill process 123"}],
  "processes": [<Process> + "outcome": "succeeded", <Process> + "outcome": "failed" + "error": "Signal failed: Failed to kill process 123"],
  "count": 2
}
```

`action` is `"kill"`, `"stop"`, `"freeze"`, `"thaw"`, `"orphans"` or `"stuck"`. `processes` is the receipt for the preview's `processes`: every process the command acted on, in the preview's order, as full entries (the keys of a process object), each with `outcome`, `"succeeded"` or `"failed"`, and on a failure `error`, the same text as in `failed[].error`. `count` is how many there are (`succeeded_count + failed_count`). Read `processes`: one list, one entry per process the preview listed, so an agent can record exactly what it did. `succeeded` and `failed` (with their counts) hold the same processes split in two, and stay. If a process failed, `success` is `false` and the exit is 1; the others were still acted on. A non-empty `not_found` does not change `success`, so compare every target you passed with `processes`. `orphans` and `stuck` have no `not_found`. `processes`, `count` and `outcome` are new in 2.3.0 and come after the older keys.

### --dry-run

`{"action": "kill", "success": true, "dry_run": true, "count": 1, "not_found": [], "processes": [<Process>, ...]}`. Nothing is changed. The same shape serves `stop`, `freeze`, `thaw`, `free`, `orphans --kill`, `stuck --kill` (the last two without `not_found`). For `free`, `processes` are every holder of the ports.

### free

```json
{
  "action": "free", "success": true,
  "results": [{"port": 3000, "freed": true}, {"port": 8080, "freed": false}],
  "not_found": [":9000"],
  "processes": [<Process> + "outcome": "succeeded", <Process> + "outcome": "failed" + "error": "Still running"],
  "count": 2
}
```

`processes` are the holders it stopped, in the preview's order, as in the other receipts (new in 2.3.0): `outcome` is `"failed"` for a holder that was still running when the ports were checked (`error` is the text of the last signal that failed for it, or `"Still running"`), `"succeeded"` for one that is gone. The check is whether the holder is still running when the receipt is written, a moment after the port check, and `success` is about the ports: a receipt can have `success: true` and a holder with `outcome: "failed"` that is still alive but no longer holds the targeted ports. `count` is how many. `results` stays the per-port answer. A port still in use after `--wait` seconds gives `success: false` and exit 1. A port held by several processes has all of them stopped and counts as freed only once none is left. Ports nobody listens on go to `not_found` as typed; if all are free the result is `{"action": "free", "success": true, "count": 0, "message": "All specified ports are already free", "not_found": [":3000"]}` with exit 0.

### wait

```json
{
  "action": "wait", "success": true, "timed_out": false,
  "elapsed_seconds": 945, "elapsed_human": "15m 45s",
  "target": "node", "initial_count": 2, "not_found": [],
  "exited": [{"pid": 12346, "name": "node", "exited_after_seconds": 512, "exited_after_secs": 512}],
  "still_running": [],
  "count": 2, "elapsed_secs": 945, "targets": ["node"]
}
```

New in 2.3.0, after the older keys: `count` (the processes waited for), `elapsed_secs`, `targets` (the arguments as typed, one per entry: a deprecated comma list stays one entry) and, in each `exited` entry after its other fields, `exited_after_secs`. Deprecated, use the new name; removed in 3.0: `initial_count` (use `count`), `elapsed_seconds` (use `elapsed_secs`), `exited_after_seconds` (use `exited_after_secs`) and `target`, every target joined by a space (use `targets`).

On timeout: `success: false`, `timed_out: true`, `still_running` lists the still-running processes, exit 1. If no target exists at all, exit 2 at once.

Each `exited` entry is `pid`, `name`, `exited_after_seconds` and then the other fields of a process object (see Process and port objects) as the last reading proc had of it before it exited. `still_running` entries carry the same fields, as of the reading taken when `wait` began. A field that reading did not have is absent.

### unstick

```json
{
  "action": "unstick", "success": true, "dry_run": false, "force": false,
  "found": 1, "recovered": 1, "not_stuck": 0, "still_stuck": 0, "terminated": 0, "failed": 0,
  "not_found": [],
  "processes": [{"pid": 1234, "name": "node", "uptime_secs": 3600, "outcome": "recovered", "cwd": "/home/user/project", "command": "node server.js", "status": "running", "cpu_percent": 0.0, "memory_mb": 256.0, "exe_path": "/usr/local/bin/node", "user": "alice", "parent_pid": 1000, "start_time": 1700000000, "parent_name": "zsh", "cpu_time_secs": 42, "open_files": 23, "child_count": 2}],
  "count": 1, "recovered_count": 1, "not_stuck_count": 0, "still_stuck_count": 0, "terminated_count": 0, "failed_count": 0
}
```

New in 2.3.0, after `processes`: `count` (the processes found) and `recovered_count`, `not_stuck_count`, `still_stuck_count`, `terminated_count` and `failed_count`, one count per outcome. Deprecated, use the new name; removed in 3.0: the bare numbers `found` (use `count`), `recovered`, `not_stuck`, `still_stuck`, `terminated` and `failed` (use `<outcome>_count`). `failed` is a number here and a list in the receipts of `kill`, `stop`, `freeze`, `thaw`, `stuck --kill` and `orphans --kill`: read `failed_count`.

Each `processes` entry has the same fields as a process object (see Process and port objects) after its own `pid`, `name`, `uptime_secs` and `outcome`; the fields of a process object that the OS does not give are absent.

`outcome` is `recovered`, `terminated`, `still_stuck`, `not_stuck`, `failed: <reason>` or `would_attempt` (dry run). Any `still_stuck` or `failed` gives `success: false` and exit 1. SIGINT ends most programs, so `unstick` can end a busy process.

### why

```json
{
  "action": "why", "success": true, "count": 1, "not_found": [],
  "results": [{
    "target": ":3000", "port": 3000, "protocol": "tcp",
    "process": {"pid": 1234, "name": "node", "command": "node server.js", "cwd": "/home/user/project", "status": "running", "uptime_secs": 3600, "cpu_percent": 12.5, "memory_mb": 256.0, "exe_path": "/usr/local/bin/node", "user": "alice", "parent_pid": 1000, "start_time": 1700000000, "parent_name": "zsh", "cpu_time_secs": 42, "open_files": 23, "child_count": 2},
    "ports": [<Port>, ...],
    "ancestry": [
      {"pid": 1, "name": "launchd", "status": "unknown", "is_target": false, "cpu_percent": 0.0, "memory_mb": 0.0, "exe_path": "/sbin/launchd", "child_count": 40},
      {"pid": 1234, "name": "node", "command": "node server.js", "cwd": "/home/user/project", "status": "running", "uptime_secs": 3600, "is_target": true, "cpu_percent": 12.5, "memory_mb": 256.0, "exe_path": "/usr/local/bin/node", "user": "alice", "parent_pid": 1000, "start_time": 1700000000, "parent_name": "zsh", "cpu_time_secs": 42, "open_files": 23, "child_count": 2}
    ]
  }]
}
```

`process` and each `ancestry` entry carry the fields of a process object (see Process and port objects): `command`, `cwd`, `status` and `uptime_secs` as before, then `cpu_percent`, `memory_mb` and the rest, with a field the OS does not give absent. `is_target` marks the target's own entry. A port with several holders gives one result per holder, with the same `target`. Two agent sessions both read `claude`, so compare PIDs with your own `proc tree $$ --ancestors --json` to tell whose it is.

### tree

`{"action": "tree", "success": true, "tree": [{"pid", "name", "cpu_percent", "memory_mb", "status", "uptime_secs", "children": [...], "cwd", "command", "exe_path", "user", "parent_pid", "start_time", "parent_name", "cpu_time_secs", "open_files", "child_count"}]}`. Each node is a process object (see Process and port objects): the keys before `children` are as before, and the fields a node lacks from the OS are absent.
With `--ancestors`: `{"action": "tree", "success": true, "ancestry": [{"target_pid", "target_name", "depth", "chain": [{"pid", "name", "cpu_percent", "memory_mb", "status", "uptime_secs", "cwd", "command", "exe_path", "user", "parent_pid", "start_time", "parent_name", "cpu_time_secs", "open_files", "child_count"}, ...]}]}`. `chain` runs from the top ancestor down to the target, and each entry is a process object as in the tree.
Without a target, `--in`, `--by` and the other filters narrow the view: an empty `tree` is exit 0, as in `list`, while `by <name>` with no match is exit 2. With a target, a name that matches nothing or a target the filters rule out is exit 2.

### for

`{"action": "for", "success": true, "count": 2, "results": [{"process": <Process>, "ports": [<Port>, ...]}]}`. Each port is the plain port object (with `exposure`, without `connections`). `ports` is absent when the OS gave no port table (for example macOS without `lsof` on the PATH). For another user's process, when proc does not run as root, an empty `ports` means proc could not see its sockets.

### watch

NDJSON, one object per refresh (`-n` seconds, default 2), until interrupted: `{"action": "watch", "success": true, "count": 2, "processes": [<Process>, ...]}`. Without a terminal and without `--json` it prints one snapshot. Bound a stream with `| head -n 5`; prefer `proc list` for one snapshot. PID targets are checked once, before the first frame: when no PID is held, or `--in` and `--by` rule out every one, it exits 2 with the error document (`not_found`, or `ruled_out` with `ruled_out_by`) and streams nothing. A name or a port is watched until something matches, so `proc watch node` can start before node does. A process that exits while watched leaves empty frames.

## Names deprecated in 2.3.0

2.3.0 adds one set of names beside the old ones, so 2.x scripts keep working. Each old name below is deprecated, use the new one; removed in 3.0. Both are in the document until then.

| Command | Old name | New name |
|---|---|---|
| `info` | `found_count` | `count` |
| `wait` | `initial_count` | `count` |
| `wait` | `elapsed_seconds` | `elapsed_secs` |
| `wait` | `exited[].exited_after_seconds` | `exited[].exited_after_secs` |
| `wait` | `target` (every target joined by a space) | `targets` (a list, as typed) |
| `unstick` | `found` | `count` |
| `unstick` | `recovered`, `not_stuck`, `still_stuck`, `terminated`, `failed` (numbers) | `recovered_count`, `not_stuck_count`, `still_stuck_count`, `terminated_count`, `failed_count` |

The receipts of `kill`, `stop`, `freeze`, `thaw`, `stuck --kill`, `orphans --kill` and `free` gain `processes` (each entry with `outcome`) and `count`. Their `succeeded`, `failed`, `succeeded_count`, `failed_count` and `results` are not deprecated.

The rule behind the names: a list's key says what is in it (`processes`, `ports`, `results`); a count is `count` for the main list and `<list>_count` for another list beside it, or `<outcome>_count`; a duration ends in `_secs`; one key never means a list in one command and a number in another.

## Heuristics: stuck and orphans

Both list candidates. Neither proves that a process is hung or abandoned. Never act on a list without checking each PID.

- **`stuck`** lists a process that has run longer than `--timeout` (default 300 s) and, in each of two readings a second apart, either uses over 50% of one core or is in uninterruptible sleep (blocked in the kernel, usually on I/O). It takes about 2 seconds. False positives: a long build, a compiler, a busy server, or any healthy program at full CPU look the same as an infinite loop. On one laptop it listed a `python` at 97%, a `node` at 98% and a `node` at 453%.
- **`orphans`** lists processes whose parent is PID 1, minus system daemons (processes with no cwd, or with cwd `/`). False positives: on macOS every app launched by launchd has parent 1, and an unscoped run listed 90 ordinary apps; `nohup` and `tmux` jobs and daemons are orphaned on purpose. A process whose parent is any other process is never listed, and on Windows the command exits 4 (not supported): Windows does not re-parent processes, so there is no reliable orphan.
- **`--kill`** applies the action to the whole list. Do not use it. Check each PID with `proc info <pid> --json` and `proc why <pid> --json`, then act on the approved PIDs only. `--in <dir>` narrows the list but does not make it yours.

## Edge cases

- **Missing targets are in `not_found`**, never an error when at least one target was found. Check it after every action.
- **Several holders of a port.** `on :PORT` lists all in `processes`; `free` stops all and says nothing about which. On `freed: false`, list again.
- **Children.** `kill` and `stop` signal the PID you give, not its children. Orphaned children keep running with parent 1.
- **PID reuse.** Before each signal proc checks the process's start time, so a delayed SIGKILL cannot reach a new process that reused the PID. Between two commands that check is yours: pass the `start_time` you recorded with `--start-time`, and a PID that a new process holds exits 2.
- **Deleted directories.** A process can keep running in a removed directory (a deleted worktree). `proc in <dir>` lists them and `cwd` shows the plain path.
- **`proc on 3000`** looks up PID 3000, not port 3000. Ports need the colon.
- **`proc free`** accepts ports only (`proc free nginx` exits 4).
- **Human output** is for the user, not for parsing. `on` (by port, PID or name), `info`, `for` with one hit and `why` print one card for a process: `Process:`, `Command:`, `Directory:`, `Path:`, `User:`, `Parent:`, `Status:`, `Resources:`, `Started:`, `Children:`, then `Listening:` and, for a process that listens, `Connections:`. Every process table has the columns `PID NAME USER CPU% MEM UPTIME COMMAND` (`DIR` second under `--in`), one process per line, with COMMAND cut to the terminal width and `-` for a value proc cannot read. `--verbose` adds `PPID STATUS DIR` and cuts nothing; `--wide` prints the directory and command whole. The list in front of a `--dry-run` or a question always has the whole command line.

## Platform differences

| Area | Windows | Linux | macOS | FreeBSD, NetBSD |
|------|---------|-------|-------|-----------------|
| `freeze`, `thaw`, `stop --signal` | exit 4, not supported | works | works | works |
| `kill --graceful` | `taskkill` without `/F`: fails for console programs (exit 1). Use `stop`, which escalates. | SIGTERM | SIGTERM | SIGTERM |
| `for` (open files) | finds none (executables only) | read from `/proc`, nothing to install | `lsof` (exit 4, `not_supported`, without it) | `fstat`, in the base system |
| `orphans` | exit 4, not supported | PID 1 test | many app false positives | PID 1 test |
| Names | `.exe` optional | cut to 15 chars; the executable and argv[0] file names also match | full names | full names |
| Port tables | built in | read from `/proc`, nothing to install | `lsof` | `sockstat`, in the base system |
| A deleted working directory | | still found by `in` | still found by `in` | the kernel forgets the path: `in` exits 4 (directory not found) |

A Windows cwd may appear in 8.3 short form.

## Signals

For `proc stop --signal` (case-insensitive, with or without `SIG`): `HUP`, `INT`, `QUIT`, `ABRT`, `KILL`, `TERM`, `STOP`, `CONT`, `USR1`, `USR2`. Numbers are rejected (exit 4). `--signal` exists only on `stop`; `--graceful` only on `kill`.

## Older versions

Before proc 2.0:

- `--json` counted as a yes. A changing command run with `--json` and without `--yes` acted at once. Do not run one on an older proc without `--dry-run` and the user's approval.
- A name matched any process whose name or command line contained it, so `kill node` reached helper processes of other apps.
- Several targets needed commas, and `kill 1 2` exited 4 with "unexpected argument".
- `kill`, `stop` and the other target commands had no `not_found` in their JSON. `free` and `on` reported one holder of a port.
- macOS wrote IPv6 addresses as `[::1]`.
- Before 1.14.1, `proc in <dir>` rejected a deleted directory with exit 4.

From 2.0 to 2.3, changing commands ask before acting, but some of what this reference describes is missing:

- 2.0.1 added `reason`; 2.1.0 added `--start-time`, `ruled_out`, `ruled_out_by` and `command_line_only`.
- 2.3.0 added the receipt's `processes` with each `outcome`, `count`, and `exposure` and `connections` on ports.
- 2.4.0 added `runs_proc`, `left_out`, `unseen_ports` and `hidden_by`.

If `proc --version` is below 2.4, ask the user to upgrade rather than working around these.
