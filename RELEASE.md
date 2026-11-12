# Release Process

Follow these steps for every release. Do not skip steps.

## 1. Pre-Release Checks

```bash
rustup update stable                           # The latest stable Rust: CI and the release builds use it
cargo update                                   # Update dependencies first, so everything below tests them
cargo fmt --check                              # Code formatted
cargo clippy --all-targets -- -D warnings      # No warnings, tests included (as CI runs it)
cargo test                                     # Tests pass
cargo build --release                          # Release builds
cargo audit                                    # No security vulnerabilities
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps # The docs.rs pages build with no broken links
```

- [ ] CI is green on main, including "Coverage gate" (100% lines across Linux, macOS and Windows; see CONTRIBUTING.md "Coverage"), both "Nix" jobs (the flake builds proc and runs its tests in Nix's sandbox, macOS's included, and the nixpkgs package builds), both "Test (freebsd)"/"Test (netbsd)" jobs (the whole suite in a BSD VM, then the release binary built and run there) and the "Package .deb and .rpm" and "Install the packages" jobs (the four packages install, run and remove in Debian, Ubuntu and Fedora). A tag release does not wait for it. Locally: `scripts/coverage-local.sh`
- [ ] The release workflow's dry run is green: `gh workflow run release.yml --ref main`, then `gh run watch`. It runs every build and package job (`build-bsd` and `package-linux` among them, the release's own forms of CI's BSD and `.deb`/`.rpm` jobs) and assembles the release files with `checksums.txt`, and publishes nothing: the GitHub Release step and the crates.io, npm, PyPI, Homebrew, Scoop, apt and COPR jobs run only for a tag, the winget job prints what it would change in the first version's pull request, and the nixpkgs, FreeBSD and pkgsrc jobs run for the version in Cargo.toml and push or file nothing (step 5). Those three build that version from a tarball of the checkout (`git archive --prefix=proc-cli-X.Y.Z/`, which holds what GitHub's download of the tag will: its source hash matched GitHub's for 2.3.0), since a version has no download before its tag. Here, before step 3's bump, that is the last release's version with this release's code, so `update-pkgsrc` may stop: its diff from pkgsrc's copy is empty once pkgsrc has that release, or `COMMIT_MSG` already names the next one. The dry run that must be green in every job is the one step 5 runs after the release commit, which builds the new version itself The PyPI wheels are built and installed on every OS in the dry run (`package-pypi`, `test-pypi`); only `publish-pypi` waits for a tag. Its version checks print instead of failing, since a branch build is a `-dev` build; the tag checks them
- [ ] The dry run's "Check credentials" job passed and printed no warning. It tries every secret under "Required Secrets" and reads when each expires (GitHub reports a token's expiry, so no date is kept anywhere): one that is invalid or expired fails the job, and the tag's release then publishes nothing; one that expires within 30 days is a warning. Renew it now, set it with `gh secret set <NAME>`, and run the dry run again
- [ ] The credentials outside CI still work. `check-credentials` can't test them, since they never leave the maintainer's Mac, so the release session runs this there:

  ```bash
  gpg -K 227B9E7E7C22B6E88774325184D7BBF5E38CCC4D   # the PPA key: listed, and no "[expires: ...]" (it has none)
  grep expiration ~/.config/copr                     # the COPR API token: expires 2027-05-02; a new one is at https://copr.fedorainfracloud.org/api/
  test -r ~/.ssh/pkgsrc-wip && echo pkgsrc-wip key ok # only for an update sent through pkgsrc-wip; a gnats diff needs no key
  ```

  A release itself needs neither token: COPR builds from the webhook, and the COPR token only matters if the package's settings change. The FreeBSD bug and a gnats report are filed in a browser, with the maintainer's own logins (bugs.freebsd.org, gnats.netbsd.org).

### Cross-Platform Check

Code behind `#[cfg(unix)]`, `#[cfg(windows)]` or a BSD's `target_os` (signals, `freeze`/`thaw`, lsof, fstat, netstat and sockstat parsing) only compiles on its own platform, so a Unix-only import or helper breaks another build with `-D warnings`. Always verify, tests included:

```bash
rustup target add x86_64-pc-windows-msvc x86_64-unknown-freebsd x86_64-unknown-netbsd  # One-time setup
cargo check --target x86_64-pc-windows-msvc --all-targets
cargo clippy --target x86_64-unknown-freebsd --all-targets -- -D warnings
cargo clippy --target x86_64-unknown-netbsd --all-targets -- -D warnings
```

Clippy for the BSDs needs no linker, so it runs on any machine; their tests run only in CI's BSD jobs.

### Oldest Supported Rust

`rust-version` in Cargo.toml is the oldest Rust that builds proc. It matters to people who build from source (`cargo install`, distro packages) and to library users; the prebuilt binaries don't depend on it. It is raised only when something proc builds needs a newer Rust:

- **When:** a dependency update needs a newer Rust (the highest `rust_version` among the dependencies goes above ours, or the check below fails), or we decide to use a language or standard-library feature that needs one and it is worth more than the way we have. Never because a new stable came out.
- **To what:** the lowest version that builds, which is the highest one the dependencies declare (command below) or the version that stabilized the feature. Never to the latest stable.
- **How:** in the same commit, `rust-version` in Cargo.toml, the version in CONTRIBUTING.md's prerequisites and in the commands below, and a CHANGELOG entry ("building from source needs Rust 1.NN"). Any release may carry it.

```bash
rustup toolchain install 1.88 --profile minimal  # One-time setup, the version in Cargo.toml
cargo +1.88 check --all-targets                  # The declared floor still builds
# The highest Rust the dependencies declare; above ours means a raise
cargo metadata --format-version 1 | jq -r '.packages[].rust_version // empty' | sort -V | tail -1
```

## 2. Manual Testing

Test new features manually with the release binary:

```bash
./target/release/proc --version    # e.g. "proc X.Y.Z-dev (main@abc1234, 3 commits after vX.Y.W, modified)"
./target/release/proc --help       # Verify help includes new commands
```

Before the tag exists, `--version` names the branch and commit: only a build of the release tag prints the plain `proc X.Y.Z` (build.rs; see "Automated Publishing" for how CI checks it). After the version bump in step 3 the version part must read the new version.

Test new commands/flags work as expected before releasing.

