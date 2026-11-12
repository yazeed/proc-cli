---
layout: page
title: Messages
description: "What each warning and error proc prints means, and what to do about it."
permalink: /commands/messages
---

A line that starts with `⚠` is a warning: the command still ran. An error ends the command, and the exit code says what kind it is. With `--json` the same error is a JSON document, and its `reason` is the word to branch on ([Scripting and exit codes](/commands#exit-codes)).

## Nothing matched (exit 2)

#### `No process found matching '1234'`

No process has that PID or name, or it exited before proc looked. Run `proc list` to see what is running. A name has to be the whole process name: `node` doesn't match `node-server` or `Claude Helper`.

#### `3 processes have 'serve' in their command line (add --cmd to match those)`

No process is named `serve`, but three have it in their command line, such as `python -m http.server`. Add `--cmd` to match those, with `--dry-run` first: a word in a command line matches far more than a name does.

#### `No process found matching '1234' (ruled out by --by node)`

A process holds PID 1234, but it isn't the one the guard describes: its name isn't `node` (or, with `--in` or `--start-time`, its directory or start time differ). Usually the process you meant has exited and the system gave its PID to a new one. Check with `proc info 1234` before you act on it.

#### `No process listening on port 3000`

Nothing listens on that port: it is free. `proc ports` lists the ports in use.

#### `sshd (PID 1329) matches 'sshd', but proc runs under it, so it's left out`

The only processes the name matched are ones proc itself runs under: your shell, your terminal or the agent that ran proc. proc won't stop the program that is running it. Target it by PID if you really mean it.

#### `Process 1234 is no longer running`

It exited between the moment proc found it and the moment proc acted on it. There is nothing left to do.

## Permission (exit 3)

#### `Port 80 is in use, but proc can't see which process holds it: it belongs to another user`

Something listens on the port, but another user owns it (root's nginx, for example), and only root can see which process that is. The port is taken, not free. `sudo proc on :80` names the holder.

#### `proc can't tell whether port 22 is in use: this system hides other users' sockets (security.bsd.see_other_uids=0)`

On FreeBSD and NetBSD set to hide other users' sockets, a port another user holds is invisible, so proc can't say whether it is free. `sudo` sees every socket.

#### `Permission denied for PID 42`

The system refused proc a read it needed (the PID reads `0` when the refusal wasn't about one process). Run the command with `sudo` only once you've checked the process is one you mean to touch.

## Partly done (exit 1)

#### `✗ Failed to kill 1 process` with `→ launchd [PID 1]: Signal failed: …`

The system refused the signal, usually because another user owns the process. The other processes in the list were handled. Make sure the failed one is really yours before you try again with `sudo`.

#### `Port 3000 still in use (may be in TIME_WAIT)`

`free` stopped the holders, but the port wasn't free within `--wait` seconds (default 10): a child process inherited the socket, or a supervisor restarted the server. `proc why :3000` shows who holds it now.

#### `Operation timed out: after 60s — 1 still running: node [1234]`

`wait --timeout` ran out with the process still running. Nothing was done to it.

#### `System error: …`

The system gave an error proc couldn't work around, such as an unreadable process table. Try again; if it keeps happening, report it with the full message.

## Nothing was done (exit 4)

#### `kill asks before changing processes, and there's no terminal to ask in; pass --yes (or --dry-run to preview)`

A command that changes processes ran where nobody could answer its question (in a script, or with stdin redirected). Preview with `--dry-run`, then add `--yes` once you know what it will touch. With `--json` the message reads `kill changes processes, so it needs --yes, with --json too` and the document lists the processes in `processes`.

#### `error: invalid value '2h' for '--min-uptime <MIN_UPTIME>': whole seconds only: write 7200 for 2h`

A value in the wrong form, and the message says what to write instead. `--timeout`, `--wait`, `--min-uptime`, `--start-time` and `wait --interval` take whole seconds: `7200`, not `2h` or `1.5`. A negative number is refused as `can't be negative`, and `--min-cpu` and `--min-mem` refuse `nan` and `inf`.

#### `Invalid input: Unknown signal: 'FOO'. Valid signals: HUP, INT, QUIT, ABRT, KILL, TERM, STOP, CONT, USR1, USR2`

`stop --signal` takes one of those names, in any case and with or without `SIG`. Numbers aren't accepted.

#### `Invalid input: Directory not found: /path`

The directory given to `--in` or `in` doesn't exist and nothing runs in it. A directory that was deleted while processes still run in it is not an error: they are listed.

#### `error: unrecognized subcommand 'kil'` with `tip: did you mean 'kill'?`

A command proc doesn't have. The tip names the closest ones. `proc --help` lists them all.

#### `Not supported on this platform: …`

The system can't do it: `freeze`, `thaw`, `orphans` and `stop --signal` on Windows, which has no signals and doesn't re-parent processes.

## Warnings and notes

These don't stop the command.

#### `⚠ Not found: 99999999`

Some of the targets matched nothing; proc carried on with the others. If every target is missing, it is the exit 2 error above instead.

#### `⚠ Left out: zsh (PID 81340), which runs this proc`

A command that changes processes matched the shell, terminal or agent that runs proc, and skipped it, so `proc kill zsh` can't end the shell you typed it in. The other matches are handled.

#### `ℹ Includes zsh (PID 81340), which runs this proc`

A command that only reads listed a process proc runs under. It is in the list; this line tells you which one it is.

#### `⚠ 2 ports in use by other users' processes, which proc can't see: 22, 80 (run with sudo to see them)`

`ports` lists only the listeners it can trace to a process. These ports are taken by other users' processes; `sudo proc ports` lists them in full.

#### `Try: sudo proc info 1 to see the rest`

Under a card (`on`, `info`, `why`, `for`): the system showed proc this process only in part. On macOS another user's process, root's `launchd` among them, gives a normal user its name, path and children and little else. `sudo` reads the rest.

#### `⚠ No processes found`

A search found nothing: `list`, or `in` (`⚠ No processes found in /path`). That is an answer, so the exit code is 0, as it is for `✓ No stuck processes found` and `✓ No orphaned processes found`.

#### `Cancelled`

You answered no at the `[y/N]` question. Nothing was changed.

#### `note: comma-separated targets are deprecated …`

`proc kill 1,2` still works, but will stop working in a future major release. Write the targets as separate arguments: `proc kill 1 2`. The note goes to stderr and never into `--json`.
