# Roadmap

This file is about what might come next. Nothing in it is promised. An idea ships only after it passes the feature test in [PHILOSOPHY.md](PHILOSOPHY.md), and many won't. When something ships it leaves this file: [CHANGELOG.md](CHANGELOG.md) is the history.

Each candidate lists what it would look like, how it fares against the feature test so far, and what's still open.

## Decided: the minor release after 2.2.0

### One rich, consistent view of a process

proc collects most of what a person wants to know about a process or port, but each view prints a different subset under different labels (`Directory:`, `dir:`, `cwd:`, `DIR`), `-v` means something different in each command, `on <pid>` says less than `on :port`, and `tree`, `why` and `wait` return cut-down JSON objects. On macOS the same process even has two names: `on :port` and `ports` print lsof's (`python3.14`), the rest sysinfo's (`python3`), so a name copied from `ports` doesn't match `by`. The plan (from a study of portview and an inventory of every view's output):

- One card for a process, the same labels, order and alignment in `on` (by port, PID or name), `info`, `for` and `why`: command line, directory, path, user, parent (name and PID), status, resources (CPU, memory, CPU time, open files), start time and uptime, children, and for each port its exposure in words (`this machine only`) and its connections (a count by state, then up to 5 peers; `-v` lists all).
- The tables (`list`, `by`, `in`, `for`, `orphans`, `stuck`, `watch`, previews) default to `PID NAME USER CPU% MEM UPTIME COMMAND`; `-v` adds PPID, STATUS and DIR and stops cutting values. `ports` moves to the same table code and gains NAME, UPTIME and COMMAND.
- One name per process everywhere: the name `Process` records, with lsof's only for a process that is already gone. Name matching is unchanged.
- JSON only grows: `parent_name`, `cpu_time_secs`, `open_files`, `child_count` on every process object (`tree`, `why` and `wait` get full ones), `exposure` and `connections` on every port.
- Every place that shows `proc on :3000` also shows `proc on <pid>` and a name.

**Feature test:** fits the domain, makes the common question (what is this and who is talking to it) one command, adds no mode or state, and keeps the JSON contract (additions only). **Open:** the cost of connections, child counts and open files per platform, measured before each is adopted (the target is no slower than `on` today, about 0.5 s).

## Decided: the next major release

### Remove comma-separated targets

`proc kill :3000,:8080` has been deprecated since 2.0.0 in favour of separate arguments (`proc kill :3000 :8080`), and prints a notice on stderr when used (human output only, so `--json` is unaffected). The next major release removes it, so there is one spelling for several targets.

**Open:** what a comma does once removed: an error that shows the separate-argument form, or treating the whole word as one name.

## Decided: after the 2.1.0 tag

### Nix as an official install method: the nixpkgs package

The flake is done (see the CHANGELOG): it reads its version from Cargo.toml, has a lock, installs the man page and completions, and builds proc with its whole test suite in the sandbox on Linux and macOS in CI. What is left:

- A nixpkgs package, `proc-cli` (`pkgs/by-name/pr/proc-cli/package.nix`), with `passthru.updateScript = nix-update-script { }` so the update bot follows releases, and a maintainer entry. The file lives in this repo as `pkg/nixpkgs/package.nix`, and each release moves it and the pull request to the new version (RELEASE.md, "After the tag: nixpkgs"). The first pull request is for 2.1.1, the first release whose tests pass wherever Nix builds them.
- The README and website install rows say `nix profile install github:yazeed/proc-cli` already; they change to `nix run nixpkgs#proc-cli` once nixpkgs merges the package.
- With 2.2.0, the package's `meta.platforms` widens from `linux ++ darwin` to the BSDs as well (`freebsd ++ netbsd`), since proc runs there from that release.

**Open:** when nixpkgs merges the package. (The macOS Nix job passes with nothing skipped: its sandbox needed Apple's `ps` and nixpkgs' `lsof` as check inputs.)

## Candidates

### One word for the preview and the receipt

A command that changes processes lists them as `processes` in its preview (`--dry-run --json`, and the `needs_confirmation` error) and as `succeeded` and `failed` in its receipt. A script that previews and then acts has to know both names for the same set of processes. The idea: settle on one word, or make the receipt carry the preview's `processes` too, so the list that was approved and the list that was changed read the same way.

