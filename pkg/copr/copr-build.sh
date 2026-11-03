#!/bin/sh
# COPR's build script for yazeed/proc-cli (the package's "custom" source
# method; COPR keeps a copy, set with `copr-cli edit-package-custom proc-cli
# --script pkg/copr/copr-build.sh --script-builddeps "curl cargo tar xz"`).
# COPR runs it on the latest Fedora, with network, and builds the spec and
# tarballs it leaves in the current directory for every chroot.
#
# release.yml's "Update COPR" step starts it by POSTing the version to the
# project's custom webhook (the COPR_WEBHOOK secret); COPR saves that body as
# ./hook_payload. A body of "VERSION REF" takes the scripts from REF instead
# of the tag, to try a change to them; an empty one builds the latest release.
set -eu

payload=$(cat hook_payload 2> /dev/null || true)
# shellcheck disable=SC2086
set -- $payload
version=${1:-}
if [ -z "$version" ]; then
    version=$(curl -fsSL https://api.github.com/repos/yazeed/proc-cli/releases/latest |
        sed -n 's/.*"tag_name": *"v\([^"]*\)".*/\1/p')
fi
ref=${2:-v$version}

# Releases before 2.3.0 have no pkg/copr/sources.sh: theirs come from main
raw="https://raw.githubusercontent.com/yazeed/proc-cli/$ref/pkg/copr"
if ! curl -fsSL -o sources.sh "$raw/sources.sh"; then
    raw="https://raw.githubusercontent.com/yazeed/proc-cli/main/pkg/copr"
    curl -fsSL -o sources.sh "$raw/sources.sh"
fi
curl -fsSL -o proc-cli.spec.in "$raw/proc-cli.spec"
sh sources.sh "$version" . proc-cli.spec.in
