#!/bin/sh
# The PPA's part of a release, run on the maintainer's Mac, where the signing
# key stays (it is never given to CI): builds the source packages in an
# Ubuntu container, signs them with the key here and uploads them, waits for
# Launchpad to publish the first upload's tarballs before sending the rest,
# and then until Launchpad has accepted every upload.
#   pkg/ppa/release.sh VERSION [REVISION]
# Needs Docker, gpg, openssl, curl and python3. Safe to run again: the
# packages are built once per VERSION-REVISION (in .scratch/ppa/), what was
# sent is not sent twice, and what was accepted is only checked.
set -eu

version=${1:?usage: release.sh VERSION [REVISION]}
revision=${2:-1}
here=$(cd "$(dirname "$0")" && pwd)
builds="$(cd "$here/../.." && pwd)/.scratch/ppa"
out="$builds/$version-$revision"
api=https://api.launchpad.net/devel/~yazeedaloyoun/+archive/ubuntu/proc-cli

fail() { echo "release.sh: $*" >&2; exit 1; }

if [ ! -d "$out" ]; then
    curl -fsI -o /dev/null "https://github.com/yazeed/proc-cli/releases/tag/v$version" ||
        fail "there is no release v$version on GitHub yet"
    docker info > /dev/null 2>&1 || fail "Docker is not running; start it and run this again"
    # Built in a folder of its own and moved into place whole, so a build that
    # stops halfway is never taken for a finished one
    mkdir -p "$builds"
    partial=$(mktemp -d "$builds/.building-$version-$revision.XXXXXX")
    docker run --rm --cpus=4 -v "$here:/ppa:ro" -v "$partial:/out" ubuntu:26.04 sh -c '
        export DEBIAN_FRONTEND=noninteractive
        apt-get update -qq && apt-get install -y -qq curl ca-certificates xz-utils dpkg-dev cargo > /dev/null
        /ppa/build-source.sh "$0" /out "$1" > /dev/null' "$version" "$revision" ||
        fail "the source packages did not build (partial output in $partial)"
    made=0
    for changes in "$partial"/*_source.changes; do
        [ -e "$changes" ] && made=$((made + 1))
    done
    [ "$made" -eq 4 ] || fail "the build made $made source packages, not 4 (in $partial)"
    mv "$partial" "$out"
fi

# The first release's upload carries the tarballs; the others wait until
# the PPA has published them, usually within the hour
until=$(( $(date +%s) + 3 * 3600 ))
while :; do
    "$here/upload.sh" "$out" || echo "an upload failed; trying again"
    left=0
    for changes in "$out"/*_source.changes; do
        [ -e "$changes.uploaded" ] || left=$((left + 1))
    done
    [ "$left" -eq 0 ] && break
    [ "$(date +%s)" -lt "$until" ] ||
        fail "$left upload(s) still not sent after 3 hours; run this again later"
    echo "waiting for Launchpad to publish the tarballs ($left left); looking again in 5 minutes"
    sleep 300
done

# Sent is not accepted: Launchpad can still reject an upload (and only says
# so by email). Wait until each version is in the PPA, building or published
versions=$(for changes in "$out"/*_source.changes; do
    awk '/^Version:/ { print $2; exit }' "$changes"
done)
until=$(( $(date +%s) + 3600 ))
while :; do
    missing=$(for v in $versions; do
        found=$(curl -m 60 -fsS "$api?ws.op=getPublishedSources&source_name=proc-cli&version=$v&exact_match=true" 2> /dev/null |
            python3 -c 'import json, sys; print(json.load(sys.stdin)["total_size"])' 2> /dev/null || echo 0)
        [ "$found" -gt 0 ] || printf ' %s' "$v"
    done)
    [ -z "$missing" ] && break
    [ "$(date +%s)" -lt "$until" ] ||
        fail "Launchpad has not accepted$missing an hour after sending; see its email for why"
    echo "waiting for Launchpad to accept:$missing"
    sleep 120
done
echo "accepted: $(echo "$versions" | tr '\n' ' ')"
echo "builds: https://launchpad.net/~yazeedaloyoun/+archive/ubuntu/proc-cli/+packages"
