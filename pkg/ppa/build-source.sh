#!/bin/sh
# Builds the unsigned source packages of one release for the Launchpad PPA
# (ppa:yazeedaloyoun/proc-cli), one per Ubuntu release:
#   pkg/ppa/build-source.sh VERSION OUTDIR [REVISION]
# REVISION (default 1) counts uploads of the same VERSION. Runs on Ubuntu or
# Debian with curl, xz-utils, dpkg-dev and a cargo that reads Cargo.lock
# version 4 (Ubuntu 26.04's does). Sign and upload with pkg/ppa/upload.sh.
set -eu

version=${1:?usage: build-source.sh VERSION OUTDIR [REVISION]}
out=$(realpath "${2:?usage: build-source.sh VERSION OUTDIR [REVISION]}")
revision=${3:-1}
here=$(realpath "$(dirname "$0")")

# codename, release number, the Rust packages to build with
releases='jammy 22.04 cargo-1.91,_rustc-1.91
noble 24.04 cargo-1.91,_rustc-1.91
resolute 26.04 cargo_(>=_1.88),_rustc_(>=_1.88)
stonking 26.10 cargo_(>=_1.88),_rustc_(>=_1.88)'

work=$(mktemp -d)
echo "working in $work"
cd "$work"

# The release's source, then every crate it needs, as the second tarball
curl -fsSL -o "proc-cli_$version.orig.tar.gz" \
    "https://github.com/yazeed/proc-cli/archive/refs/tags/v$version.tar.gz"
mkdir crates
tar -xzf "proc-cli_$version.orig.tar.gz" -C crates
(cd "crates/proc-cli-$version" && cargo vendor --locked vendor > /dev/null)
# The same bytes every time: Launchpad keeps a tarball once uploaded, and a
# later upload of this version (a new REVISION) must send an identical one
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner \
    -C "crates/proc-cli-$version" -cf - vendor | xz -9 -T1 > "proc-cli_$version.orig-vendor.tar.xz"

mkdir -p "$out"
echo "$releases" | while read -r codename number rust; do
    full="$version-$revision~ppa1~ubuntu$number.1"
    tree="$codename/proc-cli-$version"
    mkdir -p "$codename"
    tar -xzf "proc-cli_$version.orig.tar.gz" -C "$codename"
    cp "proc-cli_$version.orig.tar.gz" "proc-cli_$version.orig-vendor.tar.xz" "$codename/"
    mkdir "$tree/vendor"
    tar -xJf "proc-cli_$version.orig-vendor.tar.xz" -C "$tree/vendor" --strip-components=1
    cp -R "$here/debian" "$tree/debian"
    sed -i "s/@RUST@/$(echo "$rust" | tr '_' ' ')/" "$tree/debian/control"
    cat > "$tree/debian/changelog" <<EOF
proc-cli ($full) $codename; urgency=medium

  * proc $version: https://github.com/yazeed/proc-cli/releases/tag/v$version

 -- Yazeed Al Oyoun <me@yazeed.com>  $(date -R)
EOF
    # The first release's upload carries both tarballs (-sa); the others only
    # name them (-sd), and Launchpad takes them from the PPA, so the 20 MB of
    # crates go over Launchpad's slow FTP once. upload.sh sends those only
    # once the tarballs are published.
    if [ "$codename" = "$(echo "$releases" | head -n 1 | cut -d' ' -f1)" ]; then
        tarballs=-sa
    else
        tarballs=-sd
    fi
    (cd "$tree" && dpkg-buildpackage -S "$tarballs" -d -nc -us -uc > /dev/null)
    cp "$codename"/proc-cli_"$full"* "$out/"
    echo "built proc-cli $full for $codename"
done
# both tarballs are shared by the four uploads
cp "proc-cli_$version.orig.tar.gz" "proc-cli_$version.orig-vendor.tar.xz" "$out/"
ls "$out"
