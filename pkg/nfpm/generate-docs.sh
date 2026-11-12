#!/usr/bin/env bash
# Writes the man pages and shell completions that proc prints, in the file names
# nfpm.yaml expects. Run once with a binary that runs on this machine; the
# output is the same text for every architecture.
#
# usage: generate-docs.sh <proc binary> <output dir>
set -euo pipefail

bin=$1
out=$2

mkdir -p "$out"
# proc.1 and a page per command (proc-kill.1, ...)
mkdir -p "$out/man"
"$bin" manpage --dir "$out/man"
gzip -9n "$out"/man/*.1
"$bin" completions bash > "$out/proc.bash"
"$bin" completions zsh > "$out/_proc"
"$bin" completions fish > "$out/proc.fish"