For a breaking release, also run a migration case for every **BREAKING** entry in the release's CHANGELOG section: the old usage must fail the way the entry says, and the new one must work. For 2.0.0 that was `proc by <part-of-name>` (exits 2; adding `--cmd` matches), `proc kill <pid> --json` (exits 4 with `needs_confirmation`) and `proc kill <pid> <pid> --dry-run` (previews both).

### Install path check (before tagging)

Version numbers alone don't prove a channel works. Install the npm package from a local tarball. Run this **before the version bump in step 3**: the installer downloads the binary for the version in `pkg/npm/package.json`, which at this point is the previous, already-published release, so this tests the installer itself:

```bash
cd pkg/npm && npm pack --pack-destination /tmp/proc-npm && cd -
mkdir -p /tmp/proc-npm/install && cd /tmp/proc-npm/install
npm install /tmp/proc-npm/proc-cli-*.tgz && ./node_modules/.bin/proc --version
```

Build and run the Docker image from the release tree too. The release workflow builds it the same way, from a context without `.git` (so it prints the plain version). `docker-check.yml` already builds the image on each change to the Dockerfile, the manifests or `src/`, natively on amd64 and arm64, and checks `--version`, `list`, `on :port`, `ports` and `for` in it; this run is the release tree's own check:

```bash
docker build -t proc-cli:release-check .
docker run --rm proc-cli:release-check --version
docker run --rm proc-cli:release-check list --json | head -5
docker rmi proc-cli:release-check
```

If anything here fails, fix it in this release, before the tag.

## 3. Version Bump

