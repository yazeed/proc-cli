#!/bin/sh
# Builds the signed apt repository served at https://proc-cli.com/apt from
# the .deb files of the given releases:
#   pkg/apt/build-repo.sh OUTDIR VERSION...
# Each .deb is checked against its release's checksums.txt. The signing key
# is the one secret key in gpg's keyring (CI imports it from APT_SIGNING_KEY).
# Needs curl, sha256sum, apt-ftparchive (apt-utils) and gpg.
set -eu

out=${1:?usage: build-repo.sh OUTDIR VERSION...}
shift
[ "$#" -gt 0 ] || { echo "build-repo.sh: no versions given" >&2; exit 1; }

key=$(gpg --batch --list-secret-keys --with-colons | awk -F: '/^fpr:/ { print $10; exit }')
[ -n "$key" ] || { echo "build-repo.sh: no secret key to sign with" >&2; exit 1; }

releases=https://github.com/yazeed/proc-cli/releases/download
pool="$out/pool/main/p/proc-cli"
mkdir -p "$pool"

for version in "$@"; do
    sums=$(curl -fsSL "$releases/v$version/checksums.txt")
    for arch in amd64 arm64; do
        deb="proc-cli_${version}_${arch}.deb"
        curl -fsSL -o "$pool/$deb" "$releases/v$version/$deb"
        expected=$(printf '%s\n' "$sums" | awk -v f="$deb" '$2 == f { print $1 }')
        [ -n "$expected" ] || { echo "build-repo.sh: $deb is not in checksums.txt" >&2; exit 1; }
        echo "$expected  $pool/$deb" | sha256sum -c --quiet -
    done
done

dist="$out/dists/stable"
for arch in amd64 arm64; do
    mkdir -p "$dist/main/binary-$arch"
    # paths in Packages are relative to the repository root
    (cd "$out" && apt-ftparchive --arch "$arch" packages pool) > "$dist/main/binary-$arch/Packages"
    gzip -9nk "$dist/main/binary-$arch/Packages"
done

apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=proc-cli \
    -o APT::FTPArchive::Release::Label=proc-cli \
    -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Codename=stable \
    -o "APT::FTPArchive::Release::Architectures=amd64 arm64" \
    -o APT::FTPArchive::Release::Components=main \
    -o "APT::FTPArchive::Release::Description=proc, a semantic CLI tool for process management" \
    release "$dist" > "$out/Release.tmp"
mv "$out/Release.tmp" "$dist/Release"

gpg --batch --yes --local-user "$key" --clearsign -o "$dist/InRelease" "$dist/Release"
gpg --batch --yes --local-user "$key" --armor --detach-sign -o "$dist/Release.gpg" "$dist/Release"

# the public key, binary for /etc/apt/keyrings and armored to read
gpg --batch --yes --export "$key" > "$out/proc-cli.gpg"
gpg --batch --yes --armor --export "$key" > "$out/proc-cli.asc"
