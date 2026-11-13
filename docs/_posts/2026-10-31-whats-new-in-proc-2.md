---
title: "What's new in proc 2: it does what you meant, and says why when it doesn't"
description: "A name means that name, nothing changes without a yes, a PID that changed hands is refused, every error says why in a word a script can read, and proc installs with Nix and runs on any Windows machine."
date: 2026-10-31
---

The [last post](/posts/whats-new-in-proc/) covered proc up to 1.14.5. Since then 2.0.0, 2.0.1, 2.0.2, 2.1.0 and 2.1.1 came out. 2.0.0 is a breaking release, and the breaks are the point: each one closes a way proc could act on something you didn't mean. Here's what changed, and what to do if you used proc before.

## A name means that name

In 1.x, `proc kill node` matched any process with `node` anywhere in its name or command line. On one Mac a dry run matched 193 processes, among them `Claude Helper` and `Obsidian Helper (Renderer)`, and with `--yes` all of them would have been killed. Now a name matches the whole process name (or the file name of its executable or of its first argument), the way `killall` does:

```bash
proc by node          # processes named node
proc by node --cmd    # the old way: node anywhere in the name or command line
```

When a name matches nothing exactly but appears in command lines, proc says so rather than quietly finding nothing:

```
No process found matching 'slee'
  8 processes have 'slee' in their command line (add --cmd to match those)
```

## Nothing changes without a yes

In 1.x, `--json` counted as consent, so a script or agent asking for JSON could kill without anyone confirming. Now only `--yes`, or a person answering the prompt, counts. Without it, a command that would change processes changes nothing, exits 4, and lists what it would have done:

```json
{"action": "stop", "success": false, "exit_code": 4, "reason": "needs_confirmation",
 "needs_confirmation": true, "processes": [{"pid": 19724, "name": "sleep"}]}
```

(Shortened: each process in `processes` carries all its details.)

`--dry-run` still previews without asking. And when there's no terminal to ask in, proc doesn't ask: it says what to pass instead.

## Several targets, one spelling

Targets are separate arguments now: `proc kill :3000 :8080 1234`. The comma form (`:3000,:8080`) still works but prints a note, and goes away in the next major release.

## A PID that changed hands

The OS hands a PID to a new process once the old one exits, and on one busy Mac every PID came around again in well under an hour. If you record a PID and stop it later, it may be someone else's process by then. 2.1.0 adds `--start-time`: pass the `start_time` you recorded from the process's JSON.

```bash
proc stop 19724 --start-time 1793506406 --yes
```

If PID 19724 now belongs to a process that started at another time, proc refuses and touches nothing:

```json
{"exit_code": 2, "reason": "ruled_out", "ruled_out_by": ["--start-time"],
 "error": "No process found matching '19724' (ruled out by --start-time 1793506406)"}
```

Independently of the flag, proc checks again right before each signal, including `stop`'s SIGKILL that comes seconds later, so a delayed signal can't land on a process that took the PID in the meantime.

## Every error says why

Exit codes keep one meaning on every command: `0` success, `1` error, `2` not found, `3` permission denied, `4` nothing was done. Since 2.1.0 every JSON error also carries a `reason`, one fixed word to branch on instead of parsing the message: `not_found`, `command_line_only` or `ruled_out` for exit 2, `invalid_input`, `needs_confirmation` or `not_supported` for exit 4, and so on. When a guard (`--by`, `--in`, `--start-time`) is what ruled a live process out, `ruled_out_by` names it, and the message does too. A PID that is simply gone gets `not_found` and no `ruled_out_by`, so the two cases never look alike.

## The edges

- **Pipes:** `proc list | head -n1` no longer crashes proc, and a `kill --yes` whose output goes to a reader that stopped early still reports what it really did.
- **Typos:** `proc kil` ends with `tip: did you mean 'kill'?`, and a mistyped port like `:3000x` is an error that says ports go from `:1` to `:65535`, not a search for a process named `:3000x`.
- **The terminal:** a signal at a prompt or in `proc watch` no longer leaves the terminal in raw mode.
- **Order:** the same command lists the same processes in the same order every time, so `--limit` picks the same rows and a preview matches what runs.
- **Names:** on Linux, the processes of a service account (one whose shell is `/bin/false`) show the account's name rather than its user ID.

## More ways to install

- **Nix:** `nix profile install github:yazeed/proc-cli/vX.Y.Z`, with a release's tag ([the install page](/install#nix) has the current one), installs proc with its man page and shell completions, and `nix run github:yazeed/proc-cli/vX.Y.Z -- on :3000` runs it once without installing. The flake builds proc and runs its whole test suite in Nix's sandbox, checked in CI on Linux and macOS. A nixpkgs package is in review.
- **Arch Linux:** the AUR has `proc` (built from source) next to `proc-bin` (the release binaries), both maintained by Rafael Dominiquini, their community packager.
- **Windows:** the binary no longer needs the Visual C++ runtime, so it starts on a machine that never installed it.

## Upgrading from 1.x

```bash
brew upgrade yazeed/proc/proc   # or: npm install -g proc-cli, cargo install proc-cli, scoop update proc
proc --version                  # proc 2.1.1
```

Then check your scripts for three things: a name that relied on matching part of a command line needs `--cmd`, a destructive command run with `--json` needs `--yes` (or it exits 4 and does nothing), and comma lists should become separate arguments. The [agent skill](/skill) is written for 2.x: `npx skills update` brings it up to date. Everything else is in the [changelog](/changelog).
