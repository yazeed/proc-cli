#!/bin/sh
# Builds proc's release binary inside a FreeBSD or NetBSD VM, checks it, and
# copies it into the current directory as <name>. ci.yml and release.yml run
# it in the vmactions VMs: cross's FreeBSD and NetBSD images lack libraries
# proc links (libgeom; libkvm and libexecinfo), so the BSDs build natively.
#
# usage: bsd-build.sh <name> [<expected --version line>]
#   name      the release file name, e.g. proc-freebsd-x86_64
#   expected  e.g. "proc 2.2.0"; release.yml passes the tag's version
#
# Set CARGO_TARGET_DIR outside the workspace: vmactions copies the workspace
# back to the runner, and a target directory there would be gigabytes.
set -eu

name=$1
want=${2:-}

cargo build --release
bin="${CARGO_TARGET_DIR:-target}/release/proc"

# For a release, the checks build-release makes on the other platforms
if [ -n "$want" ] && grep -a -q -- '-dev (' "$bin"; then
  echo "::error::$name is labelled as a dev build; the tag must be v<Cargo.toml version>"
  exit 1
fi
got=$("$bin" --version | head -n 1)
echo "$got"
if [ -n "$want" ] && [ "$got" != "$want" ]; then
  echo "::error::$name --version printed '$got', expected '$want'"
  exit 1
fi

# The port table comes from sockstat here; this reads it once
"$bin" ports --json

cp "$bin" "$name"
