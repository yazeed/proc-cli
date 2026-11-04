# Roadmap

This file is about what might come next. Nothing in it is promised. An idea ships only after it passes the feature test in [PHILOSOPHY.md](PHILOSOPHY.md), and many won't. When something ships it leaves this file: [CHANGELOG.md](CHANGELOG.md) is the history.

Each candidate lists what it would look like, how it fares against the feature test so far, and what's still open.

## Decided: the next major release

### Remove comma-separated targets

`proc kill :3000,:8080` has been deprecated since 2.0.0 in favour of separate arguments (`proc kill :3000 :8080`), and prints a notice on stderr when used (human output only, so `--json` is unaffected). The next major release removes it, so there is one spelling for several targets.

The same release removes `proc_cli::ui::process_table_budget` and `ProcessTableBudget`, deprecated since 2.3.0 (the process table no longer uses them).

It also marks `proc_cli::core::Process` `#[non_exhaustive]`, so a later field never breaks code that builds a `Process` with a struct literal again, and the enums `ProcessStatus` and `StatusFilter` too, so a new status never breaks an exhaustive `match` (2.3.0 adds `idle`, `starting` and `disk_wait`). 2.3.0 adds four public fields to it (`parent_name`, `cpu_time_secs`, `open_files`, `child_count`), which is such a break in a minor release; the maintainer accepted it there because no crate depends on proc-cli (crates.io lists none) and the CHANGELOG says so.

**Open:** what a comma does once removed: an error that shows the separate-argument form, or treating the whole word as one name.

## Decided: after the 2.1.0 tag

### Nix as an official install method: the nixpkgs package

The flake is done (see the CHANGELOG): it reads its version from Cargo.toml, has a lock, installs the man page and completions, and builds proc with its whole test suite in the sandbox on Linux and macOS in CI. What is left:

- A nixpkgs package, `proc-cli` (`pkgs/by-name/pr/proc-cli/package.nix`), with `passthru.updateScript = nix-update-script { }` so the update bot follows releases, and a maintainer entry. The file lives in this repo as `pkg/nixpkgs/package.nix`, and each release moves it and the pull request to the new version (RELEASE.md, "After the tag: nixpkgs"). The first pull request is for 2.1.1, the first release whose tests pass wherever Nix builds them.
- The README and website install rows say `nix profile install github:yazeed/proc-cli` already; they change to `nix run nixpkgs#proc-cli` once nixpkgs merges the package.
- With 2.2.0, the package's `meta.platforms` widens from `linux ++ darwin` to the BSDs as well (`freebsd ++ netbsd`), since proc runs there from that release.

**Open:** when nixpkgs merges the package. (The macOS Nix job passes with nothing skipped: its sandbox needed Apple's `ps` and nixpkgs' `lsof` as check inputs.)

## Decided, not yet scheduled

### A convention for guesses

"If you have to read the docs, we failed" (PHILOSOPHY.md, "Obvious") is the standard, and proc misses it where people guess. Tried with 2.2.0:

- `proc kill 3000` and `proc on 3000` read 3000 as a PID and say "Try: proc list". Nothing mentions `:3000`, though freeing a port is proc's main job and `kill-port 3000` is the habit people bring.
- `proc kill -9 node`, `proc stop --force node` and `proc kill --all node` stop at a parser error with no tip.
- Some tips match the spelling, not the meaning: `proc list --port 5000` suggests `--sort`, `proc kill node --signal 9` suggests `--in`, and `proc what :5000` and `proc who :5000` suggest `wait`, `watch` and `why` instead of `on`.
- `proc ps aux` takes `aux` as a name and exits 0 with "No processes found".
- `p` is `ports` while `ps` is `list`, and `i` (`info`) sits next to `in`. `-n` is `--limit` in five commands and `--interval` in two. `list aux` exits 0 where `by aux` exits 2 (documented, but a second rule to learn).

Already right: `info :5000`, `list node`, `list --sort mem`, `free 5000` (its error names `:5000`), `help kill`, and `proc port` (did you mean `ports`).

These get fixed together, from one convention, not one error message at a time:

1. **Collect the guesses** before deciding anything: the spellings of the tools people come from (`kill`, `pkill`, `killall`, `fuser`, `lsof`, `ps`, `top`, `htop`, `netstat`, `ss`, `kill-port`), the agent evals in `skills/proc-cli/evals.md`, and first-run sessions of people who have never used proc.
2. **Give every guess one of three answers.** It works, because it is a habit proc can take in one meaning (proc then says what it did in its own words); it teaches, with an error that names the right command and why; or it is a plain error. A guess never makes proc act on a reading the user didn't type ("Explicit"): a hint can say "port 3000 is `:3000`", but `kill 3000` never kills what is on port 3000.
3. **Write the convention down** in CONTRIBUTING.md, with the table of guesses and their answers, and a test for each guess, so every new command and flag is checked against it.
4. **Change the code in one release**, with the CHANGELOG giving each guess's answer. Anything that changes what an existing spelling does (`-n`, the aliases) waits for a major release, with a deprecation notice first.

