# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **A test keeps the docs complete.** `tests/docs_coverage.rs` reads every command, flag and accepted value from the CLI definition, and every error reason from its constants, and fails, listing every miss at once, unless each command has its page on the website, each page lists every flag with its short form and every value it accepts, the agent reference names every flag and value, the README every flag, and the agent reference, the agent skill, the commands page, the README and docs.rs give every exit code and every `reason`. It also fails if the agent skill asks for a proc older than a flag it uses. It found the skill's exit table naming neither `permission_denied` nor `system_error`; the table now says what each means and what to do

### Changed

- **The winget submission is 2.4.0, and each release moves it until it is merged.** microsoft/winget-pkgs#469298, the first version of `yazeed.proc-cli`, was opened at 2.2.0 and still waits for a moderator: the release job skipped winget until then, so it stayed at 2.2.0 through 2.3.0 and 2.4.0. It now holds 2.4.0's manifests (the Windows zip's SHA-256 checked against the release's `checksums.txt`), so Windows users get the current release when it is merged, `proc ports` naming every holder (2.3.0) included. From the next release, `update-winget` moves it itself with `pkg/winget/move-pr.sh`: one commit on top of the pull request's branch replacing the three manifests, where only the version, the download, its SHA-256, the release date and the release notes link may change, then the title and description; a dry run prints the change. The manifests go to `pkg/winget/` on `main` with the other package files. Run from the 2.2.0 commit, the script writes the same 2.4.0 manifests that were moved by hand, byte for byte. RELEASE.md's verification now opens every submission still waiting for review (winget, nixpkgs, FreeBSD, pkgsrc) and checks it offers the new version, its description included. Nothing changed in proc

- **Every workflow action runs on Node 24.** Six still ran on Node 20, which GitHub is retiring: `actions/download-artifact` 4.3.0 to 8.0.2, `actions/upload-artifact` 4.6.2 to 7.0.2, `actions/setup-node` 4.4.0 to 6.5.0, `docker/build-push-action` 5.4.0 to 7.4.0, `docker/login-action` 3.7.0 to 4.8.0 and `docker/setup-qemu-action` 3.7.0 to 4.4.0. `cachix/install-nix-action` moved to 31.11.4 and `vmactions/freebsd-vm` to 1.6.0 (FreeBSD 15.1's image now has a UFS root, not ZFS). `setup-node` stays on 6.x: 7.0 no longer exports the placeholder `NODE_AUTH_TOKEN` the npm job's trusted publishing was tested with, so Dependabot leaves 7.x alone until a release's dry run passes on it. CI and the release's dry run passed on the branch before it was merged. Nothing changed in proc

### Fixed

- **The docs say what proc does where they had it wrong.** An audit ran the documented commands against the binary and read every page, and these were false:
  - `orphans` lists every process whose parent is PID 1, so on macOS an unfiltered run lists dozens of ordinary apps (100 on one Mac, Preview among them). Only the agent reference said so. Its page, the README, the FAQ and `proc orphans --help` now say it and tell you to narrow it with `--in` or `--by`.
  - `free` was described as killing the holder. It sends SIGTERM and only SIGKILLs what is still running after up to 5 seconds, as the README said. Its page, the commands page, `proc free --help` and docs.rs now say so.
  - `unstick` was "gentle", and the home page said nothing ends without `--force`. Its SIGINT, the signal Ctrl+C sends, ends most programs. Its page, the README, the home page, two posts and `proc unstick --help` now say that.
  - `in` and `--in` match the directory and every folder below it, not that directory alone. The README, `in`'s page, the commands page, `proc in --help` and the agent docs now say so.
  - The `info` and `on` pages said another user's process loses its status, CPU, memory and ports. On macOS it shows only `Process:`, `Path:` and `Children:`. They now say that, and that `sudo` reads the rest.
  - The FAQ's port check called every non-zero exit "free", but exit 3 means something you can't see holds the port. It now branches on 0, 2 and 3. Its zombie answer pointed to `parent_pid`, a JSON key; it now names the `-v` PPID column.
  - The README and the commands page said every command that takes targets has `not_found` in its JSON. `on` with one target, `tree`, `by`, `for`, `in` and `watch` don't, and the lists now name the ones that do.
  - The agent skill said it works with proc 2.0 or later, but its rules need `--start-time` (2.1), the receipt's `outcome` (2.3) and `runs_proc` and `left_out` (2.4). It now asks for 2.4, and the reference lists what each version from 2.0 to 2.3 lacks.
  - The agent reference said values the OS doesn't give are left out. `cpu_percent`, `memory_mb` and `status` are always there, as `0.0`, `0.0` and `unknown` for a process proc can't read, and its example showed `launchd` with values macOS doesn't give a normal user. Both are corrected.
  - The README's links to the changelog, the skill, the reference and the project's guides were relative, so the Docker Hub page, which shows the README, linked to nothing. They are absolute now.
  - An example used a real `~/Sites` path, and a docs.rs example a real user's path; both are generic now.
  - docs.rs gave no error model. Its front page now has the reasons with their exit codes and a tested Rust example that reads `reason()` and the exit code.

- **The apt repository gets the release's packages, and the release checks it.** 2.4.0's `/apt` kept listing 2.3.0 until it was redeployed by hand. Pages keeps the first deploy of a commit and ignores the next, and nearly every push to main deploys, so the tagged commit had been deployed before the release had its `.deb` files; the guard added in 2.3.0 skipped only a commit titled `chore: release v…`, and 2.4.0 tagged a later commit. `update-apt` now waits for "Commit the package files" and starts `pages.yml` on main's newest commit, which that job pushed and nothing has deployed, then reads `/apt`'s package lists for amd64 and arm64 until they show the release, and fails if they don't within 15 minutes. If no package commit was pushed, it fails at once and says how to deploy. `pages.yml` drops the guard and its `workflow_call` trigger. Nothing changed in proc

- **Three more tests no longer fail on timing.** `coloured_text_is_the_plain_text_plus_escape_codes` runs `proc list` with and without colour and compares the text. 2.4.0 made it rerun the pair until both printed the same cells, but on Windows CI 10 pairs in a row differed, the last on the listener's uptime (10s against 11s). Digits are now compared as digits, not as values, so a ticking figure matches; a figure that changes its width, or a cell that changes its word, still reruns the pair, and the spacing must still match. It still fails when colour changes the spacing (checked by planting a space in coloured cells). `a_process_records_its_parents_name_and_its_children` compared a `sleep`'s parent name with its shell's name from a later read of the process table. On macOS `sh` is a launcher that turns into `bash`, and under load one read could see the shell as `sh` and the `sleep` that `bash` started (the unit tests failed 4 of 8 runs on a Mac, 0 of 8 after); it now compares the two from the same read. `why_a_port_nobody_listens_on_warns_and_exits_not_found` picked a port it called free because binding `0.0.0.0` there succeeded, but on macOS that bind succeeds while another process listens on `127.0.0.1`, and on the Nix macOS job `proc why` found a holder on it and exited 0. Its port lookup now also checks that nothing answers on `127.0.0.1` or `[::1]`, as the `ports` and `free` tests' lookups already did. Nothing changed in proc

## [2.4.0] - 2026-11-11

### Added

- **NetBSD's pkgsrc, `sysutils/proc-cli`.** pkgsrc imported proc 2.2.0 on 2026-11-04, from the pkgsrc-wip package, which is gone. Each release now goes to pkgsrc as a diff a pkgsrc developer commits (RELEASE.md, "After the tag: pkgsrc"); the README and the install page name the package

### Changed

- **A name that matches a process proc runs under says so, instead of leaving it out without a word.** Name targets and `--by` skipped proc itself and the processes it runs under (its shell, the agent that ran it, up to 10 levels) and said nothing: from zsh, `proc by zsh` listed every zsh but the user's own, over ssh `proc by sshd` found no sshd, and `proc kill zsh` gave no sign of the shell it spared. proc itself is still never a match. The commands that only read (`by`, `list`, `in`, `info`, `why`, `on`, `tree`, `watch`) now list such a process when its name matches, and say so after the list: `ℹ Includes zsh (PID 81340), which runs this proc`. `--cmd`, the commands that act (`kill`, `stop`, `freeze`, `thaw`, `unstick`) and `wait` (which would wait for a process that can't end while proc runs) leave it out and name it: `⚠ Left out: zsh (PID 81340), which runs this proc`. Under `--cmd` such a process counts only when its name matches, or its executable or argv[0] exactly: its command line holds proc's own arguments (`sh -c 'proc by --cmd X'` has `X` in it), so a hit there alone is no match, as before, and `--cmd` searches run through a shell report nothing about it. When every match was left out, the error is exit 2, `reason: "ruled_out"`, `sshd (PID 1329) matches 'sshd', but proc runs under it, so it's left out` (it was `No process found matching 'sshd'`), and `wait` returns at once. In JSON, every process object for a process proc runs under, in every command, has `"runs_proc": true` (absent on every other), and a document or error whose name left processes out has `left_out`, `[{"pid", "name"}]`; both keys are added, no other changes. `unstick` with targets that match nothing now answers as `kill` does (a port another user's process holds is exit 3, a name that matched only proc's ancestors the error above), not always `No process found`. **Library API:** `Process` has a new public field `runs_proc` (struct literals of it need the field); `Ancestor`, `Ancestors`, `NameLookup`, `Process::lookup_name`, `proc_cli::core::lookup_target`, `ProcError::AncestorsOnly` and `ProcError::left_out` are new (`ProcError` is not `#[non_exhaustive]`, so an exhaustive `match` on it in library code must handle the new variant); `Resolved` gains `left_out` and `included` (struct literals need them) and the methods `note` and `left_out_ancestors`; `nothing_resolved` takes the left-out processes as a third argument; `Printer::with_left_out` and `Printer::note` are new; `resolve_targets` keeps such a process (marked) and `resolve_targets_excluding_self` leaves it out; `Process::find_by_name` still leaves such processes out without saying so, and a name whose only matches were such processes is `ProcessNotFound` there, without the count of command lines that `NameNotFound` carries

- **The Docker image is a static binary on distroless.** It was the glibc binary on `debian:bookworm-slim`, whose packages Docker Scout flagged with 53 known vulnerabilities on 2.3.0 (6 high: util-linux, perl, glibc, zlib), none of them in proc and none fixed in Debian yet. The image is now the static musl build, the same one the Linux release binaries are, on `gcr.io/distroless/static-debian13`, which holds no shell and no package manager. `docker run --rm -it --pid=host --net=host yazeed/proc-cli` works as before, as root, on linux/amd64 and linux/arm64. **What changes:** there is no shell to `docker exec` into or to give `--entrypoint sh`, and no `apt-get` for an image built `FROM` it; `COPY --from=yazeed/proc-cli /usr/local/bin/proc` now works in any Linux image, since the binary needs no C library. A process's user is named from the image's `/etc/passwd`, which now lists root, nobody and nonroot only, so with `--pid=host` other host users show as their number (as they did whenever Debian's list had no such ID)

- **Every action and base image is pinned, and Dependabot moves the pins.** `ci.yml` used floating tags (`actions/checkout@v4`, `dtolnay/rust-toolchain@stable` and four more); they are pinned to commit SHAs with the version in a comment, as the release workflows were. The Dockerfile's `rust:1-alpine` and `gcr.io/distroless/static-debian13` are pinned to digests. `.github/dependabot.yml` opens weekly pull requests that move them (`winget-releaser` stays on its commit until the first automated winget update)

- **The release's dry run builds the new version's nixpkgs, FreeBSD and pkgsrc packages before the tag.** Those three jobs download the release's source from GitHub, which has none until the tag exists, so a dry run built the last release instead, and the new version's packaging was first tried by the tag's own run (2.4.0's dry run, started after the version bump, failed all three on a 404). A dry run now gives them a tarball of its checkout (`git archive --prefix=proc-cli-X.Y.Z/`), which holds what GitHub's download of the tag will: for 2.3.0 the two gave the same Nix source hash. RELEASE.md runs that dry run after the release commit and tags the commit it tested. A tag's run downloads from GitHub, as before

- **The release commits its package files to main itself.** The nixpkgs, FreeBSD and pkgsrc jobs write the files submitted for the release (`package.nix`, the port's and the package's files with their checksums), and RELEASE.md had the maintainer download them from the run and commit them by hand after every release. They can't be in the release commit: their checksums are of the tag's download, which holds that commit's ID (GitHub's v2.3.0 tarball names commit 9ca5022). A new last job, `commit-package-files`, commits them to `main` as `chore: package files for vX.Y.Z`, each job's files only when that job passed; a dry run shows the change and commits nothing

- **A build on a system proc doesn't support says so.** proc reads processes, ports and open files with code for macOS, Linux, Windows, FreeBSD and NetBSD. Built anywhere else, illumos (SmartOS, OmniOS) for one, it failed with ``cannot find value `ports` in this scope``, which named no cause. The build now stops first with "proc supports macOS, Linux, Windows, FreeBSD and NetBSD, and this system is not one of them". ROADMAP.md lists illumos as a candidate

### Fixed

