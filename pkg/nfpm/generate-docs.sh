#!/usr/bin/env bash
# Writes the man page and shell completions that proc prints, in the file names
# nfpm.yaml expects. Run once with a binary that runs on this machine; the
# output is the same text for every architecture.
#
# usage: generate-docs.sh <proc binary> <output dir>
set -euo pipefail

bin=$1
out=$2

mkdir -p "$out"
"$bin" manpage | gzip -9n > "$out/proc.1.gz"
"$bin" completions bash > "$out/proc.bash"
"$bin" completions zsh > "$out/_proc"
"$bin" completions fish > "$out/proc.fish"
