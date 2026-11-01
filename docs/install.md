---
layout: page
title: Install
description: "Install proc on macOS, Linux, Windows, FreeBSD or NetBSD. One binary, no dependencies."
permalink: /install
---

The latest version is **{{ site.data.release.version }}** ([what's new](changelog)). Every method below installs the same single binary, called `proc`.

## macOS

With [Homebrew](https://brew.sh). The command adds the `yazeed/proc` tap for you.

```bash
brew install yazeed/proc/proc
```

Or with the install script, which downloads the release binary for your Mac:

```bash
curl -fsSL https://raw.githubusercontent.com/yazeed/proc-cli/main/install.sh | bash
```

## Linux

The install script works on any distribution, including Alpine and older releases such as Debian 10 and Ubuntu 20.04, because the binaries are static:

```bash
curl -fsSL https://raw.githubusercontent.com/yazeed/proc-cli/main/install.sh | bash
```

Homebrew on Linux works too: `brew install yazeed/proc/proc`.

### Debian, Ubuntu, Fedora, RHEL

Each release has `.deb` and `.rpm` files for x86_64 (amd64) and ARM64 on the [releases page](https://github.com/yazeed/proc-cli/releases/latest). They install `proc`, its man page and its bash, zsh and fish completions, and need no other package. The file names carry the version, so put the version you downloaded in the command:

```bash
# Debian, Ubuntu (amd64; the ARM64 file ends in _arm64.deb)
sudo apt install ./proc-cli_X.Y.Z_amd64.deb

# Fedora, RHEL and others that use dnf (x86_64; the ARM64 file ends in .aarch64.rpm)
sudo dnf install ./proc-cli-X.Y.Z-1.x86_64.rpm
```

These are files on the release page, not an apt or dnf repository, so `apt upgrade` and `dnf upgrade` never update them: download the new file for each release. `sudo apt remove proc-cli` or `sudo dnf remove proc-cli` removes proc. The files are not signed, so `dnf` prints a warning that it skipped the OpenPGP check.

## Windows

With [Scoop](https://scoop.sh):

```bash
scoop bucket add proc https://github.com/yazeed/scoop-bucket-proc
scoop install proc
```

## FreeBSD and NetBSD

The install script downloads the release binary for x86_64. It needs `curl`, which is a package on both (`pkg install curl` on FreeBSD, `pkg_add curl` on NetBSD):

```bash
curl -fsSL https://raw.githubusercontent.com/yazeed/proc-cli/main/install.sh | sh
```

On other CPUs, build it with `cargo install proc-cli`.

proc reads ports with `sockstat` and open files with `fstat`, both in the base system, so it needs nothing else installed. Once a directory is deleted, the kernel forgets that it was a process's working directory, so `proc in` a deleted directory exits 4 here, where Linux and macOS still find the processes in it.

## Any platform

### npm or bun

The package downloads the binary for your platform the first time you run `proc`, and checks its SHA-256 checksum.

```bash
npm install -g proc-cli
bun install -g proc-cli
```

### Cargo

From source with [Rust](https://rustup.rs), or as a pre-built binary with [cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

```bash
cargo install proc-cli
cargo binstall proc-cli
```

### Nix

```bash
nix profile install github:yazeed/proc-cli    # with the man page and shell completions
nix run github:yazeed/proc-cli -- on :3000    # or run it once without installing
```

### Docker

`--pid=host` lets proc see the host's processes, and `--net=host` its ports (for `on :port`, `ports` and `free`).

```bash
docker run --rm -it --pid=host --net=host yazeed/proc-cli
```

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

The Nix package and the `.deb` and `.rpm` files install the completions and the man page. With any other install method, add them yourself:

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