Update version in ALL of these files:
- [ ] `Cargo.toml` — version field
- [ ] `pkg/npm/package.json` — version field
- [ ] `.claude-plugin/marketplace.json` — `metadata.version` field
- [ ] `docs/_data/release.yml` — `version` field (the website's header, footer, install page and structured data read it)
- [ ] `Cargo.lock` — rebuild after the bump so its `proc-cli` entry changes, and commit it (`flake.nix` reads its version from `Cargo.toml` and its dependencies from `Cargo.lock`, so it needs no bump of its own)

`pkg/aur/PKGBUILD` is not bumped. Both AUR packages are community packages, kept by their own maintainer: `proc-cli` (built from source) and `proc-cli-bin` (the release binaries); they moved from `proc` and `proc-bin`, which are orphaned at 2.1.1, with 2.2.0. The project has no AUR account yet, so the README points Arch users to those packages, and the PKGBUILD stays in the repo for the day the project can co-maintain them (see "After the tag: AUR" below). `pkg/copr/proc-cli.spec` and `pkg/winget/*.yaml` are not bumped either: COPR's `sources.sh` sets the spec's version at build time, and the release's `update-winget` writes the winget manifests (`pkg/winget/move-pr.sh` while the first version waits for a moderator, which `commit-package-files` brings to `main`; winget-releaser in the winget-pkgs pull request after that).

## 4. Documentation

Update and verify all docs are complete:

- [ ] `pkg/pkgsrc/COMMIT_MSG` — the commit message the pkgsrc committer pastes as it is: `sysutils/proc-cli: update to X.Y.Z`, a blank line, `Changes since A.B.C:`, then one to five `- ` bullets of one line each (at most 72 characters), plain English, only what a user of the package would notice. Install channels, release tooling and docs stay out. `update-pkgsrc` stops if it isn't in that shape or names another version, and puts it at the top of the report's description under "Suggested commit message:", followed by the CHANGELOG address and what it tested. 2.3.0's:

  ```
  sysutils/proc-cli: update to 2.3.0

  Changes since 2.2.0:
  - on, info, for and why show the same card for a process
  - process tables show the user, uptime and command
  - ports says if each port is reachable from the network
  - values proc cannot read show as "-", not 0
  - JSON adds parent name, child count, CPU time, open files
  ```

- [ ] `CHANGELOG.md` — Rename `## [Unreleased]` to `## [X.Y.Z] - YYYY-MM-DD` and add a new empty `## [Unreleased]` above it. Everything worked on since the last release is already listed there (see "Unreleased Work" in CLAUDE.md); check that it is complete. Update the links at the bottom: `[Unreleased]` compares `vX.Y.Z...HEAD`, and add `[X.Y.Z]` comparing the previous tag. Each heading (`### Added`, `### Fixed`, ...) appears once per release; merge any duplicates. Note every issue and PR the section references (`#n`): step 7 replies to them
- [ ] `docs/changelog.md` — Run `python3 scripts/changelog_site.py` after renaming the section: it writes the release into the website's changelog from CHANGELOG.md (never edit that part by hand; CI fails when the two differ)
- [ ] `README.md` — Review thoroughly: hero examples, Quick Start, Commands table, Filters table, Examples section all reflect new features
- [ ] `ROADMAP.md` — Remove everything this release shipped. The roadmap only holds what's still ahead (the CHANGELOG is the history)
- [ ] `src/lib.rs` — Review Rust docs: Features list, Quick Start examples, Commands list all reflect new features. Run `cargo doc --no-deps --open` to preview. This is what shows on docs.rs.
- [ ] `skills/proc-cli/SKILL.md` — Agent Skills: new commands, flags, schemas, playbooks (the "proc 2.0 or later" floor stays unless the skill needs a newer proc)
- [ ] `skills/proc-cli/evals.md` — if the skill's rules or playbooks changed in this release, run its tasks with the new SKILL.md loaded (a few paraphrases each, on a small model and a large one) and read the commands the agent ran; every Must met, no Must never. Add a task for any new rule
- [ ] `skills/proc-cli/reference.md` — JSON output schemas match actual output
- [ ] `SECURITY.md` — supported versions table (new major or minor line)
- [ ] `docs/commands/` — GitHub Pages: new command pages, index.md updated (its exit-code table matches the README's)
- [ ] The rest of the website: `docs/index.html`, `docs/install.md`, `docs/faq.md`, `docs/skill.md`, `docs/_data/`
- [ ] `pkg/npm/README.md` (the npm page) and the `.claude-plugin/marketplace.json` descriptions
- [ ] Every surface at once: for each entry in the release's CHANGELOG section, search all of the files above, `src/lib.rs` and the `--help` text for what the entry makes untrue (the old message, exit code or behaviour), and fix what is found. Check the search finds a known mention first
- [ ] Verify `proc manpage` output includes new commands, and `proc manpage --dir` writes a page for each
- [ ] Verify `proc completions` includes new commands

## 5. Commit & Tag

Stage the release files by path, not with `git add -A`: check `git status` first, because uncommitted work that isn't part of the release must stay out of it.

```bash
git status --short                       # only the release's files should be listed
git add Cargo.toml Cargo.lock CHANGELOG.md docs/changelog.md ...   # the files from steps 3 and 4
git commit -m "chore: release vX.Y.Z"
git push origin main
gh workflow run release.yml --ref main   # the dry run again, now for X.Y.Z: every job green, then tag
git tag vX.Y.Z
git push origin vX.Y.Z                   # this one tag (not --tags, which pushes every local tag)
```

That dry run builds X.Y.Z everywhere, nixpkgs, the FreeBSD port and pkgsrc included (from the checkout, step 1), so a packaging problem shows before anything is published. Once it is green, nothing goes to `main` before the tag: the tag goes on the commit the dry run tested.

The tag must be `v` + Cargo.toml's version: both the release and Docker workflows stop before publishing anything otherwise. A repository ruleset stops `v*` tags from being moved or deleted once pushed, and `main` from being force-pushed or deleted. If a tag must be redone because nothing was published from it, switch the tag ruleset off (Settings → Rules → Rulesets) for that one fix, then back on.

### After the tag: AUR

Nothing to publish: the AUR's `proc-cli` and `proc-cli-bin` belong to their community maintainer. Step 7 checks that they caught up with the release. Once the project has an AUR account and co-maintains them, this step goes back to updating the AUR repositories with a PKGBUILD whose `sha256sums` is the tag tarball's checksum (`curl -fsSL https://github.com/yazeed/proc-cli/archive/vX.Y.Z.tar.gz | shasum -a 256`) and a regenerated `.SRCINFO` (`makepkg --printsrcinfo > .SRCINFO` on Arch).

### After the tag: nixpkgs

nixpkgs builds proc from the tag, with its tests, as `proc-cli`. `pkg/nixpkgs/package.nix` is the file nixpkgs carries at `pkgs/by-name/pr/proc-cli/package.nix`. Until the package is merged, the pull request on NixOS/nixpkgs ([#581248](https://github.com/NixOS/nixpkgs/pull/581248), from `yazeed/nixpkgs`, branch `proc-cli-init`) carries two commits, `maintainers: add yazeed` and `proc-cli: init at X.Y.Z`, and the release's `update-nixpkgs` job moves it to each release:

1. `pkg/nixpkgs/update.sh X.Y.Z` sets the version, puts `lib.fakeHash` in both hashes and builds with `pkg/nixpkgs/build.nix` (the nixpkgs in `flake.lock`, plus the maintainer entry nixpkgs may not have yet): the first build fails naming the source hash, the second the cargo hash. It puts each in, then builds once more, which runs the whole test suite in the sandbox, and checks the result prints `proc X.Y.Z`.
2. `pkg/nixpkgs/update-fork.sh X.Y.Z --push` amends `proc-cli: init at …` on the fork's branch to that `package.nix` and the new version, keeping its author and its `Assisted-by:` trailer (nixpkgs asks commits written with an AI assistant for one), force-pushes it with a lease on the commit it replaced, and retitles the pull request. It stops if the branch isn't those two commits, or if `package.nix` differs from the branch in more than the version and hash lines: bring such a change to the pull request by hand. If the retitle is refused, the job warns with the `gh pr edit` command.
3. The release's `commit-package-files` job commits that `package.nix` to `main` ("chore: package files for vX.Y.Z"), with the FreeBSD and pkgsrc files; it is also the run's `nixpkgs-package` artifact.

A dry run does steps 1 and 2 for the version in `Cargo.toml`, from a tarball of the checkout (`PROC_SOURCE`, step 1 of this file), and pushes nothing. To run them locally, in the `nixos/nix` image:

```bash
docker run --rm --cpus 4 -v "$PWD":/work -w /work -e NIX_CONFIG="experimental-features = nix-command flakes" \
  nixos/nix sh pkg/nixpkgs/update.sh X.Y.Z    # ends "built and tested: proc X.Y.Z"
```

Before the tag, make the tarball first and pass it: `git archive --format=tar.gz --prefix=proc-cli-X.Y.Z/ -o source.tar.gz HEAD`, then add `-e PROC_SOURCE=/work/source.tar.gz` to the `docker run`.

Once the package is merged, `update-fork.sh` does nothing, and nixpkgs' update bot opens `proc-cli: A -> B` after a release: check its hashes match the job's and its build passes, and approve it as the package's maintainer.

### After the tag: Ubuntu PPA

`ppa:yazeedaloyoun/proc-cli` builds proc for Ubuntu 22.04, 24.04, 26.04 and 26.10 (amd64 and arm64) from source packages signed with the maintainer's key. The key stays on the maintainer's Mac and is never given to CI, so this step runs there, as one command, once the tag is pushed:

```bash
pkg/ppa/release.sh X.Y.Z      # builds the source packages in Docker, signs and uploads them
```

It builds in an `ubuntu:26.04` container (`pkg/ppa/build-source.sh`), signs with the maintainer's key (gpg asks for its passphrase unless the agent or the macOS keychain has it) and uploads by FTP (`pkg/ppa/upload.sh`). The first release's upload carries the two tarballs; the script then waits for Launchpad to publish them, usually within the hour and for 3 hours at most (it fails with "still not sent after 3 hours"), and sends the other three. It ends once Launchpad has accepted all four, and fails, naming the version, if one is not accepted within an hour after that (Launchpad emails why). Launchpad's FTP times out often: uploads retry, and running the command again sends only what has not gone. It stops before building if Docker is not running or the tag has no GitHub release, and a build that stops halfway is never reused. Launchpad can take hours to process an upload: 2.3.0's first one, sent at 11:20 UTC, was accepted at 14:35, with no email and no "Pending" source in the meantime. Don't send it again while you wait: the script's wait covers it, and a second copy of the same version only comes back "Rejected: ... is already accepted in ubuntu/<series> and you cannot upload the same version", which changes nothing. A packaging fix to a released version is `pkg/ppa/release.sh X.Y.Z 2`. A new Ubuntu release joins the list in `build-source.sh`; check its Rust is at least proc's `rust-version`. Step 7 checks the builds.

### After the tag: FreeBSD port

`sysutils/proc-cli` (`pkg/freebsd/`) was submitted as [bug 300492](https://bugs.freebsd.org/bugzilla/show_bug.cgi?id=300492) and waits for a ports committer. The release's `update-freebsd` job moves it to each release:

1. `pkg/freebsd/update-port.sh X.Y.Z OUT_DIR` runs in a FreeBSD VM with a ports tree (`git clone --depth 1 https://git.FreeBSD.org/ports.git`). While the port is not in the tree it starts from `pkg/freebsd/*`; once it is, from the tree's own port, which a committer may have changed. It sets `DISTVERSION`, then runs `make makesum`, `make extract`, `make cargo-crates > Makefile.crates` and `make makesum` again (the second picks up the crates), all with `BATCH=yes` (without it, a dependency's options dialog waits for a terminal nobody is at). Extract first: on a port that isn't extracted, `make cargo-crates` prints the framework's progress lines (`===> License MIT accepted…`) into `Makefile.crates` too, as it did on the first 2.3.0 run; the script stops unless the list starts with `CARGO_CRATES=` and has none. Then `make clean`, so the build extracts the crates the port now lists (built in the first extraction, 2.4.0's dry run compiled the last release's itoa), `make stage check-plist stage-qa package`, the package installed and its `proc --version` checked, `make clean` and `portlint -AC`. A new port also gets `SUBDIR += proc-cli` in `sysutils/Makefile`, in alphabetical order. It copies the port's files back to `pkg/freebsd/` and writes the patch (`git format-patch`, author `me@yazeed.com`) and its kind, `new` or `update`, to OUT_DIR. The ports tree's `lang/rust` must match the installed `rust` package, or the build starts compiling Rust itself: a ports tree from the main branch needs pkg's `latest` branch, not `quarterly` (2.3.0: the tree wanted Rust 1.99.1, quarterly had 1.98.1); the job sets it up that way, by moving FreeBSD 15's own repository, `FreeBSD-ports`, to it (`FreeBSD-ports: { url: "pkg+https://pkg.FreeBSD.org/${ABI}/latest", mirror_type: "srv" }` in `/usr/local/etc/pkg/repos/FreeBSD.conf`). A second repository beside it with the same packages, as the job had until 2.4.0 (`FreeBSD:`, the 14.x name), makes `pkg install` loop on upgrading pkg itself whenever a new pkg is out, until the job is killed.
2. `pkg/freebsd/file-patch.sh X.Y.Z OUT_DIR --file` files it as the maintainer (`FREEBSD_BUGZILLA_API_KEY`, a key of the Bugzilla account `me@yazeed.com`, the port's `MAINTAINER`): a `new` patch is attached to bug 300492 as a patch with `maintainer-approval` `+`, and the patches it replaces there are marked obsolete after it; an `update` patch opens a new bug in Ports & Packages → Individual Port(s), `[PATCH] sysutils/proc-cli: Update to X.Y.Z`, and is attached to it the same way. Run again, it files nothing it has filed already.
3. The release's `commit-package-files` job commits the port's files (`Makefile`, `Makefile.crates`, `distinfo`, `pkg-descr`) to `pkg/freebsd/` on `main`; they are also in the run's `freebsd-port` artifact, with the patch.

A dry run does step 1 for the version in `Cargo.toml`, from a tarball of the checkout (`PROC_SOURCE`), and only says what step 2 would file.

### After the tag: pkgsrc

`sysutils/proc-cli` (`pkg/pkgsrc/`) is in pkgsrc itself, imported at 2.2.0 on 2026-11-04; `wip/proc-cli` was removed then. The pkgsrc developers named two ways to send an update: a diff through a problem report at https://gnats.netbsd.org for a small change such as a version bump, or the new version re-added to pkgsrc-wip (account `yazeed`, key `~/.ssh/pkgsrc-wip` on the maintainer's Mac) for a committer to take. Every release sends a problem report.

The release's `update-pkgsrc` job prepares it; the report itself is filed by hand, since the gnats form has a check for people:

1. `pkg/pkgsrc/update-package.sh X.Y.Z QUARTER OUT_DIR` runs in a NetBSD 11.0 VM. It takes the package as it is on pkgsrc trunk (NetBSD/pkgsrc's GitHub mirror), so the diff applies there, sets `DISTNAME`, runs `make makesum`, `make print-cargo-depends > cargo-depends.mk` (keeping trunk's `$NetBSD$` line) and `make makesum` again, then `make clean` (the first extraction holds trunk's crates, which this `Cargo.lock` doesn't lock: 2.4.0's dry run failed on it) and `make package install`, the installed `proc --version`, `make clean` and `pkglint`. It builds in the quarterly branch NetBSD's binary packages come from (the job reads it from where `…/packages/NetBSD/x86_64/11.0` points, `11.0_2026Q3` for 2.3.0), because a branch whose `lang/rust` differs from the binary `rust` builds Rust itself. It copies the package's files back to `pkg/pkgsrc/` and writes `proc-cli-X.Y.Z-pkgsrc.diff` (`diff -ruN`, applied with `patch -p1` from the pkgsrc root), the report's description (`pkg/pkgsrc/COMMIT_MSG` under "Suggested commit message:", then the CHANGELOG address and what was tested where) and, from the job, `gnats-fields.json` with every field of the form. `COMMIT_MSG` is written for the version before the tag, in the shape step 4 gives; the script stops if it isn't in that shape or names another version. A dry run runs it for the version in `Cargo.toml`, from a tarball of the checkout (`PROC_SOURCE`, taken as the distfile): after the bump, the release's own version, so step 5's dry run tests this step before the tag. Before the bump it is the last release's version, and the run stops once pkgsrc has that release ("diff found no change from trunk") or if `COMMIT_MSG` already names the next one.
2. The release's `commit-package-files` job commits the package's files (`DESCR`, `Makefile`, `PLIST`, `cargo-depends.mk`, `distinfo`) to `pkg/pkgsrc/` on `main`; they are also in the run's `pkgsrc-update` artifact, with the diff and `gnats-fields.json`.
3. File the report at https://www.netbsd.org/cgi-bin/sendpr.cgi?gndb=netbsd with the fields from `gnats-fields.json` (category `pkg`, synopsis `sysutils/proc-cli: update to X.Y.Z`, the diff in "fix"). The form is filled in the maintainer's browser; the maintainer answers its check and submits. A pkgsrc developer commits it.

The other way, re-adding the version to pkgsrc-wip for a committer to take, stays for changes bigger than an update.

## 6. Verify CI

After tag push, monitor GitHub Actions:
- [ ] "Check credentials" passed: it gates the GitHub Release, so nothing publishes without it
- [ ] Build succeeds for all platforms, including each build's "Check the binary reports the tag's version" step
- [ ] Both "Build freebsd/netbsd" jobs pass: each BSD binary is built in its own OS and prints the tag's version there (the GitHub Release needs them)
- [ ] Docker's "Check the tag matches Cargo.toml" step passes
- [ ] GitHub Release created with binaries (`proc-freebsd-x86_64` and `proc-netbsd-x86_64` among them, each with its `.tar.gz`), the four `.deb`/`.rpm` packages and `checksums.txt` (the `package-linux` job passed)
- [ ] crates.io published
- [ ] npm published
- [ ] Homebrew tap updated
- [ ] Scoop bucket updated
- [ ] Docker image pushed
- [ ] "Update the apt repository" passed: https://proc-cli.com/apt/dists/stable/main/binary-amd64/Packages lists the new version; the job checks it itself, for amd64 and arm64, and fails if it doesn't. Pages keeps only the first deploy of a commit, and the tagged commit was deployed by its own push, before the release had its `.deb` files, so the job deploys main's newest commit instead: the one "Commit the package files" pushed. If that job pushed nothing, this one fails and says so: deploy after the next push to main with `gh workflow run pages.yml --ref main`
- [ ] "Update winget" passed. Until a moderator merges the first version, microsoft/winget-pkgs#469298, it moves that pull request to the new version (`pkg/winget/move-pr.sh`: one commit replacing the version folder's three manifests, then the title and description), so it lands at the current release; it was opened at 2.2.0 and stayed there through 2.3.0 until it was moved to 2.4.0 by hand on 2026-11-11. Once winget-pkgs has the package, the job opens the new version's own pull request (winget-releaser). If the first version is merged between two releases, rerun this job in the latest release's run so winget catches up: `gh run rerun <run id> --job <job id>` (the ids are in `gh run view <run id> --json jobs`)
- [ ] Every submission still waiting for review offers the new version, in its title, its files and what its description says: the winget pull request (#469298, while unmerged), the nixpkgs pull request (#581248: title, `package.nix`, and each claim in the description, such as which platforms it was built on), FreeBSD bug 300492 (the newest patch is the new version's, the older ones obsolete) and the pkgsrc problem report. A job that skipped with a notice is not a pass: open each one
- [ ] The PPA's builds for the new version succeeded on every release and both architectures (https://launchpad.net/~yazeedaloyoun/+archive/ubuntu/proc-cli/+packages)
- [ ] "Update COPR" passed: the build for the new version succeeded on every chroot (https://copr.fedorainfracloud.org/coprs/yazeed/proc-cli/builds/)
- [ ] The "Install the PyPI wheel" jobs passed (Linux x86_64 and aarch64, Alpine, macOS arm64 and x86_64, Windows)
- [ ] PyPI published: https://pypi.org/project/proc-cli/ shows the new version
- [ ] "Update nixpkgs" passed: https://github.com/NixOS/nixpkgs/pull/581248 is titled `proc-cli: init at X.Y.Z` and its checks pass; the job's `package.nix` is committed to `main`
- [ ] "Update the FreeBSD port" passed: the patch for the new version is on https://bugs.freebsd.org/bugzilla/show_bug.cgi?id=300492 (or its own update bug, once the port is committed) with `maintainer-approval+`; the job's `pkg/freebsd/` files are committed to `main`
- [ ] "Update pkgsrc" passed, and its `pkgsrc-update` artifact has the diff and `gnats-fields.json`
- [ ] "Commit the package files" passed: `main` has `chore: package files for vX.Y.Z`, with the files of each of the package jobs that passed, winget's manifests among them until its first version is merged (a job that failed leaves its folder as it was, with a warning). It pushes with the workflow's own token, so the commit starts no CI run of its own
- [ ] The after-the-tag steps are done: nixpkgs, the Ubuntu PPA, the FreeBSD port, the pkgsrc update sent (step 5)

## 7. Post-Release Verification

```bash
# Verify packages (after CI completes)
cargo install proc-cli                    # crates.io
npm info proc-cli version                 # npm
npm install --prefix /tmp/proc-npm-check proc-cli && /tmp/proc-npm-check/node_modules/.bin/proc --version  # npm installer works
# install.sh, npm and cargo-binstall on FreeBSD and NetBSD download these files (CI already ran
# the binaries in their OS): each must answer 302
for f in proc-freebsd-x86_64 proc-freebsd-x86_64.tar.gz proc-netbsd-x86_64 proc-netbsd-x86_64.tar.gz; do
  curl -sI -o /dev/null -w "%{http_code} $f\n" https://github.com/yazeed/proc-cli/releases/download/vX.Y.Z/$f
done
brew update && brew info yazeed/proc/proc # Homebrew
brew upgrade yazeed/proc/proc && proc --version  # upgrades only proc, nothing else
docker pull yazeed/proc-cli               # Docker (the same image is also pushed as yazeed/proc)
nix run github:yazeed/proc-cli/vX.Y.Z -- --version  # Nix: builds the tag from the flake; prints proc X.Y.Z
# The same without Nix installed, in the nixos/nix image (afterwards: docker rmi nixos/nix, then the prunes in step 8)
docker run --rm -e NIX_CONFIG="experimental-features = nix-command flakes" nixos/nix nix run github:yazeed/proc-cli/vX.Y.Z -- --version

# nixpkgs: the open pull request (or, once merged, the update bot's) builds the new version and passes;
# after it reaches nixos-unstable, `nix run nixpkgs#proc-cli -- --version` prints proc X.Y.Z

# AUR (the community maintainer's packages): both should reach the new version within a few days
curl -s 'https://aur.archlinux.org/rpc/v5/info?arg[]=proc-cli&arg[]=proc-cli-bin' | jq -r '.results[] | "\(.Name) \(.Version)"'

# FreeBSD port: the bug (300492 until the port is in, then the update bug) has the new patch;
# once committed, https://www.freshports.org/sysutils/proc-cli/ shows the new version
# pkgsrc: sysutils/proc-cli is at the new version once a developer commits the update (mirrored on GitHub)
gh api 'repos/NetBSD/pkgsrc/commits?path=sysutils/proc-cli&per_page=1' -q '.[0].commit.message'

# Linux binaries are static (musl): they must run on an older distro and on Alpine
mkdir -p /tmp/proc-linux && cd /tmp/proc-linux
curl -fsSL https://github.com/yazeed/proc-cli/releases/latest/download/proc-linux-x86_64.tar.gz | tar xz
curl -fsSL https://github.com/yazeed/proc-cli/releases/latest/download/proc-linux-aarch64.tar.gz | tar xz
for img in debian:bullseye-slim alpine; do
  docker run --rm --platform linux/amd64 -v "$PWD":/w $img /w/proc-linux-x86_64 --version
  docker run --rm --platform linux/arm64 -v "$PWD":/w $img /w/proc-linux-aarch64 --version
done
cd -

# The .deb and .rpm release files install in a container, run, and remove cleanly
# (apt and dnf install the local file; the man page and completions land where each distro looks)
VER=X.Y.Z   # the release just made
mkdir -p /tmp/proc-pkgs && cd /tmp/proc-pkgs
for f in proc-cli_${VER}_amd64.deb proc-cli_${VER}_arm64.deb proc-cli-${VER}-1.x86_64.rpm proc-cli-${VER}-1.aarch64.rpm; do
  curl -fsSLO https://github.com/yazeed/proc-cli/releases/download/v${VER}/$f
done
curl -fsSL https://github.com/yazeed/proc-cli/releases/download/v${VER}/checksums.txt | grep -E '\.(deb|rpm)$' | shasum -a 256 -c -
# (Ubuntu's image tells dpkg to skip /usr/share/man, which a normal install doesn't; rm undoes that)
for p in amd64 arm64; do
  for img in debian:bookworm debian:trixie ubuntu:24.04; do
    docker run --rm --platform linux/$p -v "$PWD":/p:ro $img sh -c "rm -f /etc/dpkg/dpkg.cfg.d/excludes && apt-get update -qq && apt-get install -y -qq /p/proc-cli_${VER}_$p.deb >/dev/null && proc --version && ls /usr/share/man/man1/proc.1.gz && apt-get remove -y -qq proc-cli >/dev/null && ! test -e /usr/bin/proc"
  done
done
docker run --rm --platform linux/amd64 -v "$PWD":/p:ro fedora:latest sh -c "dnf install -y -q /p/proc-cli-${VER}-1.x86_64.rpm && proc --version && ls /usr/share/man/man1/proc.1.gz && dnf remove -y -q proc-cli && ! test -e /usr/bin/proc"
docker run --rm --platform linux/arm64 -v "$PWD":/p:ro fedora:latest sh -c "dnf install -y -q /p/proc-cli-${VER}-1.aarch64.rpm && proc --version && ls /usr/share/man/man1/proc.1.gz && dnf remove -y -q proc-cli && ! test -e /usr/bin/proc"
cd -
# Afterwards: docker rmi debian:bookworm debian:trixie ubuntu:24.04 fedora:latest (the ones step 7 pulled), then the prunes in step 8

# Agent skill: skills.sh doesn't watch the repo, and there's no refresh command. It re-reads
# main when the skill is installed with telemetry on (no DISABLE_TELEMETRY / DO_NOT_TRACK):
# on 2026-10-27 one install updated its stored copy and page within minutes. An install on
# 2026-10-25, about an hour after its previous refresh, changed nothing; why is unknown.
mkdir -p /tmp/proc-skill-check && cd /tmp/proc-skill-check
bunx skills add yazeed/proc-cli --skill proc-cli -a claude-code -y   # installs into this folder only
cd - && rm -rf /tmp/proc-skill-check

# Is skills.sh's stored copy current? Prints "current" when its SKILL.md matches main's
# (trailing blank lines aside: its copy ends with an extra one, which read as "stale" after 2.2.0)
curl -s https://skills.sh/api/download/yazeed/proc-cli/proc-cli \
  | uv run python -c 'import json,sys; f={x["path"]:x["contents"] for x in json.load(sys.stdin)["files"]}; print("current" if f["SKILL.md"].rstrip()==open("skills/proc-cli/SKILL.md").read().rstrip() else "stale")'
```

Also verify:
- [ ] https://docs.rs/proc-cli — Rust docs updated
- [ ] https://proc-cli.com/install — the header pill and the install page show the new version (GitHub Pages rebuilds a minute or two after the push)
- [ ] `proc --version` — prints exactly `proc X.Y.Z` from every channel (no `-dev`, no commit), Nix included (the `nix run` line above)
- [ ] `flake.lock` — refresh it when it has gone stale or a build needs a newer nixpkgs (`nix flake update`, then `nix flake check -L`; without Nix installed, run both in the `nixos/nix` Docker image with `NIX_CONFIG="experimental-features = nix-command flakes"`). Commit it with the next change; the Nix jobs in CI check it on every push
- [ ] `proc manpage` — Includes new commands; `man proc-<command>` opens on the channels that install the pages
- [ ] `proc completions bash` — Includes new commands
- [ ] skills.sh is current: the check above prints `current` (the page at https://www.skills.sh/yazeed/proc-cli/proc-cli is cached for up to 5 minutes). If it's stale, run the install again the next day. If that doesn't help either, ask for a refresh with an issue on [vercel-labs/skills](https://github.com/vercel-labs/skills/issues), which runs skills.sh; many such requests there are unanswered. Agents installing the skill are unaffected either way: `skills add` reads the current SKILL.md from GitHub

### Reply to what the release fixed

Once every channel above checks out, answer each issue and PR the release's CHANGELOG section references (`#n`, noted in step 4):

- [ ] Rerun the reporter's steps on the released build and quote the result in the reply
- [ ] Say what changed, link the commits, and name the version and how to upgrade
- [ ] Close fixed issues as *completed* (`gh issue close <n> --reason completed --comment ...`). Fix commits reference issues as `(#n)`, not `Fixes #n`, so nothing closes before the release is out
- [ ] Thank the people who reported and contributed by name

## 8. Cleanup

```bash
cargo clean               # Remove build artifacts (~10GB+ after a release session)
docker builder prune -f   # Build cache from the Docker checks in steps 2 and 7
docker image prune -f     # Dangling images left by those builds
```

---

## Automated Publishing

On tag push, GitHub Actions handles:

1. **Build**: Cross-compile for all platforms. Each build then checks its binary: no `-dev` marker, and the targets the runner can execute must print exactly `proc <tag version>`. Every publish job below needs this job, so a mislabelled binary or a tag that doesn't match Cargo.toml publishes nothing. FreeBSD and NetBSD build in `build-bsd` instead, natively in their own VMs (`scripts/bsd-build.sh`), with the same checks; the GitHub Release needs it
2. **Linux packages**: `package-linux` builds `proc-cli_X.Y.Z_{amd64,arm64}.deb` and `proc-cli-X.Y.Z-1.{x86_64,aarch64}.rpm` from the two static Linux binaries with nfpm (`pkg/nfpm/`; the man page and completions come from running the x86_64 binary). The GitHub Release needs it
3. **GitHub Release**: Create release with binaries and the packages
4. **crates.io**: Publish with `--allow-dirty`
5. **npm**: Publish proc-cli package
6. **Homebrew**: Update yazeed/homebrew-proc tap
7. **Scoop**: Update yazeed/scoop-bucket-proc bucket
8. **Docker**: Push to yazeed/proc-cli on Docker Hub, and the same build to yazeed/proc (the image's original name). A separate workflow (`docker.yml`) with its own tag check: it stops if the tag isn't `v` + Cargo.toml's version
9. **apt repository**: `update-apt`, after `commit-package-files`, starts `pages.yml` on main's newest commit and checks that `/apt` lists the release; `pages.yml` deploys proc-cli.com with `/apt` rebuilt from the `.deb` files of the last three releases and signed with `APT_SIGNING_KEY` (`pkg/apt/build-repo.sh`)
10. **winget**: `update-winget` opens the pull request for the new version on microsoft/winget-pkgs from the `yazeed/winget-pkgs` fork, with `WINGET_TOKEN` (winget-releaser; the fork must keep that name), once a first version of `yazeed.proc-cli` is in winget-pkgs. Until then it moves the first version's open pull request to the new version (`pkg/winget/move-pr.sh`), and its manifests go to `pkg/winget/` on `main`
11. **COPR**: `update-copr` sends the version to the project's custom webhook (`COPR_WEBHOOK`). COPR runs the package's build script (`pkg/copr/copr-build.sh`, kept in COPR), which fetches `pkg/copr/sources.sh` and the spec at the tag and makes the source RPM: the spec at that version with a changelog entry, the tag's tarball and every crate `Cargo.lock` names. COPR then builds it for each chroot of `yazeed/proc-cli` (Fedora, EPEL 9 and 10, Amazon Linux 2023); the job follows the build on COPR's API and fails if it fails. Fedora's new releases join the project by themselves ("Follow Fedora branching"). To change the script COPR keeps, edit the file and run `copr-cli edit-package-custom proc-cli --script pkg/copr/copr-build.sh --script-builddeps "curl cargo tar xz"`; `pkg/copr/srpm.sh X.Y.Z` builds the same source RPM locally
12. **PyPI**: `pip install proc-cli` (also `pipx` and `uv tool`). `package-pypi` repacks the five release binaries into five wheels with `pkg/pypi/build-wheels.py` (standard library only; no maturin, no second Rust build): `manylinux_2_17`/`manylinux2014`/`musllinux_1_1` for Linux x86_64 and aarch64 (the static musl binaries), `macosx_10_12_x86_64` and `macosx_11_0_arm64` (each binary's own minimum macOS, read with `vtool -show-build`; check it again when the build target changes) and `win_amd64`, each holding the binary as the `proc` command. It checks the metadata with `twine check` and expects exactly five wheels. `test-pypi` installs the wheel with pip on its own OS (Ubuntu x86_64 and arm64, an Alpine container for musl, macOS arm64 and Intel, Windows) with `pkg/pypi/test-install.sh` and checks `proc --version`. `publish-pypi` runs for a tag, after the GitHub Release and every `test-pypi` job, and uploads the wheels with trusted publishing (environment `pypi`). The wheels are not release files. There is no TestPyPI run: its `proc-cli` belongs to an unrelated project
13. **nixpkgs**: `update-nixpkgs` moves `pkg/nixpkgs/package.nix` to the release with its hashes, builds it with its tests, and brings it to the nixpkgs pull request with `NIXPKGS_FORK_TOKEN` (see "After the tag: nixpkgs"). It does nothing once nixpkgs has the package
14. **FreeBSD port**: `update-freebsd` moves `sysutils/proc-cli` to the release in a FreeBSD VM with a ports tree, builds and checks it, and files the patch with FreeBSD's Bugzilla with `FREEBSD_BUGZILLA_API_KEY` (see "After the tag: FreeBSD port")
15. **pkgsrc**: `update-pkgsrc` moves `sysutils/proc-cli` to the release in a NetBSD VM, builds and checks it, and uploads the diff and the gnats form's fields; the report is filed by hand (see "After the tag: pkgsrc")
16. **Package files on main**: `commit-package-files` commits the files those three jobs wrote to `pkg/nixpkgs/`, `pkg/freebsd/` and `pkg/pkgsrc/` on `main`, and the winget manifests to `pkg/winget/` while the first version waits (`chore: package files for vX.Y.Z`), each only when its job passed; a dry run shows the change and commits nothing

The GitHub Release, and so every job after it, waits on `check-credentials`, which runs on every tag and dry run (step 1). Docker's workflow runs on its own and logs in itself.

Nix has nothing to publish: the flake lives in the repository, so `nix run github:yazeed/proc-cli/vX.Y.Z` works as soon as the tag is pushed, and CI's Nix jobs have already built and tested the flake on main. The nixpkgs package (`nix run nixpkgs#proc-cli`) is moved to each release by `update-nixpkgs` until it is merged, and by the nixpkgs update bot after (see "After the tag: nixpkgs").

nfpm is pinned in `release.yml` (`NFPM_VERSION`) with the SHA-256 of its Linux x86_64 tarball (`NFPM_SHA256`, from that release's `checksums.txt`). To update it, change both together, then rebuild the packages and run the install check in step 7.

Every third-party action in every workflow is pinned to a commit SHA, with the version in a comment: `release.yml` and `docker.yml` hold the publishing tokens, and the others build and test what they publish. The Dockerfile pins both base images to a digest. Dependabot (`.github/dependabot.yml`) opens weekly pull requests that move the pins: the minor and patch updates of the actions in one, each major update on its own, and the image digests. Merge a minor or patch one once its CI is green. A major update to an action `release.yml` uses also needs the release workflow's dry run on its branch (`gh workflow run release.yml --ref <its branch>`), since CI never runs the publishing jobs. `winget-releaser` is left out of Dependabot until the first automated winget update succeeds. To move a pin by hand, resolve the new tag's commit (`gh api repos/<owner>/<action>/commits/<tag> --jq .sha`) and change the SHA and the comment together.

## Required Secrets

Configure in GitHub repo settings → Secrets:

| Secret | Purpose |
|--------|---------|
| `HOMEBREW_TAP_TOKEN` | PAT for homebrew-proc repo |
| `SCOOP_BUCKET_TOKEN` | PAT for scoop-bucket-proc repo |
| `DOCKERHUB_USERNAME` | Docker Hub username |
| `DOCKERHUB_TOKEN` | Docker Hub access token |
| `APT_SIGNING_KEY` | The apt repository's signing key (`proc-cli apt repository`, fingerprint `19AD 1FFA 67E9 0A13 9579 7473 DC42 B7C2 51B0 33F3`), armored, no passphrase; used only for `/apt` |
| `WINGET_TOKEN` | Classic PAT with `public_repo` and `workflow`, for the winget pull requests |
| `COPR_WEBHOOK` | The custom webhook address of `yazeed/proc-cli`'s `proc-cli` package (COPR → the project's Integrations page); it does not expire |
| `NIXPKGS_FORK_TOKEN` | Token that can push to `yazeed/nixpkgs` and retitle its pull request on NixOS/nixpkgs, for `update-nixpkgs` |
| `FREEBSD_BUGZILLA_API_KEY` | API key of the bugs.freebsd.org account `me@yazeed.com` (Preferences → API Keys), for `update-freebsd` to file the port's patch |

The PPA's signing key is not a secret here: it stays on the maintainer's Mac (see "After the tag: Ubuntu PPA").

npm, crates.io and PyPI need no secret. All three publish through **Trusted Publishing (OIDC)**:
- **npm:** configured on npmjs.com under `proc-cli` → Settings → Trusted Publisher: GitHub Actions, `yazeed` / `proc-cli`, workflow `release.yml`, no environment, **Allow npm publish** checked. The `publish-npm` job needs `id-token: write` and npm 11.5.1 or newer.
- **crates.io:** configured under the `proc-cli` crate → Settings → Trusted Publishing: GitHub, `yazeed` / `proc-cli`, workflow `release.yml`, no environment.
- **PyPI:** configured on pypi.org under `proc-cli` → Publishing: GitHub, `yazeed` / `proc-cli`, workflow `release.yml`, environment `pypi`. Until the project exists, add it as a pending publisher (pypi.org → Your account → Publishing) with the same values; the first publish creates the project. The GitHub environment `pypi` (repository Settings → Environments) allows deployments from `v*` tags only. The `publish-pypi` job needs only `id-token: write`.

If the repository or the workflow file is renamed, all three trusted publishers must be recreated. A new npm trusted publisher expires unless a publish succeeds within 2 days, so add it shortly before tagging.

## Package Distribution

Grouped by system, as docs/install.md and the README list them. "Updates" says how an installed proc gets the next release. A channel that is not live yet is marked, and its install command is not in the user-facing docs.

| System | Channel | Package Name | Install Command | Updates |
|--------|---------|-------------|-----------------|---------|
| macOS | Homebrew | proc | `brew install yazeed/proc/proc` | `brew upgrade`; the release job updates the tap |
| Windows | Scoop | proc | `scoop bucket add proc https://github.com/yazeed/scoop-bucket-proc && scoop install proc` | `scoop update proc`; the release job updates the bucket |
| Windows | winget (not live yet) | yazeed.proc-cli | `winget install yazeed.proc-cli` | `update-winget`; the first version waits for a moderator: microsoft/winget-pkgs#469298. Not in install.md or the README until merged |
| Debian, Ubuntu | apt repository | proc-cli | see https://proc-cli.com/apt/ | `apt upgrade`; `update-apt` adds every release |
| Ubuntu | PPA | proc-cli | `sudo add-apt-repository ppa:yazeedaloyoun/proc-cli` then `sudo apt install proc-cli` | `apt upgrade`; one command on the maintainer's Mac per release (step 5) |
| Debian, Ubuntu | release file | proc-cli | `sudo apt install ./proc-cli_X.Y.Z_amd64.deb` | download each release |
| Fedora, RHEL and rebuilds, Amazon Linux | COPR | proc-cli | `sudo dnf copr enable yazeed/proc-cli && sudo dnf install proc-cli` | `dnf upgrade`; `update-copr` builds every release |
| Fedora, RHEL | release file | proc-cli | `sudo dnf install ./proc-cli-X.Y.Z-1.x86_64.rpm` | download each release |
| Arch | AUR (community-maintained) | proc-cli, proc-cli-bin | `yay -S proc-cli` (from source), `yay -S proc-cli-bin` (release binary) | when the maintainer publishes it |
| FreeBSD | ports (not live yet) | proc-cli | `pkg install proc-cli` | step 5; waits for a committer: bug 300492. Not in install.md or the README until it is in the tree; install.md says it is submitted |
| FreeBSD, NetBSD | release binaries | proc-freebsd-x86_64, proc-netbsd-x86_64 | `install.sh`, `cargo binstall proc-cli`, `npm install -g proc-cli` | run again |
| NetBSD | pkgsrc | sysutils/proc-cli | from a pkgsrc tree; `pkgin` once pkgsrc's binary packages include it | the maintainer sends each release as a gnats diff (step 5) |
| Any | crates.io | proc-cli | `cargo install proc-cli` | run again |
| Any | npm | proc-cli | `npm install -g proc-cli` | `npm update -g proc-cli` |
| Any | PyPI | proc-cli | `pip install proc-cli`, `pipx install proc-cli`, `uv tool install proc-cli` | `pip install --upgrade proc-cli`; `publish-pypi`, first published with 2.3.0 |
| Any | Nix (flake in this repo) | proc-cli | `nix profile install github:yazeed/proc-cli` | no |
| Any | nixpkgs (not live yet) | proc-cli | `nix run nixpkgs#proc-cli` | step 5; waits to be merged: NixOS/nixpkgs#581248. Not in install.md or the README until merged |
| Any | Docker | yazeed/proc-cli (also yazeed/proc) | `docker run yazeed/proc-cli` | `docker pull yazeed/proc-cli` |
| Any | install script, release files | | `install.sh`, the GitHub release | run again, download each release |