**Feature test:** follows the JSON contract and adds keys without changing any. **Open:** whether to add a key to the receipt or rename one (a rename breaks 2.0 scripts, so only in a major release).

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

A shorter `proc list --min-cpu 10 --sort cpu`. **Feature test:** fits, one command, but it adds a second spelling for something `list` already does, which goes against the proposed "one obvious spelling per intent" (below). **Open:** whether there is real demand.

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

How long someone who has never used proc takes to get a correct answer. Unlike the machine clock (time per command), nothing measures it yet. **Open:** how to measure it (first-run sessions, agent evals like `skills/proc-cli/evals.md`).

### More ways to install

Each of these puts proc one command away for people who don't use the channels proc has today. nixpkgs is decided (see "Nix as an official install method: the nixpkgs package" above: `nix run nixpkgs#proc-cli` once merged). winget, `.deb`/`.rpm` and PyPI are decided for after 2.1.0, in this order:

1. **winget**, the package manager that ships with Windows. Its contributing guide sets no popularity bar, and the identifier `yazeed.proc-cli` is free. The manifest uses the release's `proc-windows-x86_64.exe.zip` as a zip with a nested portable installer and the command alias `proc` (the shape `sharkdp.fd` uses); a Windows CI job checks it with `winget validate` and `winget install --manifest`. The first version is one hand-written PR to microsoft/winget-pkgs (Microsoft's CLA is signed on it); after it merges, a `winget-releaser` job in release.yml submits each release, which needs a fork of winget-pkgs and a classic token (`public_repo`, `workflow`). winget scans installers with several antivirus engines, which can flag a tool that kills processes.
2. **`.deb` and `.rpm` files on GitHub Releases.** Decided for 2.2.0, built and tested on `main`, shipped with the release (this item leaves the file then). No gatekeeper: they are release assets, not a repository. nfpm (`pkg/nfpm/`) builds both from the static musl binaries in the `package-linux` job of release.yml, with the man page and completions `proc` prints; each was tested with `dpkg -i` or `rpm -i`, `apt` and `dnf` in Debian 12 and 13, Ubuntu 24.04 and Fedora containers on amd64 and arm64. No distro package installs a `/usr/bin/proc` (checked with `apt-file` and `dnf provides`). Without an apt or dnf repository (which needs a signing key) users get no updates, and the files are unsigned.
3. **PyPI as `proc-cli`** (the name is free; `proc` is a 2020 library that installs no command), built with maturin's `bin` bindings, as crate-ci/typos does, so `pip`, `pipx` and `uv tool` put `proc` on PATH. Wheels for manylinux and musllinux (x86_64, aarch64), macOS (two) and Windows, published with PyPI trusted publishing, which needs a one-time setup on pypi.org. A TestPyPI run comes first.
4. **The BSDs' own package systems and CPUs**, after 2.2.0 (the first release with FreeBSD and NetBSD binaries, for x86_64 only): a FreeBSD port (`pkg install proc-cli`) and a pkgsrc package for NetBSD, which proc can't publish itself (each goes through that project's review); and ARM64 binaries, which wait on cross: its released 0.2.5 has no `aarch64-unknown-freebsd` image (its main branch has one) and none for ARM64 NetBSD.
5. **homebrew-core**, later: `brew install proc` with no tap, and Linuxbrew. A new formula needs 75 stars when someone else submits it, 225 when the repo's owner does (or 30 / 90 forks or watchers), and must build from source with a `livecheck`. The repo had 62 stars when this was written.

**Feature test:** install, not a feature; it makes the common case (getting proc) one command on each platform. **Open:** the winget and PyPI builds have not been run yet; whether winget's antivirus scan accepts proc; whether the first release's packages install from the real release page (the release run itself is the one thing the containers could not test).

## Proposed changes to PHILOSOPHY.md

PHILOSOPHY.md is the criteria, so it changes slowly and on purpose. These changes are proposed and not adopted:

- **Language, not notation:** prefer words to signal numbers and other notation; accept the notation and answer with the word.
- **Safe by default:** the dangerous path must be longer to type than the safe one; prompts default by risk, not always to no.
- **Compatible with habits:** familiar spellings keep working, and proc answers in its own words.
- **One obvious spelling per intent**, as part of "Easy to remember".
- **Fast, two clocks:** the machine clock (with an honest exception for CPU readings, which need a real sampling interval) and the human clock.
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