- **`proc for` finds open files on Linux without lsof.** It asked `lsof` which processes had the file open, and where lsof was missing (the Docker image, minimal containers and servers) it found none and said "No process found", exit 2, though processes held the file. On Linux proc now reads `/proc` itself, for every way lsof reports a file in use: an open descriptor, the working directory, the root directory, the executable and a file mapped into memory. On macOS (lsof), FreeBSD and NetBSD (fstat), a missing program is an error that names it, exit 4 (`not_supported`), not an empty answer
- **A port another user's process holds is in use, not "not found".** On Linux and macOS, as a user other than root, proc can't trace another user's socket to its process, and it left those sockets out: `proc on :80` with root's nginx on port 80 answered "No process listening on port 80", exit 2, `proc free :80` called the port already free, and `kill`, `stop`, `freeze`, `thaw`, `info`, `why` and `wait` said the target was not found. The socket tables (Linux's `/proc/net`, macOS's `netstat`) list every listening socket, so proc now checks them when it finds no holder: a port something listens on is "Port 80 is in use, but proc can't see which process holds it: it belongs to another user", exit 3 (`reason: "permission_denied"`, with `sudo proc <command>` as the way to see it), and among several targets the warning says that instead of "Not found". **This changes an exit code in a minor release:** a script that read exit 2 as "the port is free" for such a port now gets 3, which is the bug fixed. A port nobody holds is exit 2 as before. FreeBSD's and NetBSD's `sockstat` lists every user's sockets too, unless the system hides other users' (FreeBSD's `security.bsd.see_other_uids=0`, NetBSD's `security.curtain=1`): there proc answered "No process listening", exit 2, and `free --dry-run` called the port already free. As a user other than root on such a system proc now says it can't tell, "proc can't tell whether port 22 is in use: this system hides other users' sockets (security.bsd.see_other_uids=0)", exit 3, `permission_denied`. Tested as a user against root's sshd on FreeBSD 15.1 and NetBSD 11.0, with the setting off (the holder is named, as before) and on. Windows lists every socket with its process to every user (tested as a standard user against administrator and system listeners), so nothing changes there. **Library API:** `ProcError::PortUnseen(UnseenPort)` is new (an exhaustive `match` on `ProcError` in library code must handle it), with `proc_cli::core::UnseenPort` (the port and why) and `proc_cli::core::Unseen` (`OtherUser`, or `Hidden` with the setting); `PortInfo::holders_seen` and `PortInfo::unseen` are new, `Resolved` gains `unseen` (struct literals of it need the field), and `proc_cli::core::nothing_resolved` is new
- **`proc ports` says which ports it leaves out.** The list leaves out ports other users' processes hold, since proc, when it isn't root, can't see their sockets, and it didn't say so. On Linux and macOS a line after the table now names them, `⚠ 2 ports in use by other users' processes, which proc can't see: 22, 80 (run with sudo to see them)`, and the JSON gains `unseen_ports` (`[]` for none; an added key, the others unchanged). It's left out with `--by`, `--in`, `--exposed` or `--local`, which proc can't apply to a holder it can't see; `--range` applies. On FreeBSD and NetBSD set to hide other users' sockets, proc can't name the ports it leaves out, and a line says why the list may be short, `⚠ This system hides other users' sockets (security.bsd.see_other_uids=0), so the list has only your own processes' ports (run with sudo to see them all)`, with `hidden_by` in the JSON (the setting; absent where nothing hides them or proc is root). **Library API:** `PortInfo::listening_unseen` is new
- **`proc ports` on Windows names every holder.** Most rows read "unknown" (`svchost.exe`, `System`, `sshd.exe`), as administrator too, while `proc on` named the same processes. A process listening on IPv4 and IPv6 has a row for each, so its PID was looked up twice, and the process library proc uses drops a process from the answer when one lookup asks for its PID twice. Elsewhere the socket table's own name stood in, so only Windows showed it. Each PID is now looked up once
- **On FreeBSD and NetBSD a daemon that rewrites its title is named after its program.** proc named sshd `sshd [listener] 0 of 10-100 startups`, in every command and as root too, and `proc by sshd` didn't find it: the name came from the process's argv[0], which sshd rewrites into a status title. When argv[0] is a title (it has a space, or starts with `-` as a login shell's does), proc now takes the kernel's name for the process, as `ps -o comm` does, and as proc already did on Linux and macOS
- **The documented `docker run` finds the processes of users other than root.** It was `docker run --rm -it --pid=host --net=host yazeed/proc-cli`, and Docker drops `SYS_PTRACE`, without which root in the container cannot read the sockets and open files of another user's process: `proc on :4799` answered "No process listening on port 4799" while a dev server held it, and `proc for` found no holder. The command now adds `--cap-add SYS_PTRACE --security-opt apparmor=unconfined` (README, install page; Docker Hub shows the README): on a host with AppArmor, as Ubuntu and Debian are, Docker's default profile blocks the reads even with the capability. Measured on GitHub's Ubuntu runner, with the listener as the runner's own user: no flag and the capability alone found no holder, both flags found it. The image's CI check, which runs as that user, found it
- **`proc for` leaves proc itself out.** Asked about the directory it runs in (`proc for .`), it listed its own process, since its working directory is that directory; every other lookup leaves proc out, and this one does too
- **The apt repository gets the release's packages.** On the 2.3.0 release it kept listing 2.2.0. A Pages deploy is named after its commit, and Pages keeps the first deploy of a commit: the release commit's push to main deployed the site before the release had its `.deb` files, and the release's own deploy of the same commit, which had them, changed nothing. The push of a release commit (`chore: release v…`) no longer deploys; the release does, once its packages exist. 2.3.0's packages were added by the next push to main
- **The release's FreeBSD port job installs its tools again.** It moved pkg to the `latest` branch by adding a second repository, `FreeBSD`, beside FreeBSD 15's own `FreeBSD-ports`, both with the same packages. Once a new pkg was out, `pkg install` tried to upgrade pkg from one and found it current in the other, again and again, for an hour, until the job was killed (2.4.0's dry run); the release would have stopped there too. The job now moves `FreeBSD-ports` itself to `latest`. Checked in the same VM: pkg upgrades itself once, and the port builds, installs and runs
- **The FreeBSD and pkgsrc jobs build with the crates the release locks.** Both extracted the source with the last release's crate list, then wrote the new one and built in the same work directory: pkgsrc's build failed (2.4.0's `Cargo.lock` locks itoa 1.0.19, the directory had 1.0.18), and FreeBSD's passed while compiling the old crates, so it didn't test the files it filed. Both now extract again after writing the list. Found by the dry run that builds the new version (above), before the tag; the tag's run would have failed the same way
- **Two tests no longer fail on a busy machine.** `coloured_text_is_the_plain_text_plus_escape_codes` compared the table header of two runs of `proc list -v`, but a column is as wide as its widest cell, and for an instant the listener was in "disk wait", which widened STATUS by one (CI, Linux). It now runs the pair until both print the same cells, then the whole text must match, spacing and all. `uptime_is_reported_in_seconds` allowed 2 seconds counted from when proc ended, and under the suite's load one `proc info` took up to 7 seconds (2 failures in 5 runs on macOS); it now checks that proc read the uptime while it ran. Both still fail on the bug each guards against (checked by planting one). Nothing changed in proc

### Security

- **The install script checks what it downloads.** `install.sh` installed the binary it downloaded without checking it. It now compares the file's SHA-256 with the release's `checksums.txt` and installs nothing on a mismatch, or when the file has no entry there, using the tool the system has: `sha256sum` (Linux), `shasum` (macOS), `sha256` (FreeBSD and NetBSD) or `openssl`. It also reads the latest version from where `github.com/yazeed/proc-cli/releases/latest` redirects, not from GitHub's API, which allows 60 requests an hour per address without a token, so the script failed on busy shared networks. Tested on macOS, on Alpine and Debian on x86_64 and arm64, and the checksum on FreeBSD 15.1 and NetBSD 11.0. The script is served from `main`, so this applies as soon as it is pushed, not at the next release

## [2.3.0] - 2026-11-03

### Added

- **`tree`, `why`, `wait` and `unstick` return whole process objects.** Their JSON carried a subset of a process's fields. Each process object they print keeps every key it had, with its name, type and position, and gains the fields it lacked after them, absent where the OS gives no value: `exe_path`, `user`, `parent_pid`, `start_time`, `parent_name`, `cpu_time_secs`, `open_files` and `child_count` in all four; `cwd` and `command` in `tree` (nodes and `--ancestors` chain) and in `wait` and `unstick`; `cpu_percent` and `memory_mb` in `why` (process and ancestry entries), `wait` and `unstick`; `status` and `uptime_secs` in `wait` and `unstick`. A `wait` process that exited carries the last reading proc had of it before it went, and a still-running one the reading taken when `wait` began. Human output is unchanged. **Library API:** none; the shared fields are a crate-private struct
- **Three more process statuses: `idle`, `starting` and `disk_wait`.** proc folded every status it did not name into `unknown`; these three are now told apart, in JSON (`status`), in the card (`Status: idle`, `starting`, `disk wait`), in the tables (`Idle`, `Starting`, `Disk wait`) and in `--status`. `idle` is a Linux idle kernel thread (state `I`). `starting` is a process still being created by `fork` (state `SIDL`) on macOS, FreeBSD and NetBSD, where the same sysinfo status means this, not an idle thread. `disk_wait` is Linux uninterruptible sleep (state `D`, waiting on the disk or other I/O that a signal cannot cut short). They used to read `unknown`, so **a script that matched `"unknown"` for such a process now sees `"idle"`, `"starting"` or `"disk_wait"`**; every other status is as before (the rest of sysinfo's statuses, such as tracing, still read `unknown`). `--status` takes `idle`, `starting` and `disk-wait` (`disk_wait` too) on `list`, `by`, `in`, `for` and `tree`; `proc stuck` still treats a process in disk wait as blocked, as before. **Library API:** `ProcessStatus` gains `Idle`, `Starting` and `DiskWait`, and `StatusFilter` gains `Idle`, `Starting` and `DiskWait`: code that matches either enum exhaustively must handle them.
- **A process's JSON names its parent, counts its children and gives its CPU time and open files.** Every process object (`list`, `by`, `in`, `for`, `info`, `on`, `orphans`, `stuck`, `watch`, and the other commands that print one) gains four optional keys: `parent_name` (the parent's name, as `name` records it), `child_count` (how many processes have it as their parent, `0` for none), `cpu_time_secs` (whole seconds of CPU time used so far) and `open_files` (how many files it has open). A value the OS does not give is an absent key, never `null`: `open_files` is absent on Windows (sysinfo counts every kernel handle there, not only files), on NetBSD without procfs mounted, and for another user's process on macOS, where `cpu_time_secs` is absent too (and for a Windows process proc cannot read); `parent_name` is absent when the parent has exited. The two that need other processes are filled from the process table every lookup already reads, so `proc info <pid>` costs about 7 ms more and `proc list` about 1 ms (measured best of 21 on a loaded Mac, 269 to 276 ms and 314 to 315 ms), and `cpu_time_secs` reads NetBSD's kernel counter as the CPU percentage does. Human output is unchanged for now. **Library API:** `proc_cli::core::Process` has four new public fields (`parent_name`, `cpu_time_secs`, `open_files`, `child_count`), so code that builds a `Process` with a struct literal needs them (all `None` for an unknown value)
- **An apt repository for Debian and Ubuntu, at https://proc-cli.com/apt.** The `.deb` files were only on the releases page, so `apt upgrade` never updated them. The repository holds the same files for x86_64 and ARM64, signed (key `19AD 1FFA 67E9 0A13 9579 7473 DC42 B7C2 51B0 33F3`), and every release adds itself, so after adding it once `apt upgrade` keeps proc current. https://proc-cli.com/apt/ gives the commands. The site and the repository now deploy together from `.github/workflows/pages.yml`, which builds the repository from the release files after checking them against `checksums.txt` (`pkg/apt/build-repo.sh`), so no binaries are kept in git. Tested in Debian 13 and Ubuntu 24.04: install, upgrade to a newer version, and refusing a tampered repository or a wrong key
- **winget, `winget install yazeed.proc-cli`.** The first version is submitted (microsoft/winget-pkgs#469298) and has passed winget's validation, install test and antivirus scan; it waits for a moderator. Once it is merged, each release opens the pull request for its own version. The manifests are in `pkg/winget/`
- **Fedora, RHEL and its rebuilds, and Amazon Linux, `dnf copr enable yazeed/proc-cli`.** COPR builds proc 2.2.0 from source for Fedora 43, 44, 45 and rawhide and for EPEL 9 and 10 (RHEL, AlmaLinux, Rocky) and for Amazon Linux 2023, on x86_64 and ARM64, and Fedora's new releases join by themselves. Installed and run from the repository in Fedora 44, EPEL 9 and 10, and Amazon Linux 2023. Every release now builds itself there: the release workflow sends the version to the project's webhook, COPR makes the source RPM with `pkg/copr/sources.sh` (the spec at that version, the tag's tarball and the vendored crates) and the job fails if the build does. `pkg/copr/srpm.sh` builds the same source RPM locally. On Amazon Linux, whose Rust RPM macros are an older generation, the spec builds with cargo directly
- **Ubuntu, `ppa:yazeedaloyoun/proc-cli`.** proc 2.2.0 is published for Ubuntu 22.04, 24.04, 26.04 and 26.10, on amd64 and arm64, built by Launchpad from source with the crates vendored (it builds without network) and Rust 1.91 on the first two, whose default Rust is older than proc needs. The source packages are signed with the maintainer's key, which stays on the maintainer's machine, so each release uploads them with one command there (`pkg/ppa/release.sh`, RELEASE.md "After the tag: Ubuntu PPA")
- **Packaging for a FreeBSD port**, in `pkg/freebsd/`. The port (`sysutils/proc-cli`) builds, passes the ports tree's checks and runs from its package on FreeBSD 15.1. It goes through FreeBSD's review before it is published
- **The release checks every credential before it publishes anything.** A new first job tries each secret the release uses (the Homebrew, Scoop and winget tokens, the Docker Hub login, the apt signing key and the COPR webhook) and reads when each expires: an invalid or expired one stops the release, and one that expires within 30 days is a warning. The dry run runs it too, so RELEASE.md's first step catches a token that needs renewing before the tag. No expiry date is stored: GitHub reports a token's own
- **PyPI, `pip install proc-cli` (also `pipx install proc-cli` and `uv tool install proc-cli`), from 2.3.0.** proc was not on PyPI. Each release publishes five wheels (macOS on Apple silicon and Intel, Linux on x86_64 and ARM64 for glibc and musl, so Alpine too, and Windows on x86_64), each holding the very binary the GitHub release has, so no Python code runs and nothing is built twice: `pkg/pypi/build-wheels.py` repacks the five binaries `build-release` already built and checked, as the `.deb` and `.rpm` files do. The release publishes them through PyPI trusted publishing (no token), and only after each wheel has been installed with pip on its own OS (Ubuntu x86_64 and ARM64, Alpine, macOS arm64 and Intel, Windows) and printed the release's version. Checked on 2.2.0's binaries: `twine check`, the RECORD hashes, and installs in a venv on macOS arm64 and in Debian-based Python images (arm64 and amd64) and Alpine
- **Every port says whether it is reachable from the network.** Each port object in JSON gains `exposure`: `"local"` when the port is bound to loopback only (`127.0.0.1`, `::1`), and `"exposed"` for all interfaces (`0.0.0.0`, `::`), any other address, or no known address, the same split as `is_local()` and `is_exposed()`. It sits on every port object, as in `ports` and `on <pid>`. Only a key is added: none is renamed, removed or retyped, and human output is unchanged for now. **Library API:** `PortInfo::exposure()` returns the new `Exposure` enum (`Local` or `Exposed`, re-exported from `proc_cli::core`), and `PortInfo::exposure_words()` returns the words people read (`this machine only`, `all interfaces, reachable from the network`, `network address <addr>`). `PortInfo` serializes by hand, with the same fields in the same order and `exposure` last, so struct literals of `PortInfo` need no change
- **`proc on` reports the connections to a port.** `proc on :3000 --json` gains `exposure` (`"local"` or `"exposed"`, next to `address`, the first holder's) and `connections`: `total`, `by_state` (`established`, `close_wait`, `time_wait` and so on) and `peers`, each with the `holder_pid` whose socket it came in on, its `state`, the client's `remote_address` and `remote_port` and, when the client is a process on this machine, its `peer` (`pid`, `name`). `peers` lists 50 at most and `peers_truncated` says the list was cut. Each port object of `on <pid>`, `on <name>` and several targets gains `connections` with `total` and `by_state` only, counting the connections that came in on that process's own sockets, so two processes holding one port number each get their own share. A port with none has `total: 0`; where proc reads none (FreeBSD and NetBSD) the key is absent, never `null`. Only keys are added. The connections are read with every holder of the port, so a `0.0.0.0` holder cannot take another holder's connections, and in the background while the CPU is measured: `proc on :<port>` and `proc on <pid>` stay within 0.02 s of 2.1.1's 0.34 to 0.38 s (best of 3, release builds, macOS, a listener with 7 connections). The socket table is read once per command, not once per port. **Library API:** new in `proc_cli::core::connections` (all re-exported from `proc_cli::core`): `connections_to(port, holders, peer_limit)` and `connections_for(ports, peer_limit)` (several ports from one reading), and the `Connections`, `Peer` and `PeerProcess` types they return
- **One set of JSON names: the receipt lists `processes`, and counts and durations are named the same way everywhere.** The JSON said the same thing two or three ways, so a script that read more than one command had to learn each. The names now follow one rule: a list's key says what is in it; a count is `count` for the main list and `<list>_count` for another list beside it, or `<outcome>_count`; a duration ends in `_secs`; one key never means a list in one command and a number in another. Every new key is added after the existing ones of its object, and every old key keeps its name, type, position and presence, so 2.x scripts keep working. What is new: the receipts of `kill`, `stop`, `freeze`, `thaw`, `stuck --kill` and `orphans --kill` (`--yes --json`) gain `processes`, every process acted on, in the order of the preview (the targets as typed, then PID ascending), each a whole process object with `outcome` (`"succeeded"` or `"failed"`, and `error` on a failure, the text `failed[].error` has) and `count`; the receipt of `free` gains `processes` (the holders it stopped, each with `outcome`) and `count`; `info` gains `count`; `wait` gains `count`, `elapsed_secs`, `targets` (the targets as a list, as typed) and `exited[].exited_after_secs`; `unstick` gains `count`, `recovered_count`, `not_stuck_count`, `still_stuck_count`, `terminated_count` and `failed_count`. `succeeded`, `failed`, `succeeded_count`, `failed_count` and `results` stay and are not deprecated. A test reads the `--json` of every command and checks it against the rule, with an explicit list for the deprecated names. A port row keeps `process_name` and `pid` (the rule prefixes a field about something else). `skills/proc-cli/reference.md` has every shape. **Library API:** new `Printer::with_processes(&[Process])`, which gives the receipt of `print_action_result` and `print_action_result_as` the preview's list to follow; without it the receipt lists the succeeded processes, then the failed ones. The two methods keep their signatures.

### Changed

- **`proc on` prints one card for a process, by port, PID or name.** The three views printed different things under different labels: by port, a short block with the directory, `Listening:` and `Resources:`; by PID or name, `is listening on:` followed by "No listening ports" for a process with none, and no command line or parent. Now each process gets the card: `Process:`, `Command:`, `Directory:`, `Path:`, `User:`, `Parent:` (name and PID), `Status:`, `Resources:` (CPU, memory, CPU time, open files), `Started:` (with uptime, replacing `Uptime:`), `Children:`, `Listening:` (every port, with TCP, the address and who can reach it) and `Connections:` (a count by state, then 5 peers and `… and N more (-v for all)`). A line is left out when the OS does not give the value. The headline by port is `✓ Port 3000 is used by 1 process`, or `by 2 processes`, with `, reachable from this machine only` or `, reachable from the network` added when every holder is bound alike; by PID it is `✓ node (PID 1234)`; by name `✓ 2 processes named 'node'` (`matching` with `--cmd`). A process that listens on nothing shows `Listening: none`. `-v` prints every value whole instead of cutting it to the terminal width, and lists every connection; `--help` gains `proc on 1234`. Exit codes and `reason` values are unchanged, and so is the JSON apart from the added keys. The directory and path, once `-v` only for a PID, are always shown. **Library API:** a new public module `proc_cli::ui::card` draws the card: `write_card(out, process, ports, opts)` writes it, `CardPort { port, connections }` is a listening port with the connections to it, and `CardOptions { verbose, width, now_secs }` says how it is drawn. `ports` is an `Option`: `None` means the ports are not known and leaves out `Listening:` and `Connections:`, `Some(&[])` prints `Listening: none`.
- **`proc info`, `proc for` (one hit) and `proc why` print the same card as `proc on`.** `info` printed `Name:`, `PID:`, `Parent PID:`, `Status:` and `CPU:` in a layout of its own; `for` with one hit printed a shorter block with `Uptime:`; `why` printed the ancestry and nothing else about the target. Now each shows the card (`Process:`, `Command:`, `Directory:`, `Path:`, `User:`, `Parent:`, `Status:`, `Resources:`, `Started:`, `Children:`, `Listening:` and, for a process that listens, `Connections:`). `info` starts with `✓ python3 (PID 39158)` like `on <pid>` (it was `✓ Process 39158`), `for` keeps `✓ Found 1 process for ./server` above the card, and `why` prints the card under its ancestry chain, which no longer has the `dir: <cwd>` line under the target (the card's `Directory:` says it). `why <pid>` now waits 0.2 s for the CPU reading the card's `Resources:` needs (0.108 s before, 0.319 s after on macOS, best of 7, release builds); the ports and connections are read during that wait, and a port target reads the socket table once (`why :3000` ran `lsof` 4 times, now 2: the listing and the connections). A process that listens on nothing shows `Listening: none` and no `Connections:`. `-v` prints every value whole and every connection instead of cutting values to the terminal width (it used to add the command line in `info` and `for`, which the card now always shows). Several hits in `for` keep the table. `--json` only gains keys: each process in `proc info --json` has `ports`, the ports it listens on, each with `exposure` and `connections` (`total` and `by_state`, as `on <pid>` gives them), or an empty list; `for` and `why` JSON are unchanged by this entry. Exit codes and `reason` values are unchanged. When the OS gives no port table (for example macOS without `lsof` on the PATH), the card leaves out `Listening:` and `Connections:` rather than say `none`, `info --json` and `for --json` have no `ports` key, and the command still works. The listening ports and their connections are read while the CPU is measured, so `proc info` takes as long as before
- **Every process table shows the user, the uptime and the command.** `list`, `by`, `in`, `for` (several hits), `orphans`, `stuck`, `watch` and the list in front of every `--dry-run` and every `[y/N]` question each had their own columns (`PID DIR NAME ARGS CPU% MEM STATUS`, with UPTIME only on terminals 100 columns wide or more) and `-v` printed a block of lines under each process. Now they print one table, `PID NAME USER CPU% MEM UPTIME COMMAND`: UPTIME is there at any terminal width, USER and COMMAND are new, and a value proc cannot read is `-`. COMMAND is the whole command line (it was ARGS: the arguments, with paths shortened to file names) and takes the width the other columns leave, ending with `…` where it is cut; every other column keeps its width. With `--in`, DIR is the second column (`PID DIR NAME ...`), cut in the middle before it takes more than a third of the line. `-v` keeps the table and adds `PPID STATUS DIR`, cutting nothing (the block layout under each process is gone, and so is the `exe:` line: the path is in `proc info`); `--wide` prints the directory and the command whole. STATUS and DIR left the default table (`-v` has them). The dry-run table and the list before a question print the command line whole at any width, since the person reading it is approving exactly those processes; they used to be one line per process (`→ node [PID 42] - CPU: 1.5%, MEM: 12MB`) for `kill`, `stop`, `freeze`, `thaw` and `free`. `watch` draws the same table (its header and keys are unchanged, and its one-shot form without a terminal now takes `--wide` and `--in` like the other views). JSON is unchanged. **Library API:** `proc_cli::ui::table` is new (`process_table`, `TableLayout`, `Screen`), `Printer` gains `scoped_to_dir` and `print_processes_to_approve`, and `process_table_budget` and `ProcessTableBudget`, which sized the old columns, are deprecated (nothing uses them now) and removed in 3.0
- **`proc ports` shows what each listener is.** The table was `PORT PROTO PID PROCESS ADDRESS` with the executable's path on a line of its own under `-v`; it is now the shared table `PORT PROTO ADDRESS EXPOSURE PID NAME UPTIME COMMAND`. EXPOSURE is `local` or `exposed`, the words the JSON uses; NAME is the one name every view shows; UPTIME and COMMAND are the holder's, and COMMAND takes the width the other columns leave and ends with `…` where it is cut. `-v` adds `USER MEM` and the executable's path (`PATH`) and cuts nothing (the `↳` path line is gone). A value proc cannot read is `-`, so a missing address is `-` where it was `*`, and a holder that ended after its port was listed keeps its port cells and the name its socket gave, with `-` in the rest. JSON: each port gains `command`, `user`, `uptime_secs` and `memory_mb` from its holder (`memory_mb` is left out, and MEM shows `-`, when the OS refused the reading), and with `-v` `connections` (`total` and `by_state`, no `peers`, counting the connections that came in on that process's own sockets, so a process on both `127.0.0.1` and `::1` has the same counts on both rows). A value the OS does not give is an absent key, never `null`; only keys are added, none is renamed, removed or retyped, and the existing keys keep their order. The holders are read once for all the ports, by one refresh of exactly their PIDs, where `--by` and `--in` looked each one up (`--in` once per port) and `-v` read the whole process table once per holder; the connections are read only with `-v`, in one reading of the socket table. Measured on macOS, release builds, best of 15 (14 listeners): `proc ports` 95 ms before, 99 ms after; `proc ports --json` 95 and 93 ms; `proc ports -v` 0.69 s before, 0.19 s after; `proc ports --by <name>` 0.14 s before, 0.10 s after. `--local`, `--exposed`, `--by`, `--in`, `--range`, `--sort` and `--limit` behave as before. **Library API:** new `Process::find_by_pids(&[u32]) -> Vec<Process>` (the running processes among the PIDs, each read in full with one refresh of those PIDs; a PID that is gone is missing; no `parent_name` or `child_count`), and in `proc_cli::ui::table` `port_table(&[PortRow], &TableLayout)` with `PortRow { port, process }`, drawn on the same width policy as `process_table`
- **The agent skill's rule 7 branches on `reason` and `ruled_out_by`, not on the message text.** After a guarded `stop` or `kill` exits 2, the skill told the agent to read the words in `error` (`No process found matching '<pid>'` against `(ruled out by --start-time …)`). It now says to read `reason` (`not_found`: the PID is gone; `ruled_out`: a process holds it and a guard rules it out) and `ruled_out_by`, the flags of the guards that did, such as `["--start-time"]` (a new process has the PID: yours is gone) or `["--by"]` (check the PID with `proc info` before treating it as gone). The `error` text stays what the agent shows the user, because it can quote a process's name. The skill also describes the shared process keys (a missing key is unknown) and the card that `on`, `info`, `for` and `why` print, and `evals.md` gains two tasks for them (15 and 16). `reference.md` is checked against the real `--json` of `on`, `info`, `for`, `why`, `ports`, `tree`, `wait` and `list`, and now says that `info` with no matching PID or name answers with `reason: "not_found"` like the other commands, that `ruled_out_by` can hold several flags, and that the ports in `for` and `why` have no `connections`. The README, `docs/`, `pkg/pypi/README.md` and the Rust docs show `proc on <pid>` wherever `proc on :3000` appears and describe the card and the whole process objects of `tree`, `wait` and `unstick`; `proc in --help` no longer says `-v` adds DIR, which that table always has; ROADMAP.md drops the entry for this release.
- **The agent skill closes the gaps its evals found.** The evals that read `on`, `info`, `ports` and guard results ran on a small model and a large one, with two wordings each (Haiku 14 of 22 and Sonnet 20 of 22 the first time; 9 of 10 and 10 of 10 on the ones that had failed, after these changes). Exit 4 with `needs_confirmation`: a preview of exactly the PID the user named, matching what they described, is the approval to re-run it with `--yes` and its guards; a process the user only described in words is shown first (rule 6). Rule 7: the agent says nothing is gone until `proc info` has answered, and quotes the `error` and the process's command when it reports a guard's refusal. Stopping a server it started is a checklist: `proc tree` for the children, stop children then parent, and verify each PID and the port before saying it is stopped. Reachability is answered from `exposure`, not worked out from `address`. `--cmd` is not added once the exact name already found processes, and a cleanup also looks at `proc in <dir>` for processes that listen on no port

- **PHILOSOPHY.md names proc's two readers.** A new value, "Easy for People, Complete for Programs": the default output is for people (the answer first, in words, cut to the terminal; `-v` for everything), and `--json` is for programs, where a script needs it stable and an agent needs it complete, with unknown values left out and facts, never instructions. The feature test gains a seventh question, "Does it serve both readers?", and ROADMAP.md plans an audit of every command against it for 3.0. Ideas the file said twice are now said once: the target grammar under Unified Targets, and the one-word reason for a failure under One Contract
- **PHILOSOPHY.md says what fast means, and how habits fit "Easy to Remember".** "Fast" said proc answers right away, but a command that shows CPU waits about 0.2 s, because a CPU reading compares two samples. It now names two clocks: the machine's, where proc waits only when the answer needs time, and the human one, how long someone new takes to reach a correct answer. "Easy to Remember" now asks for one obvious spelling per intent, meaning the same in every command, with habits from other tools kept working. ROADMAP.md plans the guesses that still miss this as one convention "Semantic" now says what it is for: commands are the words a newcomer would guess, in the sense people already use them
- **Long lines in the website's code blocks show that they continue.** A command wider than its block ran off the edge with no sign of the rest, since macOS hides scrollbars until you scroll. A soft glow now marks the side a line continues on (and goes once you reach that end), and a thin scrollbar stays visible; it is CSS only. The install page's `.deb`/`.rpm` and Nix examples now fit at desktop width, with their comments on their own lines
- **The install docs list every channel by system, and say whether it updates by itself.** The website's install page, the README, the FAQ, RELEASE.md's package table and the npm and PyPI pages now group the ways to install proc as macOS, Windows, Linux by family (Debian and Ubuntu: the apt repository, the PPA and the `.deb` files; Fedora, RHEL and Amazon Linux: COPR and the `.rpm` files; Arch: the AUR), FreeBSD and NetBSD, any system through npm, cargo or PyPI, Nix, Docker and the release files. Each entry names who maintains it and how it updates (`brew upgrade`, `apt upgrade`, `dnf upgrade`, or a new run or download). winget, the nixpkgs package and the FreeBSD port are not listed as installable until they are merged

### Deprecated

These names stay in the JSON through 2.x, with the same value, and are removed in 3.0. Use the new name beside each.

- **`info`:** `found_count`, use `count`.
- **`wait`:** `initial_count`, use `count`; `elapsed_seconds`, use `elapsed_secs`; `exited[].exited_after_seconds`, use `exited[].exited_after_secs`; `target` (every target joined by a space), use `targets` (a list).
- **`unstick`:** the bare numbers `found`, `recovered`, `not_stuck`, `still_stuck`, `terminated` and `failed`, use `count`, `recovered_count`, `not_stuck_count`, `still_stuck_count`, `terminated_count` and `failed_count`. `failed` is a number here and a list in the receipts of the other commands; `failed_count` is the same name everywhere.

### Fixed

- **On macOS a process that had exited but not been reaped was "not found" by PID.** `proc list` showed such a process (a zombie, `Z <defunct>` in `ps`), but `proc info <pid>`, `on`, `why` and `tree` answered `not_found` (exit 2), `proc watch <pid>` found nothing and `proc watch` listed no zombies, `wait <pid>` said "No process found" instead of counting it as exited, and `kill`, `stop`, `freeze` and `thaw` called it "not found" and failed, because sysinfo leaves a zombie out of a macOS lookup. A PID lookup now finds it, with status `zombie`, as `list` shows it and as Linux does, and `watch` lists and finds it too (one extra kernel query per refresh); `wait` counts it as exited (exit 0); and a signal to it is delivered and changes nothing, so `kill`, `stop`, `freeze` and `thaw` succeed as they do on Linux. Only a lookup that finds nothing asks the macOS kernel (for that one PID), so finding a running process costs what it did. A test makes a zombie, waiting for `ps` to show it rather than for a fixed time, and the test that `wait` counts zombies as exited no longer depends on proc starting within a second of its targets. Linux, Windows, FreeBSD and NetBSD are unchanged
- **A process proc cannot read shows only what it read.** For another user's process (`proc info 1` as a user on macOS) the card printed `Status: unknown`, `Resources: 0.0% CPU, 0KB` and `Listening: none`, and the tables printed `0.0` and `0KB`: values proc never read, shown as measurements, and a `none` for ports proc cannot see (lsof and `/proc/<pid>/fd` show nothing of another user's sockets unless proc runs as root). Now the card leaves out `Status:` when the OS gave none, CPU when it did not read the process, memory when it is 0, and `Listening:` and `Connections:` when proc cannot see the process's sockets; the tables show `-` for CPU% and MEM there; `proc info --json` leaves out `ports` for such a process (the key is new in this release). The JSON keys that 2.2.0 already had (`status`, `cpu_percent`, `memory_mb`, and `ports` of `on`, `for` and `why`) keep their values and presence, so a consumer reads `status: "unknown"` as unknown, and an empty `ports` of another user's process, when proc is not root, as not seen. Ports that were found are shown whoever owns the process. **Library API:** none changed.
- **`tree` and `wait -v` leave out the figures of a process proc could not read.** For another user's process (`proc tree 1` as a user on macOS) a tree line printed `0.0% 0KB` and `wait --verbose` printed `- 0.0% CPU, 0KB`: values proc never read, shown as measurements. They now print the line without them (`launchd [1]`, `-> launchd [PID 1]`), as the card does. `cpu_percent` and `memory_mb` in `--json` are unchanged.
- **`on`, `info`, `why`, `for` and `ports -v` read the socket table once per lookup on Windows and Linux.** (`info` and `why` read it once for all their targets, a port target included; `on` once for each target it is given.) The connections were read after the listing, each with its own reading: on Windows `on`, `info`, `why`, `for` and `ports -v` ran `netstat -ano` four times where 2.2.0 ran it twice, and on Linux every `/proc/<pid>/fd` was walked twice. Now one reading serves both: the listening ports and the connections to them are derived from the same tables (macOS keeps its two `lsof` calls, the listing and the connections of the ports, which are different queries). Measured on Linux (Docker, arm64, release build, a listener with 300 connections, best of 15): `on <pid>` 0.207 s before and after, since the connections are read during the 0.2 s CPU wait, and `ports -v` 9 ms before and 7 ms after; Windows was not measured here (by count, two `netstat` runs per command, where it was four). **Library API:** `connections_to` and `connections_for` keep their signatures and read the tables themselves.
- **Small corrections to the new views.** The sample cards in the docs showed `0.4s CPU time`, a value that cannot occur (the CPU time is whole seconds), and now show `0s`; a peer line cut to a narrow terminal keeps its `(PID n)`, as `Process:` and `Parent:` do, and a peer known only by its PID (`→ PID n`) keeps `PID n` whole, losing its address first; `child_count` no longer counts proc itself, so `proc tree <pid> --json` agrees with its `children` list; `proc_cli::core::process::names_by_pid` is crate-private again (it was made `pub` with no Library API note); the documentation of `connections_to` says to pass every holder of the port.
- **`proc info --json` says `reason: "not_found"` when nothing matched.** It was the one command whose not-found document had no `reason`, though the skill and reference say every error has one. The document keeps `found_count: 0`, `not_found`, exit 2 and every other key, and gains `error` (`Not found: …`), `exit_code` and `reason`, so it is an error document like the others.
- **The same process shows one name in every view.** On macOS, `proc ports` and `proc on :port` printed the name lsof gives (`python3.14`) where `list`, `by` and `on <pid>` printed the process list's (`python3`), so a name copied from `ports` did not match `by`. A port row now takes its name from the process list by PID; the socket table's name stays only for a process that has already exited. Every OS takes the name the same way, in one step: one lookup of exactly the listening PIDs per reading, and on Windows it replaces a full process refresh. `skills/proc-cli/reference.md` says `process_name` is the process's name, as `name` is
- **The terminal tests read all of proc's output on FreeBSD and NetBSD.** They failed now and then in CI on the BSDs: a key typed after proc had already exited (NetBSD, EIO), or proc's last lines missing (FreeBSD). The cause: the tests ran proc as the leader of its terminal's session, and when a session leader exits both kernels revoke the terminal, which drops what the test had not read yet (measured with a probe, 3 runs of 3 on each; a process that was not the leader kept all of it). The tests now start proc through `examples/session`, which holds the session and runs proc as its foreground job, as a shell does, and ends the way proc did once the output is read; and they answer a prompt with the key alone, as the prompt needs no Enter. Nothing changed in proc
- **The tests that need a free port check that nothing listens on it.** They picked a port by binding `0.0.0.0` to it, but on macOS that bind succeeds while another process listens on `127.0.0.1` at the same port (Rust sets `SO_REUSEADDR`), so a port a test listener held could pass as free, and `free_a_port_nothing_holds_is_already_done` failed in the macOS Nix build. A port now also has to refuse a connection on `127.0.0.1` and `[::1]`. Nothing changed in proc
- **The README names the AUR's current packages.** The Arch install rows said `yay -S proc-bin` and `yay -S proc`; with 2.2.0 the AUR packages moved to `proc-cli-bin` (the release binary) and `proc-cli` (built from source), and the old names stayed at 2.1.1 with no maintainer

## [2.2.0] - 2026-11-01

### Added

- **FreeBSD and NetBSD.** proc ran on macOS, Linux and Windows only. Each release now has x86_64 binaries for both (`proc-freebsd-x86_64` and `proc-netbsd-x86_64`, also as `.tar.gz`), which the install script, npm and `cargo binstall` download; on other CPUs, `cargo install proc-cli` builds it. Ports come from `sockstat` and open files (for `proc for`) from `fstat`, both in the base system, so nothing else is needed: pkgsrc's `lsof` found no process holding a file on NetBSD 11 x86_64. CI runs the whole test suite in FreeBSD 15.1 and NetBSD 11.0 VMs, then builds and runs the release binary in each (natively: cross's images lack `libgeom`, `libkvm` and `libexecinfo`, which proc links), and a release stops unless each binary prints its version in its own OS. What the suite found on the way: two threads reading processes at once crashed proc on FreeBSD (its libkvm keeps command lines in buffers the whole program shares), so every read now takes a lock; NetBSD CPU readings came out 0% for a process spinning at full speed (sysinfo rounds CPU time to whole seconds there), so proc reads the kernel's own counter; and NetBSD leaves a process's start and CPU time out of its record for a few hundred microseconds while the process takes a signal, which made `unstick` interrupt a process that had just recovered, so proc reads again while they are missing; and NetBSD's `poll` doesn't wake when a pipe's reader closes during the wait, so `watch --json | head -n1` there would have run on for a whole refresh interval: it looks again every 100 ms. One thing the kernels can't give: once a directory is deleted, neither keeps it as a process's working directory, so `proc in` a deleted directory exits 4 on both
- **`.deb` and `.rpm` packages on each release.** Debian, Ubuntu, Fedora and RHEL users had only the install script, npm or Homebrew. Each GitHub Release now attaches `proc-cli_X.Y.Z_amd64.deb`, `proc-cli_X.Y.Z_arm64.deb`, `proc-cli-X.Y.Z-1.x86_64.rpm` and `proc-cli-X.Y.Z-1.aarch64.rpm` (in `checksums.txt` too): `sudo apt install ./proc-cli_X.Y.Z_amd64.deb` or `sudo dnf install ./proc-cli-X.Y.Z-1.x86_64.rpm`. Each installs `/usr/bin/proc` (the static musl binary, so no dependencies), the man page, and the bash, fish and zsh completions at each family's standard path (zsh in `vendor-completions` on Debian, `site-functions` on Fedora). They are release files, not an apt or dnf repository, so they never update themselves, and they are unsigned. A new `package-linux` job in release.yml builds them with nfpm (pinned to 2.48.0 with its checksum checked) from `pkg/nfpm/`, and a new release needs it to pass. No distro ships a package with a `/usr/bin/proc` or named `proc-cli` (checked on Debian 12 and 13, Ubuntu 24.04 and Fedora 44); the packages install, run `proc list` and remove cleanly in those images on amd64 and arm64
- **Blog post: [what's new in proc 2](https://proc-cli.com/posts/whats-new-in-proc-2/).** The newest post covered 1.14.5, so anyone writing about proc from the site described 1.x behaviour 2.0 changed (a Zeniteq article on 2026-10-26 did). It covers exact name matching, `--json` no longer counting as a yes, separate-argument targets, `--start-time`, `reason` on every error, and Nix, the AUR and the Windows runtime fix, with real 2.1.1 output and what to check when upgrading from 1.x

### Changed

- **The install script runs under plain `sh`.** It needed bash, which FreeBSD and NetBSD don't install by default; `curl -fsSL …/install.sh | sh` now works everywhere, and `| bash` still does. It used to stop at "Unsupported operating system" on any BSD; now it installs on x86_64, and on an ARM CPU says that releases have no binary for it and to run `cargo install proc-cli`

### Fixed

- **The Nix flake builds on macOS with Nix's sandbox on.** The build runs the test suite, and in the macOS sandbox three tests couldn't listen on 127.0.0.1 and one couldn't read `/etc/passwd`, so `nix build` and `nix run github:yazeed/proc-cli` failed there (a nixpkgs reviewer's aarch64-darwin build showed it; CI's macOS Nix job ran without the sandbox, Nix's default on macOS). The flake allows local networking, the test asks the system's directory service as proc itself does, and CI builds macOS with `sandbox = relaxed`, as nixpkgs reviewers do
- **A signal never goes to a process whose start time can't be read.** Right before each signal (`kill`, `stop` and its later SIGKILL, `freeze`, `thaw`, `unstick`) proc reads the process on that PID again and compares its start time with the one it saw at the lookup. A start time it couldn't read counted as a match, so if the PID had passed to a process proc can't read (on macOS, another user's), the check passed. It now fails closed: the target is reported gone and gets no signal. With `--start-time` the guard already ruled such a process out at the lookup; this closes the same gap without it (found by Billy, an iLands agent, reviewing the design)

## [2.1.1] - 2026-10-30

### Fixed

- **A service account's processes show its name, not a number.** On Linux, `user` was the numeric user ID for every account whose shell is `/bin/false` (many distros give service accounts that shell) and for every user ID from 65536 up (directory and container accounts): `proc 2.1.0` showed `"user": "1000"` for a `sleep` run by such an account, where `ps` showed its name. proc read names from sysinfo's user list, which leaves those accounts out as "not real users"; it now asks the system's user database (`getpwuid_r`), as `ps` and `id` do, once per user ID per run. macOS has the same rule for its daemon accounts (shell `/usr/bin/false`), which proc mostly can't read the owner of without root anyway. Found when the Nix build ran the tests as the build user `nixbld1`
- **Two tests no longer fail at random, so a Nix build no longer can.** A Nix install builds proc and runs its test suite, and two tests depended on timing. The flag matrix's `for <listener> --status zombie` case expected no zombie, but it runs next to cases that kill their own copy of the same listener, which is a zombie until it is reaped: the case now looks only in its own directory. And on macOS, a test that listens on a port in the test process expected to be the port's only holder, but a child another test started at the same moment could inherit the socket (macOS has no `SOCK_CLOEXEC`): it now checks that the test process holds the port and stops holding it. Both failed once on the 2.1.0 release commit's CI run

## [2.1.0] - 2026-10-30

### Added

- **Nix is a checked install method.** `nix run github:yazeed/proc-cli` runs proc without installing it, and `nix profile install github:yazeed/proc-cli` installs it with its man page and its bash, zsh and fish completions. `flake.nix` had no lock file (each install used that day's nixpkgs), hard-coded the version, and no workflow built it; it now reads the version from `Cargo.toml`, ships a `flake.lock` pinned to nixpkgs-unstable, and declares Linux and macOS on Intel and Apple silicon (CI checks x86_64 Linux and Apple-silicon macOS). `nix flake check` builds proc from `Cargo.lock` and runs the whole test suite in Nix's build sandbox, and CI does that on Ubuntu and macOS on every push, then runs the built `proc --version`. The package is named `proc-cli` and the program `proc`. Two tests assumed the machine they ran on: one started `sleep` under another name, which fails where `sleep` is one name of a multi-call `coreutils` (Nix's), and the whole-machine `tree` test wanted two processes side by side, which a sandbox with one chain of processes does not have; both now hold anywhere. The nixpkgs package (`nix run nixpkgs#proc-cli`) is still to come
- **`--start-time` acts on a PID only while it belongs to the process you saw.** The OS gives a PID to a new process once the old one exits, and on a busy Mac every PID comes around again in well under an hour (2,700 new PIDs a minute measured on one developer machine, with PIDs up to 99,999). An agent that recorded a PID and acted on it later could end someone else's process: `--by` and `--in` only catch a new process with another name or directory, and comparing `start_time` by hand happens before the action, not at it. `proc stop 1234 --start-time 1700000000 --yes` now acts only if PID 1234's process started then (to the second); otherwise it exits 2 with `(ruled out by --start-time 1700000000)`. The value is the `start_time` in the process's JSON. It takes one PID target (anything else is exit 4), and a process whose start time can't be read (on macOS, another user's) is ruled out rather than trusted. On `kill`, `stop`, `freeze`, `thaw`, `wait`, `info` and `unstick`. The skill's rules 1 and 7 and its playbooks use it, and `evals.md` has a task for a reused PID (14). A guard error now names only the guards that ruled the process out, so `--by sleep --start-time 1` on a `sleep` names the start time alone. **Library API:** `guard_targets` takes a `Guards` (`in_dir`, `by_name`, `start_time`), and the seven commands' structs have a `start_time` field

### Changed

- **Every JSON error says why in `reason`.** `reason` came only with exit 4, so an agent could branch on why nothing was done but had to read prose for every other failure, and exit 2 alone meant three things: the target is gone, the name is only in command lines (`--cmd` would match), or a guard ruled the process out (Jeff de Vries, an iLands agent, and Wisp both asked for a field to branch on). Every error now carries one fixed word: `system_error` (exit 1); `not_found`, `command_line_only` or `ruled_out` (exit 2); `permission_denied` (exit 3); and, as before, `invalid_input`, `needs_confirmation` or `not_supported` (exit 4). A `ruled_out` error also has `ruled_out_by`, the flags of the guards that ruled the process out (`["--start-time"]`), and its text names them with their values on every command, `on` and `tree` included: `on` said "not in specified directory" even when `--by` was the guard that failed. A result that did not fully succeed (`failed` entries, `freed: false`, a `wait` that timed out) is not an error and has no `reason`. A port whose holders all ended between listing it and looking them up is now exit 2 (`not_found`), not exit 1. **Library API:** `ProcError::reason()` returns a reason for every error except `Reported`; new constants `REASON_SYSTEM_ERROR`, `REASON_NOT_FOUND`, `REASON_COMMAND_LINE_ONLY`, `REASON_RULED_OUT` and `REASON_PERMISSION_DENIED`; `ProcError::FilteredOut` has `ruled_out_by: Vec<(String, String)>` in place of `reason: String`, and `ProcError::ruled_out_by()` returns its flags; `proc_cli::core::ruled_out_by` names the guards that rule out a list of processes; `ProcessGone` exits 2
- **The website's changelog is written from CHANGELOG.md.** A release's notes were copied into `docs/changelog.md` by hand, with the website's headings. `scripts/changelog_site.py` now writes every release newer than the website's own history from CHANGELOG.md, and CI fails when the two differ, so a release's notes are written once
- **The agent skill reads a guard's exit 2 for what it is** (rule 7): a plain not-found means the PID is gone, while "ruled out by" means a process holds the PID and fails the guard, so the agent checks it with `proc info` before moving on. `evals.md` has a task for it (13)
- **The agent skill searches a name unscoped first.** Its "Stop something you did not start" playbook began with `proc kill <name> --in=<dir> --dry-run --json`, and in one of four eval runs an agent asked to "clean up node processes" scoped that first look to its own project directory and never showed the user the machine-wide matches. The playbook now searches without `--in`, tells the user how many processes match and what they are, and adds `--in=<dir>` only once the user names the directory
- **The agent skill no longer tells an agent to run a raw URL.** Its install line ended with `scoop bucket add proc https://github.com/...`, which Snyk's skills.sh audit flagged after 2.0.2 as an unverifiable runtime dependency (W012). The line keeps the registry installs (`brew`, `cargo`, `npm`, the last two also on Windows) and points to the README for Scoop and the rest
- **The README lists the AUR's `proc` package next to `proc-bin`.** Rafael Dominiquini, who maintains `proc-bin` (the release binaries), now also packages proc built from source as `proc` (`yay -S proc`)
- **The package listings name what proc is for.** crates.io and npm show https://proc-cli.com as the homepage (they showed the GitHub repo), and so do the Homebrew formula and the Scoop manifest. The crates.io keywords are now `kill, lsof, port, process, eaddrinuse` (`management`, `pid` and `cli` were generic or already in the name and description), with `development-tools` next to `command-line-utilities`. The npm keywords add the tools and errors people search for (`kill-port`, `lsof`, `taskkill`, `pkill`, `netstat`, `eaddrinuse`)

### Fixed

- **The man page's title line names only the version.** `proc manpage` put the whole `--version` text into the `.TH` title, so its URL and licence lines left the title's quote open, and `man proc` showed `https://github.com/yazeed/proc-cli License: MIT"` under the title. This is the page `proc manpage` prints and the Nix package installs (found while packaging proc for Nix)
- **Windows: proc.exe no longer needs the Visual C++ runtime.** The Windows binary from Scoop, npm and GitHub Releases imported `VCRUNTIME140.dll` and the `api-ms-win-crt-*` libraries, so on a machine without the Visual C++ Redistributable it could not start. It now links the C runtime in (`.cargo/config.toml`, as ripgrep and fd do), and CI and the release build fail if `proc.exe` imports it again. GitHub's Windows runners have the runtime installed, which is why no test run showed it
- **A guard that rules out the target says so.** `proc stop 1234 --by node --yes` on a live process that isn't `node` gave `No process found matching '1234'`, word for word what a PID nobody holds gives, in human output and JSON alike. An agent following the skill's PID-guard rule read it as "the PID was recycled, the process is gone" when its own `--by` was mistyped, and moved on with the process still running (found by Jeff de Vries, an iLands agent, testing the skill). The error now names the guard: `No process found matching '1234' (ruled out by --by node)`, or `--in <dir>`, or both. It is still exit 2. This covers `kill`, `stop`, `freeze`, `thaw`, `wait`, `info` and `unstick` with targets; `on` and `tree` said `(no matches with specified filters)`, and now name the guard too (see Changed). `info` used to print its normal document with nothing in it (`found_count: 0`, `not_found: []`); it now prints this error

## [2.0.2] - 2026-10-30

### Changed

- **A mistyped subcommand or value names the closest ones.** `proc kil` said only `unrecognized subcommand 'kil'`, and `--sort cp` only listed the allowed values: clap's own suggestion went out with its other tips. It now ends with `tip: did you mean 'kill'?` (in JSON, the `\n  Try: did you mean ...` line a mistyped flag already had), naming a subcommand by its name rather than an alias, closest first, three at most (`proc fre`: `'free' or 'freeze' or 'for'`). The human message also loses the blank lines the dropped tip left
- **The agent skill keeps a PID's start time and decides on numbers.** Rule 1: record a process's `start_time` with its PID, and check it is the same before acting on a PID recorded earlier, since the OS hands a dead process's PID to a new one. Rule 8: decide on the structured fields (`pid`, `parent_pid`, `start_time`, `uptime_secs`, ports, CPU and memory, exit code and `reason`); the text fields are only shown to the user. It also says that `on` answers in three shapes (`processes` for a port, `process` and `ports` for a PID, `results` for a name or several targets). Its frontmatter gains the optional `license`, `compatibility` and `metadata` fields of the Agent Skills spec, and its description and opening line use the words people search skills.sh with ("port in use", "kill a process", "free a port"), which found it before only by its name. Rule 1's example no longer has `$!` right before a backtick: skills.sh's scanner read that as a shell-command directive and labelled the skill "Contains Shell Commands" (Claude Code runs `` !`cmd` `` only after whitespace, so it never ran anything). The README's agent skill section shows the skills.sh install badge
- **Cargo.toml declares `rust-version = "1.88"`**, the version sysinfo 0.38 needs (checked with `cargo +1.88 check --all-targets`), so an older toolchain says so instead of failing inside a dependency. CI lints the tests and examples too (`cargo clippy --all-targets`), as CONTRIBUTING.md and RELEASE.md said it did. A release now starts with `rustup update stable`, the toolchain CI and the release builds use (a maintainer's 1.92 had been linting and testing for CI's 1.99), and checks the oldest supported Rust with `cargo +1.88 check --all-targets`. RELEASE.md says when `rust-version` is raised: only when a dependency or a feature we adopt needs it, to the lowest version that builds, never because a new stable came out
- **The README leads the way the website does**: one install line and the demo first, then what a command replaces, then the safety rules (it asks first, `--json` is not a yes, a name is the whole name), and the commands grouped by what they do. Six claims that were false or outdated for 2.0.1 are fixed: filters like `--min-cpu` do not combine with `kill` and the other changing commands, `ports --sort` takes port, pid or name, `list` and `watch` have no `not_found`, `free` stops with SIGTERM before SIGKILL, `wait` notices an exit within a quarter second, and the community `proc-bin` AUR package can lag behind releases

### Fixed

- **A closed pipe no longer crashes proc (exit 101), and a `kill --yes` piped to a closed reader reports its real result.** `proc list | head -n1`, `proc list -j | head -c 100` and `proc on 99999999 2>&1 >/dev/null | true` ended in a panic (`failed printing to stdout`, exit 101), because Rust's `println!` panics when a write fails and a reader that stops early makes the write fail. Worst, `proc kill <pid> --yes | true` killed the process and then exited 101, so a caller could not tell whether anything was touched. Now a reader that stops early ends proc quietly: proc stops writing and exits with the code the command's own result calls for (a `kill` that worked is 0, a not-found is 2), says nothing about the pipe, and a closed pipe on stderr changes nothing. Any other write error (a full disk, a file size limit) is a failure: exit 1 and one line on stderr (`proc: cannot write to stdout: ...`), while a command that already failed keeps its own exit code. `--help`, `-h` and `--version` into a closed pipe exit 0 quietly, like `ls --help | head -n1`; they used to exit 0 even when nothing could be written, and now exit 1 then. `completions` and `manpage` no longer panic either. Every write to stdout or stderr goes through one module, `proc_cli::ui::out`, and `clippy.toml` bans `print!`, `println!`, `eprint!`, `eprintln!`, `dbg!` and `std::io::stdout`/`stderr` everywhere else, so a new direct print fails `cargo clippy -- -D warnings`. **Library API:** `proc_cli::ui::out` is new, with the macros `out_print!`, `out_println!`, `err_print!` and `err_println!`
- **A value with a line break no longer cuts the JSON error short.** `proc kill $':99999\nUsage: x' -j` gave `invalid value ':99999` and nothing after it, because the message was cut at any line that read `Usage:`. clap's own blocks start after a blank line, and only those are taken off now
- **A usage error in JSON now carries clap's whole message.** `proc kill --by wispworker -j` said `the following required arguments were not provided:` and stopped, because only the first line of clap's message was kept and clap puts the missing `<TARGET>...` on the next one. The same cut the `[possible values: ...]` off `--sort bogus` and `--status bogus`. The `error` now joins clap's meaningful lines with a space (`the following required arguments were not provided: <TARGET>...`, `invalid value 'bogus' for '--sort <SORT>' [possible values: cpu, mem, pid, name]`), without the `Usage:` block, the `--help` pointer and the `tip:` lines, and keeps the `\n  Try: did you mean '--json'?` suggestion. Human output is clap's usual rendering. A new test builds a corpus of parser errors from the command definitions (a missing required argument, an unknown flag, a bad value, a value the argument's own parser refuses such as a target `:99999` or `ports --range bogus`, a missing value and too many values, for every subcommand and argument; an argument whose parser is its own and has no bad value in the corpus fails the test) and checks each one, with and without `--json`, for exit 4, `reason: invalid_input`, a message that does not end at a colon and names the argument, value or allowed values at fault
- **A missing target next to `--by` or `--in` says what the filter does.** `proc kill --by wispworker` only listed `<TARGET>...` as missing, and an agent read `--by` as the way to pick processes. The error now adds `kill needs at least one target; --by filters the targets, it doesn't select them` (`--in` likewise; the command is named by its canonical name, so `proc k --in` says `kill`), in human and JSON output
- **An argument that is not UTF-8 is exit 4 invalid input, not a panic (exit 101).** `proc kill $'\xff'`, `proc list $'\xff'`, `proc in $'/tmp/\xff'` and `proc list --in $'\xff'` panicked because the three places that read the raw command line used `std::env::args()`. They read `args_os()` now, and an argument with a byte that is not UTF-8 gives `argument '\u{fffd}' is not valid UTF-8; proc takes its arguments as UTF-8`, with the bad bytes shown as U+FFFD, `reason: invalid_input` in JSON and the same text on stderr without it
- **The message for a missing `--yes` no longer starts with `Invalid input:`.** The call is valid and only needs a yes, and `reason: needs_confirmation` already says so. It now reads `stop changes processes, so it needs --yes, with --json too (or --dry-run to preview)`; the other invalid-input messages keep their prefix. **Library API:** the `Display` of `ProcError::ConfirmationRequired` is now just its message, without `Invalid input: `; `reason()` and the exit code are unchanged
- **A signal no longer leaves the terminal in raw mode.** `kill -TERM` on a `proc watch`, closing the pane it ran in, or `Ctrl+C` or SIGTERM at a `[y/N]` prompt left the shell with echo and line editing off, the alternate screen on and the cursor hidden, until `stty sane`. SIGINT, SIGTERM, SIGHUP and SIGQUIT (on Windows Ctrl+C, Ctrl+Break and a closing console) now put the terminal back first, then end proc by that same signal, as a program without a handler would: a shell shows 130 for SIGINT and Ctrl+C, 143 for SIGTERM, 129 for SIGHUP, 131 for SIGQUIT, and Ctrl+C stops a script's loop (Windows has no signals, so there proc exits with 130). Nothing is printed, and a prompt that had not been answered changed nothing. The terminal is also restored when a write to it failed first. A program proc starts meanwhile (the `lsof` a `watch` refresh runs on macOS) starts with no signal blocked, so those signals still end it.
- **Windows: `proc for`, `proc on <name>` and `proc why --json` read the port table once.** They read it again for every process they showed, and each reading runs `netstat` twice and `tasklist` for every listening socket, so `proc for` on a program with a few copies running took over a minute in CI. A reading also names its sockets from one process list instead of a `tasklist` for each, which had `proc ports --in` past a minute on a busy machine
- **The npm launcher ends the way proc did.** A proc that a signal ended made `npx proc-cli` exit 1; it now ends by the same signal, so a shell sees 130 or 143 as it would without npm.
- **A prompt always shows what you would approve.** The list of processes went to stdout and the `[y/N]` question to stderr, so with stdout in a file or a pipe whose reader had left (`proc kill node | head -n1`, `proc kill node > log`) you were asked about processes you could not see, and a `y` was a blind yes. `kill`, `stop`, `freeze`, `thaw`, `free`, `unstick`, `stuck --kill` and `orphans --kill` now ask through one function: the list goes to stdout when that is a terminal and to stderr, beside the question, when it is not. If the list cannot be written, nothing is asked and nothing is changed: exit 4, `needs_confirmation`. `clippy.toml` bans dialoguer's `interact` methods outside that function.
- **`proc watch --json` stops at once when its reader leaves.** `proc watch node -j -n 20 | head -n1` kept running until the next refresh (6 s later in the test, an hour with `-n 3600`) because a closed pipe is only noticed by a write. On Linux and macOS the wait between refreshes now watches the pipe and ends the moment the reader closes it; Windows notices at the next refresh. `proc wait` is the other way on purpose and is now written down: its exit code (0 exited, 1 timed out) is its result, so it keeps waiting after its reader leaves and exits with that code.
- **`proc watch` without a terminal on stdin shows one snapshot instead of failing.** `proc watch node < /dev/null` on a terminal drew a frame, then stopped with `System error: Failed to initialize input reader` (exit 1). The screen needs a key to quit with, so without a terminal on both ends watch prints one snapshot like `list`, and says so on stderr when stdout is a terminal.
- **A failed write to the watch screen follows the same rules as every other write.** It ended in `Broken pipe (os error 32)` and exit 1, and the escape codes that put the terminal back were refused once any earlier write had failed. A reader that left now ends watch quietly, any other failure is the usual `cannot write to stdout` and exit 1, and the terminal is put back either way.
- **Failures that used to look like answers now say what went wrong.** A script that relied on the old results will see an error where it saw success:
  - `proc tree -a` (`--ancestors`) with no target printed nothing and exited 0; it is exit 4 (`invalid_input`): `--ancestors needs a target`
  - `proc tree <name>` for a name that matches no process printed an empty tree and exited 0; it is exit 2, like `info` and a PID that doesn't exist
  - `proc tree <pid> --by <name>` or `--in <dir>` that rules the target out printed an empty tree and exited 0; it is exit 2 (`no matches with specified filters`), the PID guard `info` and `on` already gave. `tree -a` ignored `--in` and `--by` altogether, and now keeps to them the same way
  - An empty target (`proc watch ""`, `proc kill ","`) watched or matched every process; it is exit 4: `a target can't be empty`. Any other empty argument is exit 4 too: `in ""`, `for ""` and `--in ""` (an unset `"$DIR"`) meant the current directory, `list ""` listed nothing and `by ""` was not found. `--in=` names its flag: `--in can't be empty`
  - `--in <dir>` for a directory that does not exist, with nothing running there, was accepted by most commands as a filter that matches nothing (or, on `watch`, as an empty view); it is exit 4 (`Directory not found`) on every command with `--in`, as `proc in` already did, and `Not a directory` for a file. A deleted directory that processes still run in is kept, so its leftovers can be found, also when it is typed through a symlink (`/tmp/x`, which macOS reports as `/private/tmp/x`) and, for `ports`, when the process there listens on nothing. `on` said its targets were not in the directory (exit 2), exactly what it says for a directory that exists, so a typo looked like an answer; it is exit 4 too. **Library API:** `OnCommand` and `WatchCommand` keep the resolved `--in` in a private field (it was turned back into text and resolved again for every process, and an error there dropped the filter), so outside the crate they are built by parsing arguments (`Cli::try_parse_from`), not with a struct literal
  - A relative path when the current directory has been deleted said `No process found matching 'unknown'` (`proc for ./x`); it is exit 4 and says the current directory no longer exists
- **`--in` compares the real directory, not the text shown for it.** A directory name with a control character is shown escaped (a newline as `\n`), and `--in` compared that text, so such a directory could not be matched, and one named `a\nb` (a backslash and an `n`) matched both. `on`, `for`, `list`, `by`, `watch`, `ports` and the commands that change processes now all match the real path through one function
- **`proc tree --in <dir>` with no target shows the processes in that directory**, each with its subtree, as `--min-cpu` does. It ignored `--in` and `--by` without a target and printed every process. A match inside another match's tree (a shell and the server it started share a directory) is shown there once, not again as a root of its own; `--min-cpu` and the other filters showed it twice too

## [2.0.1] - 2026-10-28

### Added

- **Every exit-4 JSON error says why, in `reason`.** Exit 4 means nothing was done, for one of three causes, and only a missing `--yes` was machine-readable (`needs_confirmation: true`). A wrong flag such as `proc kill --name wispworker -j` came back as exit 4 with only the text `unexpected argument '--name' found`, so an agent had to read prose to know it should fix the command. The error now has `"reason"`: `"invalid_input"` for a wrong flag, value or argument (clap's own usage errors included), `"needs_confirmation"` for a missing `--yes` (which keeps `needs_confirmation: true` for agents written against 2.0.0), and `"not_supported"` for `freeze`, `thaw`, `orphans` and `stop --signal` on Windows. Exits 1 to 3 have no `reason`. Human output is unchanged. The skill's exit-code table and `reference.md` say to branch on `reason`. **Library API:** `ProcError::reason()` returns it (`None` unless the error is exit 4), and `proc_cli::error` has the strings as `REASON_INVALID_INPUT`, `REASON_NEEDS_CONFIRMATION` and `REASON_NOT_SUPPORTED`

### Changed

- **`proc --help` ends with short labelled sections instead of one dense paragraph, and its examples are styled the same way** (an `Examples:` header, commands highlighted, meanings aligned). `Targets:` (`:3000`, `1234`, `node`, several as separate arguments), `Names:` (what a name matches, `--cmd`, a path is not a name, wildcards), `Notes:` (the deprecated comma list, `--in`) and a `More:` link, with the meanings aligned like the `Options:` list. Headers and literals use the help's own styles (`proc_cli::cli::STYLES`, clap's default look), so they follow clap's colour handling: no colour codes when piped or with `NO_COLOR`, and none in `proc manpage` or the completions. `proc -h` gets the `Targets:` section and the link. The facts are the same. **Library API:** `proc_cli::cli::STYLES` is new
- **The agent skill treats process fields as data, not instructions** (rule 8). A process's name, command line, working directory, executable path and user are written by whoever started it, so an agent never acts on text found in them and tells the user if one reads like an instruction
- **The skill and its JSON reference say how to tell exit-4 cases apart:** only the confirmation error has a `needs_confirmation` key (always `true`), and a successful action's `succeeded` and `failed` lists are the receipt for the preview's `processes`
- **The README points Arch Linux users to `proc-bin`**, the community AUR package of the release binaries, until proc's own AUR package is published
- **`--sort` help says which way each key runs** on `list`, `by`, `in`, `for`, `watch` and `ports`: cpu and mem put the biggest first; pid, name and port go ascending. Ties come by PID (by port, then PID, for `ports`)

### Fixed

- **Windows: `stop --signal` is a "not supported" error**, as the docs already said: `reason` is `not_supported` and the message starts "Not supported on this platform". It was reported as invalid input
- **The order of what proc prints is the same on every run, and `--limit` picks the same rows.** `proc by zsh --json` listed its PIDs in a new order each time, and `proc list --limit 8` returned different processes each time. The process list came out of a randomly ordered map, and `--sort cpu` (the default) ties for nearly every process, so the ties kept the random order. Now every process source lists by PID ascending and a sort breaks ties by PID, so a name, `--limit`, `orphans`, `stuck` and every list a command asks you to approve read the same twice
- **`tree` and `tree --json` give the same tree.** The human tree sorted each parent's children by PID, but the JSON children, the roots, the filtered list, the target list and `--ancestors` for a name came out in a new order every run. All of them are now in PID order, in both forms
- **A process's ports are ascending** in `on <pid>`, `for` and `why`, and `ports --sort pid` and `--sort name` break ties by port, then PID. They followed the order lsof (macOS), the kernel's socket table (Linux) or netstat (Windows) listed them in. The holders of a port are by PID, and `on :port --json` has `process`, `protocol` and `address` of the holder with the lowest PID on every OS (Linux and Windows used to follow the kernel's or netstat's listing order; macOS already did)
- **The list a command asks you to approve is in a stated order**: the targets in the order typed, then PID ascending within each target. `kill`, `stop`, `freeze`, `thaw`, `free`, `unstick`, `orphans --kill` and `stuck --kill` use it for the preview, the prompt, `--dry-run --json`, the `needs_confirmation` list and the receipt. `stop` reports `failed` in that order too, instead of the processes that refused the signal followed by the ones it could not force-kill

## [2.0.0] - 2026-10-28

This is a major release. Read **Changed** first: a name now matches the whole process name (add `--cmd` for the old matching), and `--json` no longer counts as `--yes`. The library API changed too (see the entries marked **Library API**).

### Added

- **Several targets can be separate arguments.** `proc kill 1234 5678 :8080` is the form `kill`, `docker kill` and `killport` use, and it now works everywhere proc took a comma-separated list: `kill`, `stop`, `freeze`, `thaw`, `free`, `on`, `why`, `wait`, `unstick`, `watch` and the names of `list` (`info` already did). Separate arguments and comma lists mix (`proc kill 123 :3000,:8080 node`), and a process named twice is still handled once, except by the read-only `on` and `why`, which answer each target under its own header. A message that repeated the target now shows the targets as typed, joined by spaces (`No process found matching '1234 5678'`; `wait --json` has them in `target` the same way), and the JSON `not_found` lists still have one entry per target. `by`, `tree`, `in` and `for` take a single value, as before. Because `--in` may take a value, it takes the next word as its directory unless that word starts with `-`: `proc kill --in 123 456` reads `123` as the directory, so put `--in` after the targets or write `--in=DIR`. Every `--in` says so in `--help`

- **`--cmd` matches names the old way.** `proc kill serve --cmd` matches any process with `serve` in its name or full command line, like `pkill -f`. It is on every command that takes a name target or `--by`: `on`, `by`, `in`, `for`, `list`, `info`, `ports`, `tree`, `watch`, `why`, `kill`, `stop`, `freeze`, `thaw`, `unstick`, `wait`, `orphans` and `stuck`. It has no short form, so a wider match is always spelled out. When a name matches nothing but some command lines have it, the not-found message now says so, for example `2 processes have 'serve' in their command line (add --cmd to match those)`. The hint costs nothing: the same pass over the processes counts them. `tree` shows it too; `list` (it prints an empty list) and the `--by` filters that only narrow a result (`kill --by`, `ports --by` and the like) do not

- **`--wide` / `-w` on `list`, `by`, `in`, `for`, `stuck`, `orphans` and `watch`**: prints directories and arguments in full instead of fitting them to the terminal, for when you need the whole path and your terminal can scroll
- **[FAQ page](https://proc-cli.com/faq)**: 16 answers to the most searched process and port questions ("address already in use", killing a process on a port on Mac and Windows, zombies, orphans, pausing a process), each with the proc command and the classic alternative
- **Blog post**: [What's new in proc](https://proc-cli.com/posts/whats-new-in-proc), covering the releases since March
- **Structured data**: SoftwareApplication on the home page, FAQPage on the FAQ, BreadcrumbList on posts and command pages, and an author (`yazeed`, linked to GitHub) on posts, shown with an avatar next to the date. Every page also has a social card image
- **Favicon**: `proc-cli.com/favicon.ico` was a 404. The site now has an icon, the pink chevron and a blue cursor on the terminal's dark square (`favicon.svg`, `favicon.ico` at 16, 32 and 48 px, and a 180 px `apple-touch-icon.png`), linked from every page

### Changed

- **`--version` says when a build isn't a release.** `main` keeps the last released version number until the release, so a build from `main` printed `proc 1.14.5`, the same as the real 1.14.5 release, and a bug report citing it was ambiguous. A build from the release tag still prints the plain version (`proc 2.0.0`); any other build in a git checkout prints `proc 2.0.0-dev (main@5e94647, 3 commits after v2.0.0)`, naming the branch (`detached@` when HEAD is detached), the commit and the distance from the latest `v*` tag, with `, modified` added when tracked files have uncommitted changes. Builds without git (crates.io, `cargo install proc-cli`, a Docker context without `.git`) print the plain version. `proc_cli::VERSION` carries the same string. The Dockerfiles now copy `build.rs`, which gathers the git facts
- **BREAKING: a name matches the whole process name, not any part of the name or command line.** `proc kill node` used to kill every process whose name or full command line contained `node`, ignoring case. On one Mac a dry run matched 193 processes, among them `Claude Helper` and `Obsidian Helper (Renderer)`, and `proc kill go` matched 55, including Chrome renderers and another Claude Code session whose command line said `Google`; with `--yes` all of them would have been killed. A name now equals the process name, the file name of its executable, or the file name of its first argument, ignoring case, as `killall` and `pkill` read it (`proc kill node` matches `node` only). The executable and first argument count because Linux cuts a process name to 15 characters, so a longer name only matches through them. On Windows a trailing `.exe` is optional on either side, so `node` matches `node.exe`. The same rule applies to every name target (`on`, `by`, `list`, `info`, `tree`, `watch`, `why`, `kill`, `stop`, `freeze`, `thaw`, `unstick`, `wait`) and to every `--by` filter (also `in`, `for`, `ports`, `orphans`, `stuck`), which used to match a part of the name or the command line, or on `ports` a part of the name. proc itself and its ancestor shells are still left out. A path is not a name: `proc kill ./server` and `proc kill /usr/bin/node` find nothing, because the pattern is compared with file names; use `--cmd`, or `proc for <file>` for an executable. An empty name, which used to match every process, now matches nothing (with `--cmd` it still matches everything, as `pkill -f ''` does), and a name that matches nothing no longer reports how many command lines contain the empty string. **Migration:** add `--cmd` to keep the old matching (`proc kill node --cmd`), or name the process exactly. A script that passed a part of a name now gets exit 2 (`No process found matching`) where it matched before; one that relied on a command-line match needs `--cmd`
- **On macOS the JSON `address` of an IPv6 listener loses its brackets**: `[::1]` is now `::1`, matching Linux and Windows. A script that matched the bracketed form must drop the brackets
- **CI fails when a line or a flag has no test.** The coverage gate is no longer report-only: a change that drops line coverage below 100% of `src/`, merged across Linux, macOS and Windows, fails the build, and so does a long flag (`--name`) or short flag (`-n`) of any process command that no test exercises (`completions` and `manpage`, `--help`/`-h` and `--version`/`-V` are not checked). The gate also fails if one OS's coverage run is missing, or a source file has no coverage data, so a broken job can't pass as a smaller report. `scripts/coverage-local.sh` shows the Linux numbers locally, and CONTRIBUTING.md describes the rule for excluding a line. The gate's report also prints each OS's own total, right after the merged TOTAL and labelled by its lcov file (when several are given; the percentage is cut, not rounded), so a platform that runs far less than the others is visible; it is for information only, and the merged 100% stays the only rule
- **Every CI job has a time limit** (5 to 45 minutes, at least twice a normal run), so a hung test fails the job instead of holding a runner for up to six hours
- **"Not supported on this OS" is exit 4.** `freeze` and `thaw` on Windows exited 1, while `stop --signal` on Windows exited 4. Both mean the same thing, so they now share one code: `freeze`, `thaw` and `stop --signal` exit 4 on Windows, and the skill's exit-code table says so
- **`orphans` is not supported on Windows: it exits 4.** On Windows `proc orphans` (also `--kill` and `--dry-run`) always reported "No orphaned processes found", because an orphan is a process whose parent is PID 1, and Windows has no PID 1 and does not re-parent a process whose parent exited. It now exits 4 with "orphans is not supported on Windows: Windows doesn't re-parent processes, so there is no reliable way to tell an orphan", like `freeze` and `thaw`. A script that ran `orphans` on Windows and treated the empty answer as "nothing to clean up" now sees the error. Linux and macOS are unchanged
- **`--json` no longer counts as a yes.** Commands that change processes (`kill`, `stop`, `free`, `freeze`, `thaw`, `unstick`, `orphans --kill`, `stuck --kill`) acted without asking when given `--json`, because the prompt was skipped for JSON output. `--json` only changes how results are printed, so they now need `--yes` like everywhere else: without it they exit 4 and change nothing. The same goes for running without a terminal, which used to fail with "Dialog error: not a terminal". `--dry-run` still previews without `--yes`. With `--json` the error also carries `needs_confirmation: true` and the `processes` the command would have changed, so a caller can show the list before retrying with `--yes`. A test now checks that every command takes `--json`/`-j` and `--verbose`/`-v`, and every command that changes processes `--yes`/`-y` and `--dry-run`
- **Library API: name matching.** `Process::find_by_name`, `resolve_target`, `resolve_targets`, `resolve_targets_excluding_self`, `matches_by_filter` and `apply_filters` take a `proc_cli::core::NameMatch` (`Exact` or `Command`) as a last argument. `resolve_targets` and `resolve_targets_excluding_self` return a `Resolved { processes, not_found, hints }` instead of a tuple. `Process` has two new fields, `argv0` and `raw` (neither serialized), so a `Process { .. }` literal needs them; `raw` is `None` unless a name, executable or command line had control characters escaped, and holds the unescaped text name matching reads. `ProcError` has a new variant, `NameNotFound { name, in_command_line }` (exit 2), returned when a name matched nothing but is in some command lines; an exhaustive `match` must handle it. The matcher itself is `proc_cli::core::name_match`
- **Action JSON lists the targets that weren't found.** `proc kill <pid> 99999999 --dry-run --json` returned `success: true` with no sign that a target was missing, so an agent could not tell. Every command that takes targets now puts `not_found` in its JSON, an array that is empty when every target was found: `kill`, `stop`, `freeze`, `thaw`, `unstick`, `free` and `wait` in the dry run, the result and the `needs_confirmation` error (exit 4), and `why` in its result (`info` and `on` already had it). `free` lists the ports as typed (`":3000"`). The found processes are still handled, so `success` and the exit codes are unchanged, and the human output still warns `Not found: …` (`unstick` did not before). When every target is missing the command is still the not-found error (exit 2), which names them in `error`, except `free`: a port nobody holds is success (exit 0) with the ports in `not_found`. `orphans` and `stuck` take no targets and have no `not_found`
- **Help and docs examples never pair `--yes` with a name.** The `kill`, `freeze` and `thaw` command headers and the FAQ showed a name followed by `--yes`, and it was copied by agents with no skill loaded, and it kills everything the name matches without a look. `--yes` now appears only with a `:port` or a PID in `--help`, the README and the command pages; a name is shown with `--dry-run`, or without `--yes` so it asks first. A test reads those files and fails on a name next to `--yes`. The wildcard note changed too: proc has no wildcards, an unquoted `node*` is expanded by your shell into file names, and `--cmd` is the way to a partial match
- **Library API: `PortInfo::find_by_port` is now `PortInfo::holders_of`.** It returns every process listening on the port (a `Vec`, empty when nothing does), each once, instead of the first one found. A caller that wanted one process takes the first element
- **Library API: confirmation.** `proc_cli::ui::must_confirm(action, yes, json, processes, not_found)` is new and takes the processes the command would change and, for a command with targets, the ones that matched nothing (its first form, written earlier in this unreleased work, took three arguments; no release has either). `Printer::with_not_found` makes a printer put `not_found` in the JSON it writes. `Printer::ask_confirm` no longer treats `--json` as a yes: it returns an error where it used to return `Ok(true)`, so a library caller that passed a JSON printer without `yes` now gets `ProcError::ConfirmationRequired`
- **Library API: `ProcError` has a new variant, `ConfirmationRequired { message, processes, not_found }`.** It is what a missing `--yes` returns (exit 4). `ProcError` is not `#[non_exhaustive]`, so an exhaustive `match` on it in library code must handle the new variant
- **Library API: `Process::find_orphans` returns `ProcError::NotSupported` on Windows.** It returned an empty list there, which read as "no orphans"; Windows has no PID 1 and does not re-parent a process whose parent exited, so nothing can be checked. The signature is unchanged (it returns a `Result`); a caller that ran it on Windows now gets the error (exit 4 in the `orphans` command)
- **Library API: `ProcError::FilteredOut { target, reason }`, and `NotSupported` exits 4.** `FilteredOut` (exit 2) is what `proc on` returns when `--in` or `--by` ruled out everything a target matched; it replaces a `ProcessNotFound` whose text had the reason inside the target, so an exhaustive `match` must handle it. `ProcError::NotSupported` now maps to exit 4 (it was 1). `ProcError::command_line_hint` and `proc_cli::error::command_line_hint` build the `--cmd` hint of `NameNotFound`
- **Library API: other changed and new public items.** The command structs (`KillCommand` and the rest) changed their fields, so a struct literal needs updating: `target` is a `Vec<String>` on `kill`, `freeze`, `thaw`, `free`, `on` and `why`, `watch`'s `target` was an `Option<String>` and is a `Vec<String>`, `list`'s `name` was an `Option<String>` and is a `Vec<String>`, `ports`' `range` is an `Option<(u16, u16)>` (parsed, not text), and `cmd` (every command that takes a name target or `--by`) and `wide` (`list`, `by`, `in`, `for`, `stuck`, `orphans`, `watch`) are new. `Process::kill_and_wait` returns `Result<()>` instead of `Result<Option<ExitStatus>>`. New: `Process::names`, `Process::looks_stuck_now`, `Process::cpu_over` (Unix only), `Process::measure_cpu`, `proc_cli::core::{escape_control, RawText, apply_to_each, flatten_targets, validate_target_arg}`, `proc_cli::core::filters::{cwd_within, resolve_dir, strip_verbatim_prefix}`, `PortInfo::is_exposed` and `PortInfo::is_local`, `proc_cli::ui::{process_table_budget, ProcessTableBudget}`, `Printer::wide`, and the modules `proc_cli::cli` (`Cli` and `Commands`, moved out of the binary) and `proc_cli::version`. `proc_cli::VERSION` is built by `build.rs` and can carry a `-dev` suffix (see above). `truncate_string` and `truncate_path` count display columns, so their length argument is a width
- **The agent skill asks before touching what it didn't start.** Its rule was "only touch processes you started or the user named", while the skill page promised the agent "checks with you" first. The rule now says to ask: preview with `--dry-run --json`, show the user the list, and add `--yes` only once they agree, since `--yes` is consent given on their behalf. The page describes what the skill tells agents and what proc enforces itself
- **The agent skill is rewritten for 2.0.0.** `SKILL.md` now opens with a safety model instead of a list of commands: a table of what only reads, what can be undone and what ends a process; seven ranked rules (a process is yours only if you started it and recorded its PID, a name is a search and never approval, preview with `--dry-run --json` and show the user, `--yes` is the user's consent to one list and never a retry, act by PID with an `--in`/`--by` guard, then verify); and what to do on exit 4 `needs_confirmation`. The old rule "always pass `--yes`" is gone, and no template pairs `--yes` with a name. Targets are `:port` and PID first; names are exact whole names for a search, `--cmd` only after a dry run, several targets are separate arguments, and `stuck --kill` and `orphans --kill` are never used. A permission failure is documented as it happens (a `failed` entry and exit 1, not exit 3), as are ports with several holders, `not_found`, and the false positives of `stuck` and `orphans`, with measured counts. The description now says when not to use proc (containers, system services, remote hosts). `reference.md` has a contents list and a section on older versions, where `--json` counted as a yes; the version pin is replaced by "proc 2.0 or later". New `skills/proc-cli/evals.md` lists twelve tasks with what the agent must and must never do, and `tests/doc_examples.rs` now also reads `skills/proc-cli/*.md`: it fails on a name next to `--yes`, on `--yes` with `orphans` or `stuck`, and on a comma list of targets outside a line that says commas are deprecated. `<pid>` and `:<port>` placeholders are accepted next to `--yes`; `<name>` is not
- **A bare `proc` orients you instead of listing all 19 commands.** It now leads with the one idea that unlocks the rest — a target is the same everywhere — and shows four commands that run exactly as printed, with `proc --help` still listing everything. The full list told a newcomer what exists without telling them how the tool thinks, and the measure that matters for a CLI is how long it takes someone who has never run it to get a correct answer
- **One process is always one line.** The process table truncates to the terminal width instead of letting cells wrap: on a machine with ten sibling repos, fourteen processes used to render as roughly seventy interleaved fragments with the columns no longer lining up, which stopped it being a table at all. Long directories keep their root and their tail (`/Users/…/my-app/.next/standalone`), because the tail is the part that differs between rows, and `proc watch` now does the same: its table was a separate copy of the old layout, so it kept wrapping after the other one was fixed. Both tables now size their columns from one shared budget, so they cannot drift apart again
- **Website redesign ([proc-cli.com](https://proc-cli.com)).** A new layout and type built for reading: a 42rem text column, Atkinson Hyperlegible Next and JetBrains Mono, light and dark themes. Code blocks are dark terminals with copy buttons and spacing above and below, so the text after a block no longer sits against it. The home page leads with an interactive command reader: pick any of the 19 commands and one of its targets, and it says what that exact command does and how proc reads the target (58 examples, in `docs/_data/reader.yml`). "Target" in the headline uses the same colour as the target in the reader. Command pages have a sidebar listing every command, and the changelog has one listing every version with its release date, grouped by minor version and marking the one you're reading. The header and the install page show the latest version, read from `docs/_data/release.yml`, which RELEASE.md now bumps with the others
- **Sentence case across the site**: page titles, headings, tables and post titles
- **`proc wait` notices an exit within a quarter second.** It used to check only every `--interval` (5 seconds by default), so it could return 5 seconds late. `--interval` now sets how often the status line is printed
- **`proc stop` asks every process at once and waits for them together**, so several processes share one `--timeout`. They used to be handled one after another: five processes that ignore SIGTERM took 50 seconds instead of 10
- **`proc ports` and `proc on :port` show full process names on macOS** (`python3.12`, `ControlCenter`, `OpenCode Helper`). lsof cut them to 9 characters, so a helper process could look like the app itself
- **`proc stuck` needs a process to stay stuck.** A process counts as stuck when it has been running longer than `--timeout` and, in two readings a second apart, is using over 50% CPU or is blocked in uninterruptible sleep (waiting in the kernel, usually for I/O). A single half-second reading used to be enough, so any long-running process that was busy for a moment counted, and `stuck --kill` could catch it; a hang that uses no CPU wasn't found at all. `stuck` now takes about 2 seconds. A long build at full CPU still looks the same as a loop that never ends, so the docs say to check the list before `--kill`
- **Port lookups on Linux need nothing installed.** proc reads the kernel's socket tables in `/proc` itself instead of running `ss`, so `on :port`, `ports`, `free` and `wait :port` work on minimal systems without iproute2 (Alpine, debian-slim, distroless). The Docker image no longer installs iproute2. The same port listening on IPv4 and IPv6 is listed once, as on macOS

### Deprecated

- **Comma-separated targets** (`proc kill :3000,:8080`). They still work throughout 2.x, and a command that uses one now prints a single `note:` on stderr naming the fix, built from your own targets: `note: comma-separated targets are deprecated and will be removed in a future major release; pass them as separate arguments: proc kill :3000 :8080`. It appears in human output only, so `--json` output is unchanged on stdout and stderr; it is printed once however many commas there are, and only for the targets of `on`, `info`, `list`, `kill`, `stop`, `watch`, `unstick`, `freeze`, `thaw`, `why`, `free` and `wait` (a comma in a `--by` or `--in` value, or in the one value of `by`, `in`, `for` or `tree`, is just text). It is coloured on a terminal unless `NO_COLOR` is set. Separate arguments (`proc kill :3000 :8080`) are the recommended form and commas will be removed in a future major release

### Removed

- **Printing helpers nothing called.** `Printer::error`, `Printer::print_ports`, `Printer::print_port_info` and `Printer::print_kill_result` (in `proc_cli::ui::output`) are gone, along with the output types behind the two port printers. proc itself never used them: `ports` and `on` print through their own code, and `kill` reports through `print_action_result`
- **Two library functions nothing called.** `proc_cli::core::resolve_target_single` and `Process::wait` are gone. proc itself never used them: every command resolves targets with `resolve_targets` (or `resolve_target`), and `proc wait` polls for the exit instead of calling `Process::wait`
- **Unused dependencies `tokio`, `anyhow` and `indicatif`.** Nothing in proc used them; builds are smaller and faster (20 fewer crates to compile)

### Fixed

- **`proc free` called a port free when a copy of the holder took it back a moment later.** It looked once after stopping the holders, so a supervisor or forked worker that rebinds the port within milliseconds could be missed between the old holder's exit and its own bind, and `free` reported the port freed (exit 0) while it was in use again. A port now counts as free only when it reads free in two looks a quarter of a second apart; one that is taken again is reported `still in use` (exit 1, `"freed": false`) as before. A free port costs a quarter of a second more
- **Name matching read escaped text in some places and the raw text in others.** A control character in a process's name, executable or command line is shown escaped (`\x07`), but `Process::from_sysinfo` stored the escaped first argument while the lookup compared the raw one, so a process `find_by_name` found could be dropped by `--by` (`proc in . --by NAME`, `kill --by`), and the other way round with `--cmd`. Matching now always reads the text as the OS gave it, in `find_by_name`, `watch` and every `--by` filter, and display keeps the escapes. Only a name with a control character in it was affected
- **`proc watch <name>` listed the shell that launched it.** Its name lookup left out proc itself but not its ancestors, so `sh -c 'proc watch node --cmd'` showed that shell, and a shell started under the name showed even without `--cmd`. `watch` now skips the ancestors as `by`, `kill` and `tree` do
- **A port held by several processes was handled as if one held it.** One port number can be open in two processes, one on `127.0.0.1` and one on `[::1]`, which are different sockets. `proc ports` listed both, but every lookup by `:port` took the first one it found: `proc free :3000 --yes` killed one process, the port stayed busy, and it exited 1 with `freed: false` and nothing left for the next call to find; `kill :3000` and `stop :3000` left the other holder running; `on :3000`, `info`, `wait`, `tree`, `watch` and `why` showed one process. A `:port` now means every process listening on it, in all of them, including a master and the workers it forked that share one listen socket (nginx, Apache prefork, gunicorn): Linux used to credit the socket to one of them, so `free :80` stopped one and the workers kept the port; `ports` on Linux now lists every such process, as it already did on macOS. `free` kills them all and reports the port freed only when none is left, and `why :3000` traces each holder. `on :3000 --json` keeps `process` as the first holder, so existing scripts read what they did, and adds `processes` with all of them; the human output lists each holder under "Port 3000 is used by 2 processes:"
- **Windows never listed IPv6 listeners, and macOS reported them differently.** On Windows proc ran `netstat -p TCP`, which shows IPv4 sockets only, so ports open on `[::]` or `[::1]` were missing from `ports`, `on :port`, `free` and `wait :port`, and from `--exposed` and `--local`; proc now also runs `netstat -p TCPv6` (if that call fails, the IPv4 listeners are still listed); macOS kept the brackets in the address (`[::1]`). Windows and macOS now read it with one function that splits at the last colon and drops the brackets, so every OS reports `::1` and `::`, as the `address` field is documented. `*` is still `0.0.0.0` and scope suffixes (`fe80::1%eth0`) stay. Linux already read the kernel tables and was correct. The bracket special case in `--local` is gone (thanks @gtwatts, #1)
- **`proc kill --graceful` reported success on Windows when nothing happened.** A graceful kill there runs `taskkill` without `/F`, which only asks a program to close its windows, so a console program (a server, a script, anything without a window) is left running and taskkill fails; proc ignored taskkill's answer and reported the process as killed. It now reports the failure with taskkill's own reason, on one line. `proc stop` still stops such a process: when the polite request can't be delivered, it waits out `--timeout` and force-kills, as before. `proc free` does the same without a warning: on Windows it used to print `Failed to send SIGTERM to node.exe` for that refused request, then force-kill the process and report the port as freed, and the warning (now worded without SIGTERM, which Windows doesn't have) only appears for a request that fails on another OS or for a process that is gone
- **`proc unstick --force` on Windows reports a process that is already gone as terminated.** If the process exited between the polite request and the kill, `kill` failed and the process was reported as `failed` (exit 1) although it was gone. It is now reported as `terminated`, the same as on Linux and macOS
- **`proc stuck` never found a stuck process on Windows, and targeted `unstick` never saw one as stuck.** Both read CPU through sysinfo's own figure over two reads, and on Windows sysinfo stores no baseline in a process's first update, so the next read divides the process's lifetime CPU by all the CPU time since boot: a process spinning a full core read about 0%. `stuck` and the stuck check of a targeted `unstick` now measure CPU the way `list` and `info` already did on every OS: the CPU time each process used between two reads over the wall time between them. `stuck` now also reports that measured figure in its output. What `unstick` does to a stuck process on Windows is unchanged: it has no gentle step there, so without `--force` it reports `still_stuck`, and with `--force` it asks the process to close and then force-kills it
- **`proc tree <name>` listed the shell that launched it.** Name matching is meant to skip proc's own ancestors, whose command lines hold proc's arguments (`sh -c 'proc tree node'`), but the walk up started from proc's own entry in the process list, and that list leaves proc out, so it stopped at once and only proc itself was skipped. The shell, and the terminal or IDE wrapper above it, matched and appeared as results. The walk now starts from proc's parent, so name matching in `tree` skips any ancestor of proc whose command line matches, for example `proc tree zsh` run from zsh. `tree --ancestors <name>` had the same problem
- **`--in` found nothing when the path was a symlink or not in the OS's own form.** The directory was only made absolute, then compared with each process's cwd, which the OS reports with symlinks resolved. On macOS `proc in /tmp/x`, `--in /var/folders/...` and anything under `$TMPDIR` matched no process, because processes report `/private/tmp/...` and `/private/var/...`; a symlinked project directory failed the same way; on Windows a process reports its cwd in the form it was started with, so a directory typed as `C:\Users\RUNNER~1\...` (8.3 short name) or `C:\Users\runneradmin\...` (long) found only the processes started from the same form. An existing directory is now canonicalized first (and on Windows the `\\?\` prefix removed), and on Windows a process's cwd is compared both as reported and canonicalized, so the short and the long form both find it, for `list`, `by`, `in`, `tree`, `on`, `ports`, `for`, `watch` and every command that takes `--in`, and `list --path` resolves the same way. As a result `in` and `list --in` now print the canonical directory in "Found N process in …" and in the JSON context (`/private/tmp/x` for `/tmp/x` on macOS). A path that does not exist is still only made absolute, so processes that outlived their directory can still be found
- **`proc stuck --timeout N` needed a process to be N+1 seconds old.** A process's age is read in whole seconds and was compared with `>`, so `--timeout 0` skipped a process that read 0 seconds and `--timeout 5` one that read 5, though both had run "longer than --timeout" by the command's own definition. The comparison is now `>=`: `--timeout N` finds processes that have run N seconds or more
- **`proc for <file>` found nothing on Windows.** The path was resolved with `canonicalize`, which on Windows returns the extended form `\\?\D:\...`, while process executable paths are plain `D:\...`, so no process ever matched. The `\\?\` prefix is now removed (`\\?\UNC\server\share` becomes `\\server\share`) before the comparison
- **Not-found messages no longer nest a sentence inside a sentence.** `proc for ./Cargo.toml` printed `No process found matching 'No processes found for file: ./Cargo.toml'`, and `proc on <name>` with a filter that matched nothing doubled its quotes. Both passed pre-formatted text into an error template that supplies its own wording and quoting; they now pass the target, so the message reads `No process found matching './Cargo.toml'`.
- **`proc on` put the filter note inside the quotes of its not-found message.** When `--in` or `--by` ruled out everything a target matched, `proc on node --by zzz` said `No process found matching 'node (no matches with specified filters)'`, and `proc on :3000 --in DIR` and `proc on 1234 --in DIR` did the same with `port 3000 (...)` and `PID 1234 (...)`. The target now sits in the quotes and the reason follows them: `No process found matching 'node' (no matches with specified filters)`. The exit code is still 2
- **proc could crash on a non-ASCII path, process name or argument.** Truncation cut strings by byte position, so a cut landing inside a multi-byte character (an accent, CJK, an emoji) panicked instead of printing a row. The cut point is derived from the terminal width, so the same command crashed or succeeded depending on how wide the window was. Truncation now counts display columns and never splits a character, and CJK and emoji — two columns wide each — stay aligned
- **`proc stop`, `orphans --kill` and `stuck --kill` could hang forever** when a killed process became a zombie whose parent didn't reap it (seen on Linux: `proc stop` never returned). Waiting after SIGKILL is now limited to 5 seconds
- **A zombie (exited, not yet reaped) counted as running** on Linux, so `wait` timed out on processes that had already exited, and `stop`, `free` and `unstick` waited out their timeouts on them
- **Zombies were never listed on macOS**, so `proc list --status zombie` found nothing; they are now read from the kernel, as `ps` does
- **A reused PID counted as the original process**: `wait`, `stop` and the others now compare the start time too
- **`proc wait --timeout` overshot by up to `--interval`**: `--timeout 1` took 5 seconds. A process that exited at the last moment could also be reported as still running
- **`proc free --wait` wasn't a maximum**: the wait for the port started again after the force kill, so `--wait 10` could take 15 seconds
- **`proc watch --interval` with `nan`, `inf` or a negative number crashed**, and `0` refreshed in a busy loop using a full CPU core. It now needs at least 0.1 seconds, and anything else exits 4
- **`proc ports --range` ignored a range it couldn't read** (`abc`, `70000-80000`) and listed every port; a reversed range (`9000-8000`) matched nothing. Both now exit 4 and say why
- **A mistyped port such as `:70000` was searched for as a process name** and reported "not found" (exit 2). Every command that takes targets now rejects it as invalid input (exit 4): ports go from `:1` to `:65535`. `list` and `by` take names, not ports, so they still treat `:70000` as a name that matches nothing
- **IPv6 addresses on Linux came out as `]`** (`[::]:80` was split at the wrong colon), so `ports --exposed` left out ports listening on `::` and `--local` missed `::1`. They are now `::` and `::1`, as the filters expect
- **`proc unstick <target>` never tried.** It read CPU over a few milliseconds, which is about 0% for any process, so a named target was always "not stuck", and in the recovery steps every process looked recovered after the first signal. Stuck is now measured like `proc stuck`, and recovery over a full second
- **CPU readings were wrong: a process using a whole core showed as about 0.4%.** Every lookup read CPU a few milliseconds after its first refresh, so `list --sort cpu`, `--min-cpu` and every CPU column were close to meaningless. `list`, `by`, `in`, `for`, `info`, `on`, `tree`, `orphans` and `wait -v` now measure CPU over 0.2 s, from each process's CPU time, before they show, filter or sort by it, which makes them about 0.2 s slower (one wait for all processes, not one each). `kill`, `stop`, `freeze` and `thaw` measure only for a dry run or the confirmation prompt, so with `--yes` they're as fast as before

### Security

- **Signals can't reach a process that reused the PID.** Every signal (`kill`, `stop`, `free`, `freeze`, `thaw`, `unstick` and the `--kill` flags) first checks that the PID still belongs to the same process, by its start time. `stop` sends its SIGKILL up to `--timeout` seconds after finding the target, long enough for a PID to be reused
- **Process names from lsof can't inject terminal escapes or crash proc (macOS).** The full-name change above decodes lsof's `\xNN` escapes; only printable characters are decoded now, so a process named with escape sequences can't control the terminal, and a name with `\x` before a multi-byte character can't make `proc ports` panic. Neither was in a release
- **Process data can't control the terminal.** Names, command lines, working directories, executable paths and users are set by whoever runs a process, and proc printed them as they were, so a process with escape sequences in its arguments, or running from a directory named with them, could recolour, retitle or rewrite the terminal of whoever ran `proc list`, `info`, `ports` or `watch`, or write to its clipboard. Control characters are now shown as visible escapes (`\x1b`, `\n`, `\u{202e}`), as `ps` and `lsof` do, in every view and in JSON. Ordinary text, non-ASCII included, is unchanged

## [1.14.5] - 2026-10-25

### Changed

- **Uptime in every process view.** It was missing from several:
  - `proc on <PID|name>` shows an `Uptime:` line, like `proc on :port`
  - `proc tree` (including `--ancestors`) and `proc why` show `up 3m 12s` on each line, so a long-lived parent such as a forgotten tmux session stands out
  - `proc watch` has the UPTIME column on terminals at least 100 columns wide, and an `uptime:` line with `--verbose`
  - in JSON, `uptime_secs` is now on `tree` and `tree --ancestors` nodes, on `why`'s process and ancestry entries, and on `unstick`'s results; every other command already had it

  The JSON changes only add fields
- **`proc for` uses the standard layouts.** One process gets the same detail view as `proc on :port` (directory, path, listening ports, resources, uptime). Several get the standard process table (directory, arguments, uptime, status) followed by the ports they listen on, instead of a table of their own
- **`proc on :port`** reads uptime from `uptime_secs`, like every other command, instead of working it out from `start_time`
- **npm package: no install script.** The binary is now downloaded the first time you run `proc`, which is already what happened on npm 12 and newer (they skip install scripts). Its checksum is verified either way. `node install.js` in the package directory still downloads it ahead of time, for example in an image that will run offline. This removes the "install scripts" supply-chain alert that scanners such as Socket raise
- **Socket badge** in the README
- **Website: an [agent skill](https://proc-cli.com/skill) page.** It covers installing the skill (`npx skills add`, `bunx`, the Claude Code plugin), updating it, what it teaches agents, and prompts to try. It's linked from the navigation, the install page and the README
- **Demo recording** re-rendered, so `proc why` shows the uptime of each process in the chain

### Fixed

- **Uptime of about 56 years for other users' processes on macOS.** proc can't read their start time, and the system reports it as 0, so `start_time` was `0`, `uptime_secs` was roughly the time since 1970, and `--min-uptime` always matched them. On one Mac that was 233 of 1,516 processes, including `launchd`. Their start time and uptime are now reported as unknown: left out of JSON, `-` in tables, and never matched by `--min-uptime`
- **`--status` with an unknown value matched every process**, so a typo such as `--status stoped` listed everything. It now exits 4 and lists the valid values (`running`, `sleeping`, `stopped`, `zombie`); `stop`, `sleep` and any letter case still work. The same filter is shared by `list`, `by`, `in`, `for` and `tree`, which each had their own copy
- **Windows: `proc stop --signal` ignored the signal** and sent an ordinary stop. Windows has no signals, so `--signal` now exits 4 there with an explanation

## [1.14.4] - 2026-10-25

### Fixed

- **Linux: threads were listed as processes.** `list`, `by`, `in`, `orphans`, `stuck`, `watch` and name targets listed every thread of a process as a separate entry, with the process's command line. A Node dev server, for example, showed up a dozen times, and a name target such as `proc kill node` resolved those extra entries too. Only processes are listed now. macOS and Windows were not affected
- **`proc orphans` and `proc stuck`** printed two headlines for one result ("⚠ Found 2 orphaned processes", then "✓ Found 2 processes"). They now print one

### Changed

- **New demo recording** (README GIF and website video). It tells one story:
  - `npm run dev` fails because port 3000 is taken
  - `proc on`, `proc why` and `proc free` find the old tmux session holding the port and free it
  - `proc orphans` finds the leftovers, shows them as JSON, and cleans them up after a confirmation

  It's recorded in a staged project (`demo/`), so the recording tool's own processes no longer appear, and it's larger and easier to read. To regenerate it, run the two commands at the top of `demo.Dockerfile`
- **`.dockerignore`** keeps `target/` and other unused folders out of Docker build contexts

## [1.14.3] - 2026-10-25

### Changed

- **Repository renamed to [`yazeed/proc-cli`](https://github.com/yazeed/proc-cli)**, matching the crate, the npm package, the agent skill and proc-cli.com. Old `github.com/yazeed/proc` links, git remotes and release downloads redirect, so existing installs keep working. Install commands now use the new name: `npx skills add yazeed/proc-cli`, `/plugin marketplace add yazeed/proc-cli`, `nix profile install github:yazeed/proc-cli`. The Homebrew command is unchanged (`brew install yazeed/proc/proc`)
- **Docker image published as `yazeed/proc-cli`.** Docker Hub can't rename repositories, so each release also pushes the same image to `yazeed/proc`, and `docker pull yazeed/proc` keeps getting updates

### Removed

- **`Formula/proc.rb` and `pkg/scoop/proc.json`**: hand-maintained copies left over from before publishing was automated (frozen at 1.2.2 and 1.3.3 since January). Releases have written the real Homebrew formula and Scoop manifest to `yazeed/homebrew-proc` and `yazeed/scoop-bucket-proc` ever since; nothing read these

## [1.14.2] - 2026-10-25

### Changed

- **Website** moved to [proc-cli.com](https://proc-cli.com). The old `yazeed.github.io/proc` address redirects there
- **PHILOSOPHY.md** describes proc as it is today: every command in the grammar, the JSON and exit-code contract shared by people, scripts and agents, how targets are read without guessing, and `--kill` on `orphans`/`stuck`

### Fixed

- **`proc stop --signal`** only checked the signal name after the dry run and the confirmation prompt. `--dry-run` reported success for a signal the real run rejects (`--signal 9`), and without `--yes` you confirmed before being told the name was invalid. An unknown signal now exits 4 straight away, with or without `--dry-run`

## [1.14.1] - 2026-10-25

### Changed

- **Skill**: tells agents when to reach for proc. It covers:
  - a fuller description of situations that trigger the skill (a busy port, `EADDRINUSE`, leftover dev servers and headless browsers, a slow machine)
  - a table of the shell commands proc replaces (`lsof`, `ps | grep`, `pkill`, `kill`)
  - playbooks for a busy port, cleaning up background work, checking for leftovers before finishing, a slow machine, and clean restarts
  - safety rules for not touching other people's or other agents' processes
  - how to check the installed version and install proc
  - the command aliases, and the flags it was missing (`list --path`, `ports --local`, `tree --depth`/`--compact`, `orphans --kill`)
  - new gotchas: `kill`/`stop` don't signal children, how to find processes left running in a deleted directory, and an orphan isn't necessarily abandoned
- **Release checklist**: triggers a skills.sh refresh after publishing, since skills.sh only re-reads the repo when someone installs the skill

### Fixed

- **`proc in <dir>`** exited 4 ("Directory not found") when the directory had been deleted, even with processes still running in it, such as a dev server left behind in a removed git worktree. It now lists them. A missing path with nothing running in it still exits 4, as a typo
- **Linux: processes in a deleted directory** reported their `cwd` as `<dir> (deleted)`, which the kernel appends, so `--in <dir>` never matched them. `cwd` now shows the plain path, as on macOS, and `--in` matches it

## [1.14.0] - 2026-10-25

### Added

- **Uptime in process output**: every process in JSON has `uptime_secs`; tables from `list`, `by`, `in` and the other process lists show an `UPTIME` column on terminals at least 100 columns wide, and `--verbose` prints it
- **`proc inspect`**: alias of `proc info`, next to `i`
- **Skill**: explains that inspecting a process means `proc info` (or `proc inspect`) and what it reports, that `by` takes one pattern while `list` takes comma-separated ones, and that `start_time` is a timestamp while `uptime_secs` is the duration

### Changed

- **Release checklist**: after publishing, upgrades the local Homebrew `proc` on its own (`brew upgrade yazeed/proc/proc`) and checks its version

## [1.13.4] - 2026-10-25

### Fixed

- **Linux binaries** failed to start on anything older than Ubuntu 24.04 (`GLIBC_2.39 not found`, e.g. on Debian 12), because they were built on the `ubuntu-latest` runner and linked against its glibc. They are now static musl builds (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`) with no glibc dependency. Tested on x86_64 and ARM64 with Amazon Linux 2, Debian 10, 11 and 12, Ubuntu 20.04 and Alpine 3.20. File names are unchanged, so the curl install, npm, Homebrew on Linux and cargo-binstall pick them up as before
- **Docker image**: `on :port`, `ports` and `free` failed with `Failed to run ss: No such file or directory`, because the image didn't include `ss`, which port lookups use on Linux. The image now installs `iproute2`. The documented `docker run` command adds `--net=host`, which port lookups need to see the host's ports
- **`user`** showed the numeric user ID (e.g. `"501"`) in JSON, `info`, `list` and `watch`. It now shows the user name, and falls back to the ID when the ID has no name on the system. Scripts that read `user` as a number need updating

### Changed

- **CI** builds the same Linux targets as a release, including ARM64, so a broken release build shows up on every push instead of at tag time
- **cargo-binstall**: musl hosts (Alpine) now resolve to the Linux release files
- **Release checklist**: runs the published Linux binaries on Debian 11 and Alpine

## [1.13.3] - 2026-10-24

### Fixed

- **`npm install proc-cli`** failed for every version with `Redirect to untrusted host: release-assets.githubusercontent.com`. GitHub now serves release downloads from that host, which the installer's allowlist didn't include. It is now allowed; the existing hosts and the SHA-256 checksum check are unchanged
- **npm 12**: install scripts no longer run by default, so the binary was never downloaded and `proc` crashed. The `proc` command now downloads and verifies the binary on first run when it is missing, with progress on stderr so `--json` output stays clean. If the package directory isn't writable, the binary goes to `~/.cache/proc-cli/<version>` (`%LOCALAPPDATA%\proc-cli\<version>` on Windows). A failed download during install no longer fails `npm install`; it is retried on first run
- **npm wrapper**: a `proc` killed by a signal now exits non-zero, and a binary that can't be started reports an error instead of crashing

### Changed

- **Release checklist**: installs the npm package from a local tarball before tagging, and runs a real `npm install` after publishing

## [1.13.2] - 2026-10-24

### Changed

- **npm publishing** uses Trusted Publishing (OIDC) from the release workflow instead of a stored token. The `NPM_TOKEN` secret is no longer used. The publish job runs on Node 24 with npm 11.5.1 or newer, as Trusted Publishing requires. 1.13.0 and 1.13.1 were not published to npm, so npm goes from 1.12.3 to 1.13.2

## [1.13.1] - 2026-10-24

### Fixed

- **Docker image**: the binary failed to start (`GLIBC_2.39 not found`) on both amd64 and arm64, since at least 1.12.3. The build stage used `rust:latest`, which moved to Debian trixie, while the runtime stage is `debian:bookworm-slim` (glibc 2.36). The build stage now uses `rust:1-bookworm`
- **Release workflow**: the crates.io and npm publish steps reported success even when publishing failed, because the result was piped through `tee` without `pipefail`. This hid that 1.13.0 was never published to npm

## [1.13.0] - 2026-10-24

A consistent JSON and exit-code contract: every command prints one JSON document, `action` is always the command name, and `"success": false` always comes with a non-zero exit code. Scripts that relied on the old behaviour should check the **Changed** entries below.

### Changed

- **`--dry-run --json`** now reports the command's own `action` (e.g. `"kill"`) and `"dry_run": true`, instead of a list result with `"action": "list"` (`kill`, `stop`, `freeze`, `thaw`, `free`, `orphans --kill`, `stuck --kill`)
- **`proc orphans --kill` / `proc stuck --kill` with `--json`** print one action result (`"action": "orphans"`/`"stuck"`, `succeeded`/`failed`) instead of two JSON documents
- **`proc thaw --json`** reports `"action": "thaw"` instead of `"resume"`
- **`proc why` / `proc tree` JSON** use lowercase `status` (`"running"`) and `protocol` (`"tcp"`), matching the Process and Port objects
- **Invalid flags and arguments** exit with code 4 (invalid input) instead of 2, which meant "not found"; with `--json` the error is a JSON object on stdout
- **`proc wait --json` on timeout** exits 1, as it already did without `--json`
- **`proc stop`, `freeze`, `thaw`, `orphans --kill`, `stuck --kill`** exit 1 when any process fails, as `proc kill` already did
- **`proc free`** exits 1 when a port is still in use after `--wait`
- **`proc unstick`** exits 1 when a process is still stuck or failed; an explicit target removed by `--in`/`--by` is "not found" (exit 2)
- **`proc info`** exits 2 when no target is found
- **`proc on` with multiple targets and `--json`** prints one document (`count`, `results` with `target`/`process`/`ports`, `not_found`) instead of one per target; exits 2 when no target is found
- **`proc in <path>`** exits 4 when the directory does not exist, like `proc for` with a missing file

### Fixed

- **`proc why --json`** prints a JSON error and exits 2 when nothing is found, instead of printing nothing and exiting 0; multiple targets print one document instead of one per target
- **`proc kill --json`** no longer prints a second JSON error document after a partial failure
- **Error `action`** uses the canonical command name when an alias is used (`proc k` → `"kill"`)
- **JSON errors** are detected for short-flag clusters such as `-yj`
- **Human output**: "Stopped" instead of "Stoped"
- **Agent skill**: `SKILL.md` covers `proc for`, `proc in`, `proc unstick`, `--ancestors`, `--graceful`, `--exposed`/`--local`/`--range`, `--status`, `--min-uptime`, `--parent`, with a per-command filter table and the bounded `proc watch --json` stream; `reference.md` adds the `on <PID>`, `tree --ancestors`, `--dry-run`, `unstick`, and `watch` shapes
- **Plugin manifest**: `.claude-plugin/marketplace.json` version kept in sync and added to the release checklist

### Added

- **Docs**: agent skill install instructions (`npx skills add yazeed/proc`, Claude Code plugin) in README and the install page; exit code table in README and the commands page
- **Tests**: JSON contract integration tests (`tests/cli_json.rs`)

## [1.12.3] - 2026-05-03

### Fixed

- **Clippy**: Use `sort_by_key` instead of `sort_by` for name sorting (Rust 1.95+ lint)

## [1.12.2] - 2026-03-30

### Security

- **npm install script hardened**: SHA-256 checksum verification, redirect host pinning, `execFileSync` (no shell injection), dead code removed

### Fixed

- **README**: SafeSkill badge, correct badge ordering
- **Skills**: Added `proc for` and empty result JSON schemas to reference
- **Rust docs**: Added `wait`, `free`, `why`, `freeze/thaw`, `--signal`, `--json` to Quick Start and Features

## [1.12.1] - 2026-03-27

### Fixed

- **CI**: Upgrade to `softprops/action-gh-release@v2` — v1 fails with immutable releases enabled

## [1.12.0] - 2026-03-27

### Added

- **`proc wait`**: Block until process(es) exit — the pipe-friendly replacement for `while ps aux | grep ... | grep -v grep; do sleep; done`
  - `proc wait node` — wait until all node processes exit
  - `proc wait :3000 --timeout 60` — wait up to 60s for port process to exit
  - `proc wait node -n 10 -q` — check every 10s, quiet mode for scripting
  - Supports `--in`, `--by`, `--json`, `--verbose` like other commands
  - Reports each process as it exits, with elapsed time
  - JSON output includes per-process exit timing and timeout status

### Fixed

- **`--by` now matches full command line**: The `--by` filter across all commands now matches against both process name AND command line arguments (consistent with how positional targets and `proc by` work). Previously `proc wait node --by KXBTCD` would not match a process whose args contained "KXBTCD" — now it does. Affects: kill, stop, freeze, thaw, info, tree, stuck, unstick, orphans, wait, on, watch, for
- **JSON output for empty results**: Commands that find nothing (`stuck`, `orphans`, `free` when ports already free) now output `{"action":"...", "success":true, "count":0, "message":"..."}` in JSON mode instead of empty stdout
- **Consistent JSON envelopes**: `on` (name target), `for`, `why` now wrap results in `{"action":"...", "success":true, "count":N, "results":[...]}` instead of outputting bare arrays
- **Consistent JSON action field**: All commands now use lowercase action names (`"kill"`, `"stop"`, `"freeze"`, `"resume"`) instead of mixed-case past-tense verbs (`"Killed"`, `"Stopped"`)
- **Command-specific action names**: `by`, `in`, `stuck`, `orphans` now report their own command name as the JSON `action` field instead of generic `"list"`
- **`wait` timeout no longer double-outputs JSON**: Previously emitted both a WaitOutput and an error envelope; now emits a single `{"success":false, "timed_out":true, ...}` object
- **Error envelope includes `action` field**: JSON error responses now include the command name (e.g. `{"action":"kill", "success":false, "error":"...", "exit_code":2}`)
- **`watch` JSON includes `success` field**: Consistent with all other commands
- **`tree --ancestors` uses `action: "tree"`**: Consistent regardless of `--ancestors` flag
- **Ancestor chain exclusion**: Name matching now excludes the entire ancestor process chain (up to 10 levels) instead of just the immediate parent. Prevents false matches in deep shell environments (IDE → terminal → shell → shell wrapper → proc)

### Changed

- **Shared `matches_by_filter`**: Extracted the `--by` matching logic into `core::filters::matches_by_filter()`, replacing 14 inline copies with a single consistent implementation
- **Agent Skills**: Added `proc wait` to skill reference, added "Agent/Pipe Readiness" section documenting which commands are safe for agents vs interactive-only
- **`src/lib.rs`**: Clarified `watch` as interactive TUI, `wait` as pipe-friendly blocking

### Updated

- Updated dependencies (`mio`, `roff`, `unicode-segmentation`, `wasm-bindgen`)

## [1.11.0] - 2026-03-23

### Changed

- **CLAUDE.md**: Added explicit rules requiring user approval before every push, tag, and release. Added mandatory RELEASE.md checklist compliance for all versions including patches

### Updated

- Updated dependencies (`itoa`, `unicode-segmentation`)

## [1.10.1] - 2026-03-23

### Fixed

- **Parent shell self-matching**: Name-based searches (`proc by`, `proc on <name>`, `proc tree <name>`) no longer match the parent shell process whose command line contains proc's own arguments. Previously, `proc on :99999 --json` would match the shell running proc instead of returning "not found". Now correctly returns exit code 2 with a JSON error

## [1.10.0] - 2026-03-23

### Added

- **JSON error envelope**: With `--json`, errors now output `{"success":false, "error":"...", "exit_code":N}` to stdout instead of plain text to stderr — enables reliable machine parsing by LLMs and scripts
- **Agent Skills**: `skills/proc-cli/SKILL.md` — Claude Code skill for process management with proc. Includes structured command reference, JSON output schemas, and LLM usage guidelines. Compatible with the [Agent Skills](https://agentskills.io) open standard

## [1.9.1] - 2026-03-16

### Fixed

- **Windows build**: Gate unix-only imports behind `#[cfg(unix)]` in `freeze.rs` and `thaw.rs` — fixes unused import errors with `-D warnings` on Windows CI

## [1.9.0] - 2026-03-16

### Added

- **`proc freeze`**: Pause processes with SIGSTOP
  - `proc freeze node` — freeze all node processes
  - `proc freeze :3000 --yes` — freeze by port, skip confirmation
  - Supports `--in`, `--by`, `--dry-run`, `--json`, `--verbose`
- **`proc thaw`**: Resume frozen processes with SIGCONT
  - `proc thaw node` — resume all frozen node processes
  - `proc thaw :3000` — resume process on port 3000
  - Supports `--in`, `--by`, `--dry-run`, `--json`, `--verbose`
- **`proc orphans`**: Find orphaned processes (parent exited, reparented to init/launchd)
  - `proc orphans` — list orphaned processes
  - `proc orphans --in .` — orphans in current directory
  - `proc orphans --kill --yes` — find and kill orphans
  - Filters out system daemons on both macOS and Linux
- **`proc why`**: Trace why a port is busy or show process ancestry
  - `proc why :3000` — ancestry chain with port context
  - `proc why node` — how was this node process started?
  - Shows working directory and command for the target process
  - Full JSON output support
- **`proc free`**: Free ports by killing the process and verifying availability
  - `proc free :3000` — kill process, verify port freed
  - `proc free :3000,:8080 --yes` — free multiple ports
  - `proc free :3000 --wait 30` — wait up to 30s for port to free
  - Reports per-port success/failure (handles TIME_WAIT)
  - Port-only targets; rejects PIDs/names with clear error message
- **`--signal` flag on `proc stop`**: Send custom initial signal instead of SIGTERM
  - `proc stop nginx --signal HUP` — reload config (SIGHUP)
  - `proc stop worker --signal USR1` — application-defined signal
  - Accepts signal names: HUP, INT, QUIT, ABRT, KILL, TERM, STOP, CONT, USR1, USR2

### Changed

- **Core**: Added `Process::send_signal()` for arbitrary Unix signal delivery
- **Core**: Added `Process::find_orphans()` for orphan detection with system process filtering

### Updated

- Updated dependencies (`clap`, `sysinfo`, `libc`, `tempfile`, and others)

## [1.8.1] - 2026-03-06

### Fixed

- **README demo image**: Use absolute URL for demo.gif so it renders on Docker Hub, crates.io, and npm

### Added

- **GitHub Pages blog** ([yazeed.github.io/proc](https://yazeed.github.io/proc/)): 7 technical posts covering Unix process management, awk pipelines, ss/netstat/lsof internals, zombie processes, and shell scripting patterns
- **Install page**: Dedicated [install page](https://yazeed.github.io/proc/install) with all platforms, shell completions, and man page setup
- **Blog redesign**: Custom terminal editorial theme — Azeret Mono + Newsreader typography, syntax highlighting, SEO meta tags
- **Blog badge** in README linking to GitHub Pages

## [1.8.0] - 2026-03-06

### Added

- **`proc watch`**: Real-time process monitoring with auto-refresh
  - Watch all processes: `proc watch` (alias: `proc top`, `proc w`)
  - Watch by target: `proc watch node`, `proc watch :3000`, `proc watch 1234`
  - Configurable refresh interval: `--interval/-n` (default: 2s)
  - Sort by cpu, mem, pid, name: `--sort/-s` (default: cpu)
  - Limit results: `--limit/-l`
  - Combines with existing filters: `--in`, `--by`, `--min-cpu`, `--min-mem`
  - Alternate screen + raw mode for clean terminal experience
  - Exit with `q`, `Esc`, or `Ctrl+C` — terminal always restored
  - NDJSON output (`--json`) for streaming to other tools
  - Non-TTY detection: single snapshot when piped
  - Panic hook ensures terminal state is restored on crash

### Fixed

- **Multi-target "not found" output**: Commands (`kill`, `stop`, `info`) now show a single consolidated warning (e.g., `Not found: :3000, :8080`) instead of one line per missing target

## [1.7.4] - 2026-03-01

### Fixed

- **Self-exclusion**: `proc` no longer shows itself in results when using `list`, `by`, `in`, `stuck`, or any command that enumerates all processes. Previously only `find_by_name` excluded the proc process; now `find_all` does too.

## [1.7.3] - 2026-02-26

### Added

- **Working directory in output**: `proc on`, `proc info`, and all table views now show the process working directory — instantly tells you which project folder a process is running from
  - `proc on :3000` shows `Directory: /Users/you/Sites/my-app` instead of only the binary path
  - `proc info` shows `Directory:` between Name and Path
  - `proc on <name>` reverse lookup shows `Directory:` for each process

### Changed

- **Table `PATH` → `DIR` column**: Process tables (`list`, `by`, `in`, `kill`, `stop`, `stuck`) now show the working directory instead of the executable's parent directory — far more useful for identifying which project a process belongs to

### Updated

- Updated dependencies (`clap`, `sysinfo`, `syn`, `regex-syntax`, and others)

## [1.7.2] - 2026-02-11

### Fixed

- **Table column widths**: Fixed columns (PID, CPU%, MEM, STATUS) now account for comfy-table's per-cell padding (2 chars), preventing wrapping of "Sleeping", "Running", and wide memory values
- **ARGS column dominance**: Added upper boundary (`width / 2`) and content truncation so ARGS no longer squeezes PATH and NAME columns on narrow terminals

### Changed

- **`--sort` shows possible values**: Sort flags now use `ValueEnum` enums — invalid values show `[possible values: cpu, mem, pid, name]` instead of a generic error
- **Shared sort logic**: Deduplicated sort match blocks across 5 commands into `sort_processes()` in `src/core/filters.rs`
- **CLAUDE.md release checklist**: Added detailed release steps so the process is always followed

## [1.7.1] - 2026-02-11

### Fixed

- **STATUS column wrapping**: Fixed table column too narrow for "Running"/"Sleeping" text
- **Memory display**: Human-readable adaptive units — `512KB`, `6.0MB`, `3.0GB` instead of always showing MB
- **PATH column too narrow**: Removed hardcoded 19-char truncation, increased minimum width so paths are readable
- **ARGS column showing executable**: When a process has no arguments, shows `-` instead of repeating the full executable path
- **Misleading error tips**: Replaced clap's confusing `tip: to pass '...' as a value, use '-- ...'` with smart "did you mean?" suggestions using edit distance and substring matching (e.g., `--cpu` → `did you mean '--min-cpu'?`)
- **Help example consistency**: Removed `-y` flag from kill example to match the pattern of other examples

## [1.7.0] - 2026-02-11

### Added

- **Terminal-adaptive tables**: Process and port tables now use `comfy-table` with `ContentArrangement::Dynamic` — tables adapt to terminal width automatically, no more overflow on 80-column terminals. Falls back to 120 chars when stdout is not a TTY
- **New filters**:
  - `--min-uptime <seconds>`: Filter by process uptime (on `list`, `by`, `in`, `for`, `tree`)
  - `--parent <pid>`: Filter by parent PID (on `list`, `by`, `in`)
  - `--range <start-end>`: Filter ports by range, e.g. `--range 3000-9000` (on `ports`)
  - `--limit/-n`: Limit result count (on `ports`, `for`)
  - `--sort/-s`: Sort results by cpu, mem, pid, or name (on `for`)
  - `--dry-run`: Preview before killing stuck processes (on `stuck --kill`)
- **Multi-target `proc list`**: Comma-separated names with PID deduplication
  - `proc list node,python` — find both node and python processes
- **`--verbose/-v`**: Added to `stop`, `tree`, and `unstick` (previously missing)
- **Shared utility modules**: `src/core/filters.rs` and `src/ui/format.rs`
- **Unified confirmation prompts**: `print_confirmation` and `print_action_result` in Printer

### Changed

- **`--json` short flag standardized**: All commands now use explicit `-j` (previously some used inferred short flags)
- **`--in` flag on `on` and `for`**: Now supports `num_args = 0..=1` with `default_missing_value = "."` matching all other commands
- **Confirmation prompts unified**: All destructive commands now use `⚠` icon consistently (stop previously used `!`)
- **Philosophy criterion #5 updated**: Changed from "Is it something you'd use weekly?" to "Does it deepen proc's command of its domain?"

### Removed

- **Duplicated code**: Eliminated 12 copies of `resolve_in_dir`, 3 copies of `format_duration`, 3 copies of `truncate_string`, and inline `colorize_status` — all replaced by shared modules

## [1.6.0] - 2026-02-09

### Added

- **`--in` and `--by` flags across all commands**: Consistent filtering by directory and process name
  - `proc kill node --in .` — Only kill node processes in current directory
  - `proc stop node --by worker` — Only stop node processes matching "worker"
  - `proc info node --in . --by server` — Scoped process info
  - `proc tree node --in .` — Tree scoped to directory (targeted mode only)
  - `proc stuck --by node` — Only stuck node processes
  - `proc unstick --in . --by node` — Scoped unstick
- **`--in` flag for `proc ports`**: Filter listening ports by process working directory
  - `proc ports --in .` — Only ports from processes in current directory

### Changed

- **`proc ports`**: Renamed `--filter`/`-f` to `--by`/`-b` for consistency with other commands

### Updated

- Updated dependencies (`clap`, `sysinfo`, `regex`, `anyhow`, and others)

## [1.5.1] - 2026-02-03

### Security

- Updated `bytes` dependency to 1.11.1 to fix integer overflow vulnerability (RUSTSEC-2026-0007)

### Changed

- Added `cargo audit` to release checklist

## [1.5.0] - 2026-02-02

### Added

- **`proc for <file>`**: Find processes by file path
  - Find processes running a specific executable: `proc for /usr/bin/node`
  - Find processes with a file open (via lsof): `proc for ./data.json`
  - Supports relative paths, absolute paths, and tilde expansion: `proc for ~/bin/myapp`
  - Shows process info AND listening ports (like `on` command)
  - Composable filters: `--in`, `--by`, `--min-cpu`, `--min-mem`, `--status`

### Removed

- **`--path` filter from `proc by`**: Use `proc for <file>` instead
- **`--exe-path` filter from `proc in`**: Use `proc for <file>` instead

## [1.4.2] - 2026-02-01

### Fixed

- **Self-matching bug**: `proc kill <target>` no longer kills itself when target matches its own command line arguments
- **Dry-run message visibility**: Message now appears after the process table (not before) so it's visible without scrolling
- **Kill feedback**: All killed processes now show in results (previously proc would die mid-loop if it matched itself)

### Changed

- Destructive commands (`kill`, `stop`, `unstick`) now exclude the current process from target resolution

## [1.4.1] - 2026-02-01

### Added

- **PHILOSOPHY.md**: Project manifesto documenting principles, values, and the feature test
- **RELEASE.md**: Comprehensive release checklist and publishing process
- **CLAUDE.md**: Project-specific instructions for Claude Code
- **Shell completions docs**: Added to README.md
- **Man page docs**: Added to README.md

### Changed

- **ROADMAP.md**: Updated with v1.4.0 highlights, competitive context, and philosophy references
- **src/lib.rs**: Updated crate docs for docs.rs with v1.3/v1.4 features

## [1.4.0] - 2026-01-29

### Added

- **Shell completions**: Generate completions for bash, zsh, and fish
  - `proc completions bash > /etc/bash_completion.d/proc`
  - `proc completions zsh > ~/.zsh/completions/_proc`
  - `proc completions fish > ~/.config/fish/completions/proc.fish`

- **Man page generation**: `proc manpage > /usr/local/share/man/man1/proc.1`

- **`--dry-run` for stop command**: Preview what would be stopped
  - `proc stop node --dry-run`

## [1.3.3] - 2026-01-29

### Changed

- Docs: Complete README rewrite for clarity and completeness
- Docs: Streamlined package READMEs (npm, Scoop, Homebrew) to link to main docs

## [1.3.2] - 2026-01-29

### Added

- **Scoop bucket**: `scoop bucket add proc https://github.com/yazeed/scoop-proc`
- CI: Automated Scoop bucket updates on release

### Fixed

- CI: npm publishing now uses automation token (NPM_TOKEN secret)

## [1.3.1] - 2026-01-29

### Changed

- CI: npm trusted publishing via OIDC (no long-lived tokens)
- CI: Automated Homebrew tap updates on release
- CI: Fixed ARM64 Linux cross-compilation
- CI: Updated Dockerfile to `rust:latest` for edition2024 support
- Docs: Improved README hero examples showcasing multi-target support
- Docs: Added npm badge to README

### Fixed

- npm package now includes README for better discoverability

## [1.3.0] - 2026-01-28

### Added

- **Proc Query Language**: Composable process discovery primitives
  - **`proc by <name>`** — Filter processes by name with all filters
    - Example: `proc by node --in . --min-cpu 5`
  - **`proc in <path>`** — Filter processes by working directory
    - Example: `proc in ~/projects --by python`
    - Supports `.`, relative paths, absolute paths, and `~` expansion

- **Multi-target support** for kill, stop, info, and on commands
  - Comma-separated targets: `proc kill :3000,:8080,node`
  - Mixed target types: ports (`:3000`), PIDs (`1234`), names (`node`)
  - PID deduplication prevents double-kills on overlapping targets
  - Single confirmation prompt shows all targets before action
  - Not-found targets show warnings but don't block found targets

- **Combinable filters** across commands
  - `proc on node --in .` — Node processes on ports, filtered by directory
  - `proc by node --in .` — Node processes in current directory
  - `proc in . --by node` — Same result, different entry point

- **Comprehensive help examples** covering all command variants and scenarios

### Changed

- Updated dependencies: colored 3.1, sysinfo 0.38, indicatif 0.18, nix 0.31

- `proc on` now supports `--in` filter for directory-based filtering
- `proc info` now supports comma-separated targets
- Help text reorganized into semantic sections (Discovery, Info, Kill, Stop, etc.)

## [1.2.3] - 2026-01-21

### Added

- **Tier 3 distribution channels**:
  - **Scoop** (Windows): `pkg/scoop/proc.json` manifest with auto-update
  - **AUR** (Arch Linux): `pkg/aur/PKGBUILD` for `yay -S proc`
  - **npm** wrapper: `pkg/npm/` for `npm install -g proc-cli`
  - **Nix flake**: `flake.nix` for `nix profile install github:yazeed/proc`

## [1.2.2] - 2026-01-21


### Added

- **cargo-binstall support**: `cargo binstall proc-cli` now downloads pre-built binaries
  - 10x faster than `cargo install` (no compilation required)
  - Configured for all platforms: macOS, Linux, Windows (Intel and ARM)
- **Docker Hub automation**: Images automatically published on release
  - Multi-arch support: linux/amd64, linux/arm64
  - Available at `docker run yazeed/proc`

## [1.2.1] - 2026-01-15

### Changed

- Updated dependencies: colored 3.1, sysinfo 0.38, indicatif 0.18, nix 0.31

- Enhanced `--version` output to include repository URL and license

## [1.2.0] - 2026-01-15

### Added

- **ARGS column** in `proc list` output showing command arguments
  - Displays script names for Python/Node processes (e.g., `daily_spread_trader.py`)
  - Simplifies paths to filenames for readability
  - Enables finding processes by script name: `proc list my_script.py`

### Changed

- Updated dependencies: colored 3.1, sysinfo 0.38, indicatif 0.18, nix 0.31

- Reorganized `proc list` columns: PID, PATH, NAME, ARGS, CPU%, MEM, STATUS
- Improved process identification for interpreted languages (Python, Node, Ruby, etc.)

## [1.1.0] - 2026-01-14

### Changed

- Updated dependencies: colored 3.1, sysinfo 0.38, indicatif 0.18, nix 0.31

- **Breaking:** Renamed `ps` command to `list` for better semantics
  - `ps` remains as an alias for backwards compatibility
- Updated tagline: "Semantic CLI tool for process management. Target by port, process id (PID), name or path."

## [1.0.2] - 2026-01-13

### Changed

- Updated dependencies: colored 3.1, sysinfo 0.38, indicatif 0.18, nix 0.31

- Streamlined README with cleaner structure and code-first approach
- Added Targets section explaining unified `:port`, `PID`, `name` syntax
- Removed "Why proc?" comparison section

### Fixed

- Fixed README filter table incorrectly stating `--in` and `--path` work with `tree` (ps-only)

## [1.0.1] - 2026-01-13

### Fixed

- Fixed outdated error message suggesting removed `proc find --all` command
- Fixed `--cwd` references in help text and documentation (renamed to `--in`)
- Fixed JSON output action field from "find" to "ps"

## [1.0.0] - 2026-01-12

Initial public release.

### Commands

All commands accept **targets**: `:port`, `PID`, or `name` where applicable.

**Discovery** (nouns — observe state)
- `proc on <target>` — Bidirectional port/process lookup
  - `:port` → What process is using this port?
  - `PID` → What ports is this process using?
  - `name` → What ports are these processes using?
- `proc ports` — List all listening ports
- `proc ps [name]` — List processes (filter by name, path, or resources)
- `proc info <target>` — Detailed process information
- `proc tree [target]` — Process hierarchy view
- `proc stuck` — Find hung processes

**Lifecycle** (verbs — change state)
- `proc kill <target>` — Force kill (SIGKILL)
- `proc stop <target>` — Graceful stop (SIGTERM, then SIGKILL after timeout)
- `proc unstick [target]` — Attempt to recover stuck processes
  - Tries SIGCONT → SIGINT recovery sequence
  - Use `--force` to terminate if recovery fails

### Features

- **Unified targets**: Most commands accept `:port`, `PID`, or process `name`
- **Path filtering**: `--in` and `--path` filters for `proc ps`
- **Resource filtering**: `--min-cpu`, `--min-mem`, `--status` filters for `ps` and `tree`
- **Bidirectional lookup**: `proc on` works both ways (port→process and process→ports)
- **Cross-platform**: macOS (Apple Silicon, Intel), Linux (x86_64, ARM64), Windows (x86_64)
- **Output formats**: Colored terminal output, JSON (`--json`) for scripting
- **Safety**: Confirmation prompts before destructive actions

### Principles

- **Semantic**: Commands mean what they say
- **Explicit**: User intent must be clear
- **Complete**: Cover the full workflow, nothing more
- **Fast**: Sub-100ms for all operations
- **Obvious**: If you have to read the docs, we failed

### Values

- **Unified targets**: `:port`, `PID`, and `name` work the same way everywhere
- **Natural grammar**: Nouns to observe, verbs to act
- **Practical simplicity**: Every feature solves a real, repeated problem
- **Easy to remember**: Consistent patterns—know one command, know them all

---

[Unreleased]: https://github.com/yazeed/proc-cli/compare/v2.4.0...HEAD
[2.4.0]: https://github.com/yazeed/proc-cli/compare/v2.3.0...v2.4.0
[2.3.0]: https://github.com/yazeed/proc-cli/compare/v2.2.0...v2.3.0
[2.2.0]: https://github.com/yazeed/proc-cli/compare/v2.1.1...v2.2.0
[2.1.1]: https://github.com/yazeed/proc-cli/compare/v2.1.0...v2.1.1
[2.1.0]: https://github.com/yazeed/proc-cli/compare/v2.0.2...v2.1.0
[2.0.2]: https://github.com/yazeed/proc-cli/compare/v2.0.1...v2.0.2
[2.0.1]: https://github.com/yazeed/proc-cli/compare/v2.0.0...v2.0.1
[2.0.0]: https://github.com/yazeed/proc-cli/compare/v1.14.5...v2.0.0
[1.14.5]: https://github.com/yazeed/proc-cli/compare/v1.14.4...v1.14.5
[1.14.4]: https://github.com/yazeed/proc-cli/compare/v1.14.3...v1.14.4
[1.14.3]: https://github.com/yazeed/proc-cli/compare/v1.14.2...v1.14.3
[1.14.2]: https://github.com/yazeed/proc-cli/compare/v1.14.1...v1.14.2
[1.14.1]: https://github.com/yazeed/proc-cli/compare/v1.14.0...v1.14.1
[1.14.0]: https://github.com/yazeed/proc-cli/compare/v1.13.4...v1.14.0
[1.13.4]: https://github.com/yazeed/proc-cli/compare/v1.13.3...v1.13.4
[1.13.3]: https://github.com/yazeed/proc-cli/compare/v1.13.2...v1.13.3
[1.13.2]: https://github.com/yazeed/proc-cli/compare/v1.13.1...v1.13.2
[1.13.1]: https://github.com/yazeed/proc-cli/compare/v1.13.0...v1.13.1
[1.13.0]: https://github.com/yazeed/proc-cli/compare/v1.12.3...v1.13.0
[1.12.3]: https://github.com/yazeed/proc-cli/compare/v1.12.2...v1.12.3
[1.12.2]: https://github.com/yazeed/proc-cli/compare/v1.12.1...v1.12.2
[1.12.1]: https://github.com/yazeed/proc-cli/compare/v1.12.0...v1.12.1
[1.12.0]: https://github.com/yazeed/proc-cli/compare/v1.11.0...v1.12.0
[1.11.0]: https://github.com/yazeed/proc-cli/compare/v1.10.1...v1.11.0
[1.10.1]: https://github.com/yazeed/proc-cli/compare/v1.10.0...v1.10.1
[1.10.0]: https://github.com/yazeed/proc-cli/compare/v1.9.1...v1.10.0
[1.9.1]: https://github.com/yazeed/proc-cli/compare/v1.9.0...v1.9.1
[1.9.0]: https://github.com/yazeed/proc-cli/compare/v1.8.1...v1.9.0
[1.8.1]: https://github.com/yazeed/proc-cli/compare/v1.8.0...v1.8.1
[1.8.0]: https://github.com/yazeed/proc-cli/compare/v1.7.4...v1.8.0
[1.7.4]: https://github.com/yazeed/proc-cli/compare/v1.7.3...v1.7.4
[1.7.3]: https://github.com/yazeed/proc-cli/compare/v1.7.2...v1.7.3
[1.7.2]: https://github.com/yazeed/proc-cli/compare/v1.7.1...v1.7.2
[1.7.1]: https://github.com/yazeed/proc-cli/compare/v1.7.0...v1.7.1
[1.7.0]: https://github.com/yazeed/proc-cli/compare/v1.6.0...v1.7.0
[1.6.0]: https://github.com/yazeed/proc-cli/compare/v1.5.1...v1.6.0
[1.5.1]: https://github.com/yazeed/proc-cli/compare/v1.5.0...v1.5.1
[1.5.0]: https://github.com/yazeed/proc-cli/compare/v1.4.2...v1.5.0
[1.4.2]: https://github.com/yazeed/proc-cli/compare/v1.4.1...v1.4.2
[1.4.1]: https://github.com/yazeed/proc-cli/compare/v1.4.0...v1.4.1
[1.4.0]: https://github.com/yazeed/proc-cli/compare/v1.3.3...v1.4.0
[1.3.3]: https://github.com/yazeed/proc-cli/compare/v1.3.2...v1.3.3
[1.3.2]: https://github.com/yazeed/proc-cli/compare/v1.3.1...v1.3.2
[1.3.1]: https://github.com/yazeed/proc-cli/compare/v1.3.0...v1.3.1
[1.3.0]: https://github.com/yazeed/proc-cli/compare/v1.2.3...v1.3.0
[1.2.3]: https://github.com/yazeed/proc-cli/compare/v1.2.2...v1.2.3
[1.2.2]: https://github.com/yazeed/proc-cli/compare/v1.2.1...v1.2.2
[1.2.1]: https://github.com/yazeed/proc-cli/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/yazeed/proc-cli/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/yazeed/proc-cli/compare/v1.0.2...v1.1.0
[1.0.2]: https://github.com/yazeed/proc-cli/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/yazeed/proc-cli/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/yazeed/proc-cli/releases/tag/v1.0.0
