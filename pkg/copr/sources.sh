#!/bin/sh
# Writes what one release's Fedora package is built from into OUTDIR:
#   pkg/copr/sources.sh VERSION OUTDIR SPEC
# - the spec, from SPEC with Version set to VERSION and a changelog entry
#   for it, so no release has to edit the spec;
# - the release's source tarball, from its GitHub tag;
# - a tarball of every crate Cargo.lock names, since the build has no
#   network.
# COPR's build of each release runs it (pkg/copr/copr-build.sh), and so does
# pkg/copr/srpm.sh for a local source RPM. Needs curl, cargo, tar and xz.
set -eu

usage="usage: sources.sh VERSION OUTDIR SPEC"
version=${1:?$usage}
out=${2:?$usage}
spec=${3:?$usage}
mkdir -p "$out"
out=$(cd "$out" && pwd)

tarball="proc-cli-$version.tar.gz"
curl -fsSL -o "$out/$tarball" \
    "https://github.com/yazeed/proc-cli/archive/refs/tags/v$version/$tarball"

work=$(mktemp -d)
tar -xzf "$out/$tarball" -C "$work"
(cd "$work/proc-cli-$version" && cargo vendor --locked vendor > /dev/null)
tar -cJf "$out/proc-cli-$version-vendor.tar.xz" -C "$work/proc-cli-$version" vendor

# The spec at this version, its changelog naming the release
day=$(LC_ALL=C date -u '+%a %b %d %Y')
awk -v version="$version" -v day="$day" '
    /^Version:/ { print "Version:        " version; next }
    /^%changelog/ {
        print
        print "* " day " Yazeed Al Oyoun <me@yazeed.com> - " version "-1"
        print "- proc " version ": https://github.com/yazeed/proc-cli/releases/tag/v" version
        exit
    }
    { print }' "$spec" > "$out/proc-cli.spec"
echo "wrote proc-cli.spec, $tarball and proc-cli-$version-vendor.tar.xz to $out"