This takes in the candidates "Errors that name what you meant", "Signal words, not numbers", "`proc :3000`" and "One meaning for `-n`" below, and the human clock is how it is measured: time to a correct answer for someone new, before and after.

**Feature test:** obvious, explicit, and one spelling per intent with habits kept (PHILOSOPHY.md, "Easy to Remember"). **Open:** which release; how far a hint may go before it is a guess.

## Candidates

### One set of JSON names

The JSON says some things two or three ways, so a script that reads more than one command has to learn each:

- **The preview and the receipt:** a command that changes processes lists them as `processes` in its preview (`--dry-run --json`, and the `needs_confirmation` error) and as `succeeded` and `failed` in its receipt. The list that was approved and the list that was changed should read the same way.
- **Counts:** `count` in most commands, `found_count` and `not_found_count` in `info`, `succeeded_count` and `failed_count` in actions, `initial_count` in `wait`.
- **Durations:** `uptime_secs` on a process, but `elapsed_seconds` and `exited_after_seconds` in `wait`.
- **Names:** `process_name` on a port row, `name` on a process.
- **Where the answer is:** `processes`, `ports` or `results`, depending on the command.

The idea: settle every one of these in one pass, from one written rule (a count is `count`, or `<list>_count` beside a list it counts; a duration ends in `_secs`; a process's name is `name`), and check every command's JSON against it in tests.

**Feature test:** keeps the JSON contract (PHILOSOPHY.md, "One Contract"). **Plan:** the next minor release adds the settled names beside the old ones, so 2.x scripts keep working, and the CHANGELOG and `reference.md` mark the old names deprecated; the next major release removes them. **Open:** the rule's exact wording, and whether the receipt also carries the preview's `processes`.

### Signal words, not numbers

`-9`, `-15` and `-HUP` are notation people copy without knowing what they mean. proc already takes signal names (`--signal KILL`), but rejects numbers: `proc stop 1234 --signal 9` is an error. The idea: accept the number people already type, act on it, and answer with the word (`9 is SIGKILL`), so a habit turns into knowledge.

**Feature test:** fits the domain; makes the common case effortless for people with `kill -9` in their fingers. **Open:** whether `proc kill -9 <target>` (today: "unexpected argument") should also be accepted, and how that reads next to `kill` always meaning SIGKILL.

### A verb for reloading

`proc stop nginx --signal HUP` sends HUP and then, like every `stop`, force-kills whatever is still running after `--timeout`, so it can't safely reload a config. Reloading is something people do, so it could be a verb:

```bash
proc reload nginx      # send SIGHUP, don't escalate, report who got it
```

**Feature test:** one obvious command, natural grammar (a verb that changes state). **Open:** which signal each common server reloads on, and whether `reload` should verify anything afterwards.

### Defaults that follow the risk

Every confirmation prompt defaults to no today. A prompt that always defaults to no trains people to type yes without reading. The idea: default to yes when the action can be undone (`freeze`, which `thaw` reverses) and to no when it can't (`kill`). `--json` and scripts are unaffected: they still need `--yes`.

**Feature test:** "is the safe path the short path?" (proposed below). **Open:** which commands count as recoverable (`stop` can escalate to SIGKILL).

### `proc :3000`

A bare target as a shortcut for `proc on`: the shortest possible answer to "what's on port 3000?". Today it's "unrecognized subcommand".

**Feature test:** one obvious command for the most common question. **Open:** whether a bare name or PID should work too (`proc node`), and the conflict with a process named like a future command.

### Errors that name what you meant

When a guess is wrong, the error should say the right command. A mistyped subcommand or value already gets the closest names (`proc kil` says "did you mean 'kill'?"); these don't yet:

- `proc kill 3000 --dry-run`, with no process 3000 but a listener on port 3000, could ask "did you mean :3000?".
- Flags people reach for from other tools: an agent tester typed `proc kill --name worker` and got "unexpected argument '--name' found". The error could say "to filter by name, use --by". Likewise `--port` (a target is `:3000`) and `--pid` (a PID is a plain number).

**Feature test:** obvious, explicit (proc says which reading it took), and it keeps one spelling per intent. **Alternative considered:** hidden aliases (`--name` meaning `--by`). They fix the typo silently but give one flag two spellings and teach nothing, which "Easy to remember" argues against. **Open:** how far suggestions go before they become guesses, and which borrowed flags are common enough to name.

### Hog

```bash
proc hog           # resource-heavy processes
proc hog --cpu
proc hog --mem
```

A shorter `proc list --min-cpu 10 --sort cpu`. **Feature test:** fits, one command, but it adds a second spelling for something `list` already does, which goes against "one obvious spelling per intent" (PHILOSOPHY.md, "Easy to Remember"). **Open:** whether there is real demand.

### Quiet mode

```bash
proc kill :3000 -q    # errors only
proc on :3000 -q      # the PID only
```

For shell scripts and pipelines that want one value without JSON. **Feature test:** fits, obvious flag. **Open:** demand, and what each command prints in quiet mode.

### Doctor

```bash
proc doctor
```

One health check after a crash or a messy session: port conflicts, orphans, stuck processes, zombies, each with the command that fixes it. **Feature test:** fits, one command, but it aggregates existing commands rather than adding a capability. **Open:** what counts as a problem worth reporting.

### Orphans on Windows

`proc orphans` exits 4 on Windows, because an orphan there can't be told by a parent PID of 1: Windows has no PID 1 and keeps the dead parent's PID on the child. The idea: call a process an orphan when its parent PID is no longer running, or has been reused by a process that started after the child.

**Feature test:** fits the domain; keeps one command the same on every OS. **Open / risk:** normal Windows processes outlive their launcher (`explorer.exe`, services, installers, anything started through a shell that exits), so the list would be long and mostly wanted, and `--kill` could take wanted processes with it. It would need to be list-only, or limited by `--in`, before it is safe.

### UDP ports

`proc on`, `ports` and `free` only see TCP listeners. **Open:** how UDP sockets show up in each command without making TCP output noisier.

### Connection counts per listening socket

`ports` and `on <pid>` give a process listening on one port number at two addresses (`127.0.0.1` and `::1`) the same `connections` counts on both rows, because a connection is credited to a holder, not to the socket it came in on. **Open:** record each connection's local address in `src/core/connections.rs`, so counts split per (holder, local address) and summing rows no longer counts twice.

### `tree` children in start order

`tree` lists siblings by PID. PIDs wrap and get reused, so PID order isn't spawn order: on one Mac, 9-21% of adjacent siblings had the higher PID as the older process. A tree is read top to bottom as "what started what", so start time (PID as the tie-break) is the natural order.

**Feature test:** deepens proc's command of its domain. **Open:** whether `tree` alone should differ from `list --sort pid`, or a `tree --sort` flag is better than changing the default (a sort flag on a hierarchy only orders siblings, which is easy to misread).

### `--sort uptime`

```bash
proc by node --sort uptime    # the longest-running first: which duplicate is the stale one
proc in . --sort uptime
```

The column is already shown and `--min-uptime` already filters by it. On `list`, `by`, `in`, `for` and `watch`. **Feature test:** fits, one obvious flag value. **Open:** the direction (oldest first matches cpu and mem putting the most extreme first) and the name (`uptime` or `started`).

### Sorting `orphans` and `stuck`

Both print processes in PID order (81 orphans on one Mac), which says nothing about which to look at first. A `--sort` like `list`'s would. **Feature test:** fits, but it's another way to ask a `list` question; `list --parent 1` is not the orphan set, so it isn't redundant. **Open:** real demand. `--limit` must never limit what `--kill` acts on.

### One meaning for `-n`

`-n` is `--limit` in `list`, `by`, `in`, `for` and `ports`, but `--interval` in `watch` and `wait` (where the limit is `-l`). **Feature test:** "Easy to remember" says one spelling means one thing. **Open:** which side moves, and how to deprecate the other without breaking scripts (a major release).

### The human clock

How long someone who has never used proc takes to get a correct answer (PHILOSOPHY.md, "Fast"). Unlike the machine clock (time per command), nothing measures it yet; the convention for guesses (above) is the first thing it would measure. **Open:** how to measure it (first-run sessions, agent evals like `skills/proc-cli/evals.md`).

### More operating systems

proc runs on macOS, Linux, Windows, FreeBSD and NetBSD. It reads processes through sysinfo, which supports Android, FreeBSD, NetBSD, iOS, Linux, macOS and Windows and returns empty lists anywhere else, so a new OS needs sysinfo support (or a process reader of proc's own), a port reader, and a CI VM. In order:

1. **Android (Termux).** sysinfo supports it and Rust targets it, so proc may nearly work; the port reader is Linux's, which reads `/proc/net`, and Android limits that for apps. Try it before deciding.
2. **OpenBSD**, the BSD that's left: `fstat` and `netstat` in the base system cover open files and ports. The larger part is OpenBSD support in sysinfo, best contributed there, where other tools gain from it too.

Not planned: illumos and Solaris (no sysinfo support and few users), Cygwin (its users are on Windows, where proc already runs natively) and Redox (too early). **Feature test:** a platform, not a feature; it puts the same commands where people already are. **Open:** what Android allows an app to read in `/proc`; whether sysinfo would take OpenBSD support.

### More ways to install

Each of these puts proc one command away for people who don't use the channels proc has today. nixpkgs is decided (see "Nix as an official install method: the nixpkgs package" above: `nix run nixpkgs#proc-cli` once merged). winget and PyPI are decided, in this order:

1. **winget**, the package manager that ships with Windows. Its contributing guide sets no popularity bar, and the identifier `yazeed.proc-cli` is free. The manifest uses the release's `proc-windows-x86_64.exe.zip` as a zip with a nested portable installer and the command alias `proc` (the shape `sharkdp.fd` uses); a Windows CI job checks it with `winget validate` and `winget install --manifest`. The first version is one hand-written PR to microsoft/winget-pkgs (Microsoft's CLA is signed on it); after it merges, a `winget-releaser` job in release.yml submits each release, which needs a fork of winget-pkgs and a classic token (`public_repo`, `workflow`). winget scans installers with several antivirus engines, which can flag a tool that kills processes.
2. **PyPI as `proc-cli`** (the name is free; `proc` is a 2020 library that installs no command), built with maturin's `bin` bindings, as crate-ci/typos does, so `pip`, `pipx` and `uv tool` put `proc` on PATH. Wheels for manylinux and musllinux (x86_64, aarch64), macOS (two) and Windows, published with PyPI trusted publishing, which needs a one-time setup on pypi.org. A TestPyPI run comes first.
3. **The BSDs' own package systems and CPUs**, after 2.2.0 (the first release with FreeBSD and NetBSD binaries, for x86_64 only): a FreeBSD port (`pkg install proc-cli`) and a pkgsrc package for NetBSD, which proc can't publish itself (each goes through that project's review); and ARM64 binaries, which would build like the x86_64 ones, natively in an ARM64 VM (vmactions takes an `arch`; untried).
4. **homebrew-core**, later: `brew install proc` with no tap, and Linuxbrew. A new formula needs 75 stars when someone else submits it, 225 when the repo's owner does (or 30 / 90 forks or watchers), and must build from source with a `livecheck`. The repo had 62 stars when this was written.

**Official Debian, revisited once the repository passes 1,000 stars.** proc's own signed apt repository (https://proc-cli.com/apt) already gives Debian and Ubuntu users every release the day it ships. Being in Debian's archive, so `apt install proc-cli` works with nothing added, costs more than it gives today: Debian builds proc from its own packaged crates, not `Cargo.lock` (five of proc's direct dependencies are at other versions there, so each release would need patches); a Debian Developer has to sponsor it through an intent-to-package bug and the new-package queue (weeks to months); and Debian stable then keeps one version for a year or more. At 1,000 stars, look again: whether someone in Debian's Rust team would sponsor or co-maintain it, and how far Debian's crate versions are from proc's.

**Feature test:** install, not a feature; it makes the common case (getting proc) one command on each platform. **Open:** the winget and PyPI builds have not been run yet; whether winget's antivirus scan accepts proc.

## Proposed changes to PHILOSOPHY.md

PHILOSOPHY.md is the criteria, so it changes slowly and on purpose. These changes are proposed and not adopted:

- **Language, not notation:** prefer words to signal numbers and other notation; accept the notation and answer with the word.
- **Safe by default:** the dangerous path must be longer to type than the safe one; prompts default by risk, not always to no.
- **Two new feature-test questions:** "Would you say it out loud?" and "Is the safe path the short path?"

## Not planned

These are outside proc's scope. See [PHILOSOPHY.md](PHILOSOPHY.md) for why.

- **Service management:** use systemd, launchd or supervisord
- **Container management:** use docker or podman
- **Remote processes:** use ssh + proc
- **Historical data:** use monitoring tools; `proc watch` shows the present
- **A GUI or an interactive TUI:** proc is for when you know what you want. `proc watch` refreshes a live view, but its only key is quit
- **Auto-updates:** use your package manager

---

Have an idea? Open a [discussion](https://github.com/yazeed/proc-cli/discussions).
