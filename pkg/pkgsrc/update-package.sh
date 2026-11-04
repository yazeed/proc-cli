#!/bin/sh
# Moves pkgsrc's sysutils/proc-cli to VERSION, builds and checks it on NetBSD,
# copies its files back to pkg/pkgsrc/ and writes the diff and the text for
# the gnats problem report (RELEASE.md, "After the tag: pkgsrc"):
#   pkg/pkgsrc/update-package.sh VERSION QUARTER OUT_DIR
# It starts from the package as it is on pkgsrc trunk (NetBSD/pkgsrc's GitHub
# mirror), so the diff applies there, and builds it in the QUARTER branch
# (e.g. 2026Q3), the one NetBSD's binary packages, rust among them, come
# from: a branch whose lang/rust differs would build Rust itself. OUT_DIR
# gets proc-cli-VERSION-pkgsrc.diff (patch -p1 from the pkgsrc root),
# synopsis, description (pkg/pkgsrc/COMMIT_MSG's body, and what was tested)
# and environment. Runs as root on NetBSD with the rust and pkglint packages
# installed; the release's update-pkgsrc job runs it in a vmactions VM.
# PKGSRCDIR names an existing tree of that branch (default /usr/pkgsrc,
# downloaded if missing).
set -eu

version=${1:?usage: update-package.sh VERSION QUARTER OUT_DIR}
quarter=${2:?usage: update-package.sh VERSION QUARTER OUT_DIR}
out=${3:?usage: update-package.sh VERSION QUARTER OUT_DIR}
here=$(cd "$(dirname "$0")" && pwd)
tree=${PKGSRCDIR:-/usr/pkgsrc}
files="DESCR Makefile PLIST cargo-depends.mk distinfo"
trunk=https://raw.githubusercontent.com/NetBSD/pkgsrc/trunk/sysutils/proc-cli

fail() { echo "update-package.sh: $*" >&2; exit 1; }

synopsis=$(head -n 1 "$here/COMMIT_MSG")
[ "$synopsis" = "sysutils/proc-cli: update to $version" ] ||
    fail "pkg/pkgsrc/COMMIT_MSG starts '$synopsis', not 'sysutils/proc-cli: update to $version': write its entry first"

mkdir -p "$out"
out=$(cd "$out" && pwd)
if [ ! -d "$tree" ]; then
    ftp -V -o "$out/pkgsrc.tar.gz" "https://cdn.netbsd.org/pub/pkgsrc/pkgsrc-$quarter/pkgsrc.tar.gz"
    tar -xzf "$out/pkgsrc.tar.gz" -C /usr
    rm "$out/pkgsrc.tar.gz"
fi

# a/ is trunk's package, b/ the updated one
a=$out/a/sysutils/proc-cli
b=$out/b/sysutils/proc-cli
pkg=$tree/sysutils/proc-cli
mkdir -p "$a" "$b" "$pkg"
for f in $files; do
    ftp -V -o "$a/$f" "$trunk/$f"
    cp "$a/$f" "$pkg/$f"
done

[ "$(grep -c '^DISTNAME=' "$pkg/Makefile")" -eq 1 ] || fail "expected one DISTNAME= line in the package's Makefile"
sed "s/^DISTNAME=.*/DISTNAME=	proc-cli-$version/" "$pkg/Makefile" > "$out/Makefile"
mv "$out/Makefile" "$pkg/Makefile"

cd "$pkg"
# A work directory left from another build would be reused
make clean
make makesum
# print-cargo-depends reads Cargo.lock from the extracted source. It writes a
# bare "# $NetBSD$" first line: trunk's is kept
make extract
make print-cargo-depends > "$out/cargo-depends.mk"
grep -q '^CARGO_CRATE_DEPENDS+=' "$out/cargo-depends.mk" || fail "make print-cargo-depends printed no crates"
! grep -q '^=' "$out/cargo-depends.mk" || fail "make print-cargo-depends printed progress lines into the crate list"
{ head -n 1 "$a/cargo-depends.mk"; tail -n +2 "$out/cargo-depends.mk"; } > cargo-depends.mk
rm "$out/cargo-depends.mk"
make makesum

make package install
printed=$(/usr/pkg/bin/proc --version | head -n 1)
[ "$printed" = "proc $version" ] || fail "the installed proc prints '$printed', not 'proc $version'"
echo "installed and run: $printed"
make clean
pkglint

for f in $files; do
    cp "$pkg/$f" "$b/$f"
    cp "$pkg/$f" "$here/$f"
done

cd "$out"
status=0
diff -ruN a b > "proc-cli-$version-pkgsrc.diff" || status=$?
[ "$status" -eq 1 ] || fail "diff found no change from trunk, or failed ($status)"

release="NetBSD $(uname -r)"
echo "$synopsis" > synopsis
echo "$release, pkgsrc-$quarter" > environment
{
    tail -n +3 "$here/COMMIT_MSG"
    printf 'Tested: make package, pkglint and the installed proc --version on\n%s against pkgsrc-%s.\n' "$release" "$quarter"
} > description
cat description
echo "diff: $out/proc-cli-$version-pkgsrc.diff"
