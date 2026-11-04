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
- [ ] The release workflow's dry run is green: `gh workflow run release.yml --ref main`, then `gh run watch`. It runs every build and package job (the BSD builds and the `.deb`/`.rpm` files run only in this workflow's form here) and assembles the release files with `checksums.txt`, and publishes nothing: the GitHub Release step and the crates.io, npm, PyPI, Homebrew and Scoop jobs run only for a tag. The PyPI wheels are built and installed on every OS in the dry run (`package-pypi`, `test-pypi`); only `publish-pypi` waits for a tag. Its version checks print instead of failing, since a branch build is a `-dev` build; the tag checks them
- [ ] The dry run's "Check credentials" job passed and printed no warning. It tries every secret under "Required Secrets" and reads when each expires (GitHub reports a token's expiry, so no date is kept anywhere): one that is invalid or expired fails the job, and the tag's release then publishes nothing; one that expires within 30 days is a warning. Renew it now, set it with `gh secret set <NAME>`, and run the dry run again
- [ ] The credentials outside CI still work: the PPA key on the maintainer's Mac (`gpg -K 227B9E7E7C22B6E88774325184D7BBF5E38CCC4D` lists it; it has no expiry date, and if one is ever set it shows as `[expires: ...]`) and, only if the COPR package's settings change in this release, the COPR API token (`grep expiration ~/.config/copr`; a new one comes from https://copr.fedorainfracloud.org/api/). A release itself needs neither token: COPR builds from the webhook

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

Build and run the Docker image from the release tree too. The release workflow builds it the same way, from a context without `.git` (so it prints the plain version):

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

`pkg/aur/PKGBUILD` is not bumped. Both AUR packages are community packages maintained by Rafael Dominiquini: `proc-cli` (built from source) and `proc-cli-bin` (the release binaries); he moved them from `proc` and `proc-bin`, which are orphaned at 2.1.1, with 2.2.0. The project has no AUR account yet, so the README points Arch users to his packages, and the PKGBUILD stays in the repo for the day the project can co-maintain them (see "After the tag: AUR" below).

## 4. Documentation

Update and verify all docs are complete:

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
- [ ] Verify `proc manpage` output includes new commands
- [ ] Verify `proc completions` includes new commands

## 5. Commit & Tag

Stage the release files by path, not with `git add -A`: check `git status` first, because uncommitted work that isn't part of the release must stay out of it.

```bash
git status --short                       # only the release's files should be listed
git add Cargo.toml Cargo.lock CHANGELOG.md docs/changelog.md ...   # the files from steps 3 and 4
git commit -m "chore: release vX.Y.Z"
git tag vX.Y.Z
git push origin main vX.Y.Z              # main and this one tag (not --tags, which pushes every local tag)
```

The tag must be `v` + Cargo.toml's version: both the release and Docker workflows stop before publishing anything otherwise. A repository ruleset stops `v*` tags from being moved or deleted once pushed, and `main` from being force-pushed or deleted. If a tag must be redone because nothing was published from it, switch the tag ruleset off (Settings → Rules → Rulesets) for that one fix, then back on.

### After the tag: AUR

Nothing to publish: the AUR's `proc-cli` and `proc-cli-bin` are Rafael Dominiquini's. Step 7 checks that they caught up with the release. Once the project has an AUR account and co-maintains them, this step goes back to updating the AUR repositories with a PKGBUILD whose `sha256sums` is the tag tarball's checksum (`curl -fsSL https://github.com/yazeed/proc-cli/archive/vX.Y.Z.tar.gz | shasum -a 256`) and a regenerated `.SRCINFO` (`makepkg --printsrcinfo > .SRCINFO` on Arch).

### After the tag: nixpkgs

nixpkgs builds proc from the tag, with its tests, as `proc-cli`. `pkg/nixpkgs/package.nix` is the file nixpkgs carries at `pkgs/by-name/pr/proc-cli/package.nix`, and every release moves it to the new version:

1. Set `version` in `pkg/nixpkgs/package.nix` and both `hash` and `cargoHash` to `lib.fakeHash`. Build it in the `nixos/nix` image (`pkg/nixpkgs/build.nix` builds it against the nixpkgs in `flake.lock`, with the maintainer entry nixpkgs may not have yet); the first build fails with the source hash it got, the second with the cargo hash. Put each in, then build once more: it runs the whole test suite and must pass.

   ```bash
   docker run --rm --cpus 4 -v "$PWD":/work -w /work -e NIX_CONFIG="experimental-features = nix-command flakes" \
     nixos/nix sh -c 'nix-build pkg/nixpkgs/build.nix && ./result/bin/proc --version'   # prints proc X.Y.Z
   ```

2. Commit `pkg/nixpkgs/package.nix` to `main`.
3. Bring it to nixpkgs, after the maintainer has seen the commit and the pull request text. Until the package is merged, the pull request on NixOS/nixpkgs (from the maintainer's fork) carries two commits, `maintainers: add yazeed` and `proc-cli: init at X.Y.Z`; each release updates it to the new version. Once it is merged, nixpkgs' update bot opens `proc-cli: A -> B` after a release: check its hashes match step 1 and its build passes, and approve it as the package's maintainer. nixpkgs asks commits written with an AI assistant to say so with an `Assisted-by:` trailer.

### After the tag: Ubuntu PPA

`ppa:yazeedaloyoun/proc-cli` builds proc for Ubuntu 22.04, 24.04, 26.04 and 26.10 (amd64 and arm64) from source packages signed with the maintainer's key. The key stays on the maintainer's Mac and is never given to CI, so this step runs there, as one command, once the tag is pushed:

```bash
pkg/ppa/release.sh X.Y.Z      # builds the source packages in Docker, signs and uploads them
```

It builds in an `ubuntu:26.04` container (`pkg/ppa/build-source.sh`), signs with the maintainer's key (gpg asks for its passphrase unless the agent or the macOS keychain has it) and uploads by FTP (`pkg/ppa/upload.sh`). The first release's upload carries the two tarballs; the script then waits for Launchpad to publish them, usually within the hour, and sends the other three. It ends once Launchpad has accepted all four, and fails, naming the version, if one is not accepted within an hour (Launchpad emails why). Launchpad's FTP times out often: uploads retry, and running the command again sends only what has not gone. It stops before building if Docker is not running or the tag has no GitHub release, and a build that stops halfway is never reused. A packaging fix to a released version is `pkg/ppa/release.sh X.Y.Z 2`. A new Ubuntu release joins the list in `build-source.sh`; check its Rust is at least proc's `rust-version`. Step 7 checks the builds.

### After the tag: FreeBSD port

`sysutils/proc-cli` (`pkg/freebsd/`) was submitted as [bug 300492](https://bugs.freebsd.org/bugzilla/show_bug.cgi?id=300492) and waits for a ports committer. Every release moves it to the new version:

1. In a FreeBSD VM with a ports tree (`git clone https://git.FreeBSD.org/ports.git`), copy `pkg/freebsd/*` to `sysutils/proc-cli/`, set `DISTVERSION`, then `make makesum`, `make cargo-crates > Makefile.crates`, and `make makesum` again (the second picks up the crates). Build and check it: `make stage check-plist stage-qa package` and `portlint -AC` must pass, and the installed package's `proc --version` prints the new version.
2. Copy the changed files back to `pkg/freebsd/` and commit them to `main`.
3. Make the patch (`git format-patch` in the ports tree). Until bug 300492 is committed, attach it there and mark the old patch obsolete; once the port is in, each release is a new bug in Ports & Packages → Individual Port(s), `[PATCH] sysutils/proc-cli: Update to X.Y.Z`. Either way the maintainer files it from the Bugzilla account `me@yazeed.com` (the port's `MAINTAINER`), ticks "patch" (Bugzilla reads `git format-patch` output as a mailbox otherwise) and sets `maintainer-approval` to `+`.

To automate once the port is in: a workflow job that runs step 1 in a FreeBSD VM, as `build-bsd` does, and uploads the patch, leaving only the filing.

### After the tag: pkgsrc

`sysutils/proc-cli` (`pkg/pkgsrc/`) is in pkgsrc-wip as `wip/proc-cli` (account `yazeed`, key `~/.ssh/pkgsrc-wip` on the maintainer's Mac). An import into pkgsrc itself is requested (tech-pkg, and Thomas Klausner, who created the wip account). Every release moves wip to the new version:

1. In a NetBSD VM with pkgsrc and pkgsrc-wip checked out, copy `pkg/pkgsrc/*` to `wip/proc-cli/`, set `DISTNAME`, then `make print-cargo-depends > cargo-depends.mk` and `make makesum`. `make package`, the installed `proc --version` and `pkglint` must pass. Build against a quarterly branch if trunk needs a newer system library than the NetBSD release has (2.2.0: trunk wanted libunwind >= 22.1).
2. Copy the changed files back to `pkg/pkgsrc/` and commit them to `main`; update `COMMIT_MSG` to the version.
3. Commit and push to pkgsrc-wip from the maintainer's Mac with that key (wip asks a human to review what is pushed).

Once the package is in pkgsrc itself, updates go to its committer instead (send-pr, or mail to whoever imported it). To automate wip: a CI-only key that Thomas adds to the wip account, and a job that does steps 1 and 3.

## 6. Verify CI

After tag push, monitor GitHub Actions:
- [ ] Build succeeds for all platforms, including each build's "Check the binary reports the tag's version" step
- [ ] Both "Build freebsd/netbsd" jobs pass: each BSD binary is built in its own OS and prints the tag's version there (the GitHub Release needs them)
- [ ] Docker's "Check the tag matches Cargo.toml" step passes
- [ ] GitHub Release created with binaries (`proc-freebsd-x86_64` and `proc-netbsd-x86_64` among them, each with its `.tar.gz`), the four `.deb`/`.rpm` packages and `checksums.txt` (the `package-linux` job passed)
- [ ] crates.io published
- [ ] npm published
- [ ] Homebrew tap updated
- [ ] Scoop bucket updated
- [ ] Docker image pushed
- [ ] "Update the apt repository" passed: https://proc-cli.com/apt/dists/stable/main/binary-amd64/Packages lists the new version
- [ ] "Update winget" opened the pull request for the new version on microsoft/winget-pkgs (until the first version is merged it skips, with a notice)
- [ ] The PPA's builds for the new version succeeded on every release and both architectures (https://launchpad.net/~yazeedaloyoun/+archive/ubuntu/proc-cli/+packages)
- [ ] "Update COPR" passed: the build for the new version succeeded on every chroot (https://copr.fedorainfracloud.org/coprs/yazeed/proc-cli/builds/)
- [ ] The "Install the PyPI wheel" jobs passed (Linux x86_64 and aarch64, Alpine, macOS arm64 and x86_64, Windows)
- [ ] PyPI published: https://pypi.org/project/proc-cli/ shows the new version
- [ ] The after-the-tag steps are done: nixpkgs, the Ubuntu PPA, the FreeBSD port's patch filed, pkgsrc-wip pushed (step 5)

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

# AUR (Rafael Dominiquini's packages): both should reach the new version within a few days
curl -s 'https://aur.archlinux.org/rpc/v5/info?arg[]=proc-cli&arg[]=proc-cli-bin' | jq -r '.results[] | "\(.Name) \(.Version)"'

# FreeBSD port: the bug (300492 until the port is in, then the update bug) has the new patch;
# once committed, https://www.freshports.org/sysutils/proc-cli/ shows the new version
# pkgsrc: wip/proc-cli is at the new version (its commits are mirrored on GitHub)
gh api 'repos/NetBSD/pkgsrc-wip/commits?path=proc-cli&per_page=1' -q '.[0].commit.message'

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
- [ ] `proc manpage` — Includes new commands
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
9. **apt repository**: `update-apt` runs `pages.yml`, which deploys proc-cli.com with `/apt` rebuilt from the `.deb` files of the last three releases and signed with `APT_SIGNING_KEY` (`pkg/apt/build-repo.sh`)
10. **winget**: `update-winget` opens the pull request for the new version on microsoft/winget-pkgs from the `yazeed/winget-pkgs` fork, with `WINGET_TOKEN` (winget-releaser; the fork must keep that name). It skips, with a notice, until a first version of `yazeed.proc-cli` is in winget-pkgs
11. **COPR**: `update-copr` sends the version to the project's custom webhook (`COPR_WEBHOOK`). COPR runs the package's build script (`pkg/copr/copr-build.sh`, kept in COPR), which fetches `pkg/copr/sources.sh` and the spec at the tag and makes the source RPM: the spec at that version with a changelog entry, the tag's tarball and every crate `Cargo.lock` names. COPR then builds it for each chroot of `yazeed/proc-cli` (Fedora, EPEL 9 and 10, Amazon Linux 2023); the job follows the build on COPR's API and fails if it fails. Fedora's new releases join the project by themselves ("Follow Fedora branching"). To change the script COPR keeps, edit the file and run `copr-cli edit-package-custom proc-cli --script pkg/copr/copr-build.sh --script-builddeps "curl cargo tar xz"`; `pkg/copr/srpm.sh X.Y.Z` builds the same source RPM locally
12. **PyPI**: `pip install proc-cli` (also `pipx` and `uv tool`). `package-pypi` repacks the five release binaries into five wheels with `pkg/pypi/build-wheels.py` (standard library only; no maturin, no second Rust build): `manylinux_2_17`/`manylinux2014`/`musllinux_1_1` for Linux x86_64 and aarch64 (the static musl binaries), `macosx_10_12_x86_64` and `macosx_11_0_arm64` (each binary's own minimum macOS, read with `vtool -show-build`; check it again when the build target changes) and `win_amd64`, each holding the binary as the `proc` command. It checks the metadata with `twine check` and expects exactly five wheels. `test-pypi` installs the wheel with pip on its own OS (Ubuntu x86_64 and arm64, an Alpine container for musl, macOS arm64 and Intel, Windows) with `pkg/pypi/test-install.sh` and checks `proc --version`. `publish-pypi` runs for a tag, after the GitHub Release and every `test-pypi` job, and uploads the wheels with trusted publishing (environment `pypi`). The wheels are not release files. There is no TestPyPI run: its `proc-cli` belongs to an unrelated project

The GitHub Release, and so every job after it, waits on `check-credentials`, which runs on every tag and dry run (step 1). Docker's workflow runs on its own and logs in itself.

Nix has nothing to publish: the flake lives in the repository, so `nix run github:yazeed/proc-cli/vX.Y.Z` works as soon as the tag is pushed, and CI's Nix jobs have already built and tested the flake on main. The nixpkgs package (`nix run nixpkgs#proc-cli`) is moved to each release by hand until it is merged, and by the nixpkgs update bot after (see "After the tag: nixpkgs").

nfpm is pinned in `release.yml` (`NFPM_VERSION`) with the SHA-256 of its Linux x86_64 tarball (`NFPM_SHA256`, from that release's `checksums.txt`). To update it, change both together, then rebuild the packages and run the install check in step 7.

Third-party actions in `release.yml` and `docker.yml` are pinned to commit SHAs, with the version in a comment, because those jobs hold the publishing tokens. To update one, resolve the new tag's commit (`gh api repos/<owner>/<action>/commits/<tag> --jq .sha`) and change the SHA and the comment together.

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

The PPA's signing key is not a secret here: it stays on the maintainer's Mac (see "After the tag: Ubuntu PPA").

npm, crates.io and PyPI need no secret. All three publish through **Trusted Publishing (OIDC)**:
- **npm:** configured on npmjs.com under `proc-cli` → Settings → Trusted Publisher: GitHub Actions, `yazeed` / `proc-cli`, workflow `release.yml`, no environment, **Allow npm publish** checked. The `publish-npm` job needs `id-token: write` and npm 11.5.1 or newer.
- **crates.io:** configured under the `proc-cli` crate → Settings → Trusted Publishing: GitHub, `yazeed` / `proc-cli`, workflow `release.yml`, no environment.
- **PyPI:** configured on pypi.org under `proc-cli` → Publishing: GitHub, `yazeed` / `proc-cli`, workflow `release.yml`, environment `pypi`. Until the project exists, add it as a pending publisher (pypi.org → Your account → Publishing) with the same values; the first publish creates the project. The GitHub environment `pypi` (repository Settings → Environments) allows deployments from `v*` tags only. The `publish-pypi` job needs only `id-token: write`.

If the repository or the workflow file is renamed, all three trusted publishers must be recreated. A new npm trusted publisher expires unless a publish succeeds within 2 days, so add it shortly before tagging.

## Package Distribution

| Channel | Package Name | Install Command |
|---------|-------------|-----------------|
| crates.io | proc-cli | `cargo install proc-cli` |
| npm | proc-cli | `npm install -g proc-cli` |
| Homebrew | proc | `brew install yazeed/proc/proc` |
| Scoop | proc | `scoop bucket add proc https://github.com/yazeed/scoop-bucket-proc && scoop install proc` |
| Docker | yazeed/proc-cli (also yazeed/proc) | `docker run yazeed/proc-cli` |
| Debian, Ubuntu (release file) | proc-cli | `sudo apt install ./proc-cli_X.Y.Z_amd64.deb` |
| Fedora, RHEL (release file) | proc-cli | `sudo dnf install ./proc-cli-X.Y.Z-1.x86_64.rpm` |
| Nix (flake in this repo) | proc-cli | `nix profile install github:yazeed/proc-cli` |
| AUR (community, Rafael Dominiquini) | proc-cli, proc-cli-bin | `yay -S proc-cli` (from source), `yay -S proc-cli-bin` (release binary) |
| Debian, Ubuntu (apt repository) | proc-cli | see https://proc-cli.com/apt/ (`update-apt`, every release) |
| Ubuntu PPA | proc-cli | `sudo add-apt-repository ppa:yazeedaloyoun/proc-cli` (one command on the maintainer's Mac, step 5) |
| Fedora, EPEL, Amazon Linux (COPR) | proc-cli | `sudo dnf copr enable yazeed/proc-cli && sudo dnf install proc-cli` (`update-copr`, every release) |
| PyPI | proc-cli | `pip install proc-cli`, `pipx install proc-cli`, `uv tool install proc-cli` (`publish-pypi`; first published with 2.3.0) |
| winget | yazeed.proc-cli | `winget install yazeed.proc-cli` (`update-winget`; the first version waits for a moderator: microsoft/winget-pkgs#469298) |
| nixpkgs | proc-cli | `nix run nixpkgs#proc-cli` (step 5; waits to be merged: NixOS/nixpkgs#581248) |
| FreeBSD ports | proc-cli | `pkg install proc-cli` (step 5; waits for a committer: bug 300492) |
| pkgsrc (NetBSD) | proc-cli | from pkgsrc-wip now (`wip/proc-cli`); `pkgin install proc-cli` once imported (step 5) |
