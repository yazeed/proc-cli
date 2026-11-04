---
layout: page
title: Install
description: "Install proc on macOS, Linux, Windows, FreeBSD or NetBSD, with Homebrew, Scoop, apt, dnf, npm, cargo, pip and more. One binary, no dependencies."
permalink: /install
---

The latest version is **{{ site.data.release.version }}** ([what's new](changelog)). Every method below installs the same single binary, called `proc`. Each entry says who keeps it current: some channels update with your system's package manager, some take a command again.

## macOS

**Homebrew.** The command adds the `yazeed/proc` tap for you.

```bash
brew install yazeed/proc/proc
```

Each release updates the tap. Update with `brew upgrade`.

**Install script**, which downloads the release binary for your Mac:

```bash
curl -fsSL https://raw.githubusercontent.com/yazeed/proc-cli/main/install.sh | bash
```

Does not update itself. Run it again for a newer version.

## Windows

**Scoop.**

```bash
scoop bucket add proc https://github.com/yazeed/scoop-bucket-proc
scoop install proc
```

Each release updates the bucket. Update with `scoop update proc`.

## Linux

### Debian, Ubuntu and derivatives

**proc's apt repository**, signed, for amd64 and ARM64. [The apt page](apt/) has the setup commands. Every release lands there automatically, so `apt upgrade` keeps proc current.

```bash
sudo apt install proc-cli
```

Run it after the setup on that page.

**Ubuntu PPA**, for Ubuntu 22.04, 24.04, 26.04 and 26.10 on amd64 and arm64:

```bash
sudo add-apt-repository ppa:yazeedaloyoun/proc-cli
sudo apt install proc-cli
```

Launchpad builds each release from source, and `apt upgrade` updates it.

**The `.deb` files** on the [releases page](https://github.com/yazeed/proc-cli/releases/latest), for amd64 and ARM64. The file names carry the version, so put the version you downloaded in the command:

```bash
# ARM64: proc-cli_X.Y.Z_arm64.deb
sudo apt install ./proc-cli_X.Y.Z_amd64.deb
```

A file is not a repository, so `apt upgrade` never updates it: download the new file for each release. The files are not signed.

### Fedora, RHEL and its rebuilds, Amazon Linux

**COPR**, for Fedora 43, 44, 45 and rawhide, EPEL 9 and 10 (RHEL, AlmaLinux, Rocky) and Amazon Linux 2023, on x86_64 and aarch64:

```bash
sudo dnf copr enable yazeed/proc-cli
sudo dnf install proc-cli
```

Every release builds there automatically, and `dnf upgrade` updates it.

**The `.rpm` files** on the [releases page](https://github.com/yazeed/proc-cli/releases/latest), for x86_64 and aarch64:

```bash
# aarch64: proc-cli-X.Y.Z-1.aarch64.rpm
sudo dnf install ./proc-cli-X.Y.Z-1.x86_64.rpm
```

`dnf upgrade` never updates a file: download the new one for each release. The files are not signed, so `dnf` prints a warning that it skipped the OpenPGP check.

### Arch Linux

**AUR**, packaged by Rafael Dominiquini, not by this project. `proc-cli-bin` installs the release binary, `proc-cli` builds from source:

```bash
yay -S proc-cli-bin
yay -S proc-cli
```

It follows a release once the maintainer publishes it, which can take a few days. Check `proc --version` after installing.

### Any distribution

**Install script.** It works on any distribution, including Alpine and older releases such as Debian 10 and Ubuntu 20.04, because the binaries are static:

```bash
curl -fsSL https://raw.githubusercontent.com/yazeed/proc-cli/main/install.sh | bash
```

Does not update itself. Run it again for a newer version.

**Homebrew on Linux** works too: `brew install yazeed/proc/proc`, updated with `brew upgrade`.

## FreeBSD and NetBSD

**Install script.** It downloads the release binary (`proc-freebsd-x86_64` or `proc-netbsd-x86_64`) for x86_64. It needs `curl`, which is a package on both (`pkg install curl` on FreeBSD, `pkg_add curl` on NetBSD):

```bash
curl -fsSL https://raw.githubusercontent.com/yazeed/proc-cli/main/install.sh | sh
```

Does not update itself. Run it again for a newer version. `cargo binstall proc-cli` and `npm install -g proc-cli` download the same binaries. On other CPUs, build it with `cargo install proc-cli`.

**pkgsrc-wip**, for NetBSD users of pkgsrc-wip: the package is `wip/proc-cli`. The maintainer moves it to each release.

There is no FreeBSD port yet. It is submitted and waits for review.

proc reads ports with `sockstat` and open files with `fstat`, both in the base system, so it needs nothing else installed. Once a directory is deleted, the kernel forgets that it was a process's working directory, so `proc in` a deleted directory exits 4 here, where Linux and macOS still find the processes in it.

## Any system, with a language package manager

**npm, yarn, pnpm or bun.** The package downloads the binary for your platform the first time you run `proc`, and checks its SHA-256 checksum.

```bash
npm install -g proc-cli
yarn global add proc-cli
pnpm add -g proc-cli
bun install -g proc-cli
```

Update with `npm update -g proc-cli` (or your tool's equivalent).

**Cargo**, from source with [Rust](https://rustup.rs), or as a pre-built binary with [cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

```bash
cargo install proc-cli
cargo binstall proc-cli
```

Run the same command again to update.

**PyPI**, new in 2.3.0. The wheel holds the release binary, so no Python code runs. Wheels exist for macOS (Apple silicon and Intel), Linux (x86_64 and ARM64, glibc and musl) and Windows (x86_64):

```bash
pip install proc-cli
pipx install proc-cli
uv tool install proc-cli
```

Update with `pip install --upgrade proc-cli`, `pipx upgrade proc-cli` or `uv tool upgrade proc-cli`.

## Nix

The flake in this repository. The nixpkgs package is not merged yet, so `nixpkgs#proc-cli` does not exist.

```bash
# install the latest release, with the man page and shell completions
nix profile install github:yazeed/proc-cli/v{{ site.data.release.version }}
# or run it once without installing
nix run github:yazeed/proc-cli/v{{ site.data.release.version }} -- on :3000
```

Does not update itself. Without `/v{{ site.data.release.version }}` the flake builds the main branch, which can be ahead of the latest release.

## Docker

`--pid=host` lets proc see the host's processes, and `--net=host` its ports (for `on :port`, `ports` and `free`). The image is also published as `yazeed/proc`.

```bash
docker run --rm -it --pid=host --net=host yazeed/proc-cli
```

Update with `docker pull yazeed/proc-cli`.

## Release files

Every release on the [releases page](https://github.com/yazeed/proc-cli/releases/latest) has the binaries for macOS, Linux, Windows, FreeBSD and NetBSD, the `.deb` and `.rpm` files, and `checksums.txt`. Download a file and put the binary on your `PATH`. Nothing updates it.

## Check it works

```bash
proc --version
proc ports
```

## Teach your coding agent

The [agent skill](skill) teaches Claude Code, Codex, Cursor, Gemini CLI and others when to reach for proc:

```bash
npx skills add yazeed/proc-cli
```

## Shell completions

The Nix package, the `.deb` and `.rpm` files, the apt repository (which holds the same `.deb` files), the Ubuntu PPA and COPR install the completions and the man page. With any other install method, add them yourself:

```bash
proc completions bash > /etc/bash_completion.d/proc          # bash
proc completions zsh > ~/.zsh/completions/_proc               # zsh
proc completions fish > ~/.config/fish/completions/proc.fish  # fish
```

## Man page

```bash
proc manpage | sudo tee /usr/local/share/man/man1/proc.1 > /dev/null
man proc
```
