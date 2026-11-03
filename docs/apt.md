---
layout: page
title: apt repository
description: "Install proc on Debian and Ubuntu from its signed apt repository, so apt upgrade keeps it current."
permalink: /apt/
---

This is proc's apt repository, for Debian, Ubuntu and their derivatives on x86_64 (amd64) and ARM64. It holds the same `.deb` files as the [releases page](https://github.com/yazeed/proc-cli/releases/latest), and each release adds itself, so `apt upgrade` keeps proc current.

## Add it

```bash
sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://proc-cli.com/apt/proc-cli.gpg | sudo tee /etc/apt/keyrings/proc-cli.gpg > /dev/null
sudo tee /etc/apt/sources.list.d/proc-cli.sources > /dev/null <<'EOF'
Types: deb
URIs: https://proc-cli.com/apt
Suites: stable
Components: main
Signed-By: /etc/apt/keyrings/proc-cli.gpg
EOF
sudo apt update
sudo apt install proc-cli
```

The package installs `proc`, its man page and its bash, zsh and fish completions.

## The signing key

The repository is signed with this key, which the first command above saves. Its fingerprint:

```
19AD 1FFA 67E9 0A13 9579  7473 DC42 B7C2 51B0 33F3
```

Check it with `gpg --show-keys /etc/apt/keyrings/proc-cli.gpg`. The same key is at [proc-cli.asc](proc-cli.asc) as text.

## Remove it

```bash
sudo apt remove proc-cli
sudo rm /etc/apt/sources.list.d/proc-cli.sources /etc/apt/keyrings/proc-cli.gpg
```

For every other way to install proc, see [Install](../install).
