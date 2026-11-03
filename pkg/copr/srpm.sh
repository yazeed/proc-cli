#!/bin/sh
# Builds the COPR source RPM for one released version, on Fedora:
#   pkg/copr/srpm.sh 2.2.0 [outdir]
# Needs rpm-build, cargo and curl. Writes proc-cli-<version>-1.<dist>.src.rpm
# to outdir (default: the current directory); upload it with
#   copr-cli build yazeed/proc-cli <the .src.rpm>
set -eu

version=${1:?usage: srpm.sh VERSION [OUTDIR]}
outdir=$(realpath "${2:-.}")
spec=$(realpath "$(dirname "$0")/proc-cli.spec")

if ! grep -q "^Version: *$version\$" "$spec"; then
    echo "proc-cli.spec is not at version $version: bump Version first" >&2
    exit 1
fi

# The sources stay in this temporary directory after the build.
work=$(mktemp -d)
echo "working in $work"

mkdir -p "$work/SOURCES"
tarball="proc-cli-$version.tar.gz"
curl -fsSL -o "$work/SOURCES/$tarball" \
    "https://github.com/yazeed/proc-cli/archive/refs/tags/v$version/$tarball"

# The vendor tarball holds every crate Cargo.lock names, for any platform.
tar -xzf "$work/SOURCES/$tarball" -C "$work"
(cd "$work/proc-cli-$version" && cargo vendor --locked vendor > /dev/null)
tar -cJf "$work/SOURCES/proc-cli-$version-vendor.tar.xz" \
    -C "$work/proc-cli-$version" vendor

rpmbuild -bs --define "_topdir $work" --define "_srcrpmdir $outdir" "$spec"
