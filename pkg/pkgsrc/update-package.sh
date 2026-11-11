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
# synopsis, description (pkg/pkgsrc/COMMIT_MSG as the suggested commit
# message, the changelog's address and what was tested)
# and environment. Runs as root on NetBSD with the rust, pkglint and digest packages
# installed; the release's update-pkgsrc job runs it in a vmactions VM.
# PKGSRCDIR names an existing tree of that branch (default /usr/pkgsrc,
# downloaded if missing). Before the tag, VERSION has no download:
# PROC_SOURCE then names a tarball of the checkout with proc-cli-VERSION/ at
# its top (git archive --prefix=proc-cli-VERSION/, as the release's dry run
# makes it), and the package takes it as its distfile.
set -eu

version=${1:?usage: update-package.sh VERSION QUARTER OUT_DIR}
quarter=${2:?usage: update-package.sh VERSION QUARTER OUT_DIR}
out=${3:?usage: update-package.sh VERSION QUARTER OUT_DIR}
here=$(cd "$(dirname "$0")" && pwd)
tree=${PKGSRCDIR:-/usr/pkgsrc}
files="DESCR Makefile PLIST cargo-depends.mk distinfo"
trunk=https://raw.githubusercontent.com/NetBSD/pkgsrc/trunk/sysutils/proc-cli

fail() { echo "update-package.sh: $*" >&2; exit 1; }

# Without digest, make makesum writes a distinfo with no checksums at all
[ -x /usr/pkg/bin/digest ] || fail "/usr/pkg/bin/digest is missing: pkg_add digest"

synopsis=$(head -n 1 "$here/COMMIT_MSG")
[ "$synopsis" = "sysutils/proc-cli: update to $version" ] ||
    fail "pkg/pkgsrc/COMMIT_MSG starts '$synopsis', not 'sysutils/proc-cli: update to $version': write its entry first"

# COMMIT_MSG is the commit message a committer pastes as it is: the synopsis,
# a blank line, "Changes since X.Y.Z:" and one to five one-line bullets
bullets=0
number=0
while IFS= read -r line || [ -n "$line" ]; do
    number=$((number + 1))
    case $number in
        1) ;;
        2) [ -z "$line" ] || fail "COMMIT_MSG's line 2 is not empty" ;;
        3) case $line in
               "Changes since "*:) ;;
               *) fail "COMMIT_MSG's line 3 is '$line', not 'Changes since X.Y.Z:'" ;;
           esac ;;
        *) case $line in
               "- "*) ;;
               *) fail "COMMIT_MSG's line $number is not a '- ' bullet: '$line'" ;;
           esac
           [ "${#line}" -le 72 ] || fail "COMMIT_MSG's line $number is ${#line} characters; a bullet takes at most 72"
           bullets=$((bullets + 1)) ;;
    esac
done < "$here/COMMIT_MSG"
[ "$bullets" -ge 1 ] && [ "$bullets" -le 5 ] || fail "COMMIT_MSG has $bullets bullets; it takes 1 to 5"

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
# The name GitHub's download of the tag has in distinfo; a wrong one here
# would make makesum fetch it, and fail, since the tag doesn't exist yet
if [ -n "${PROC_SOURCE:-}" ]; then
    distdir=$(make show-var VARNAME=DISTDIR)
    mkdir -p "$distdir"
    cp "$PROC_SOURCE" "$distdir/proc-cli-$version.tar.gz"
fi
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
grep -q "^SHA512 (proc-cli-$version.tar.gz) = [0-9a-f]" distinfo ||
    fail "distinfo has no checksum for proc-cli-$version.tar.gz after make makesum"

# The work directory was extracted with trunk's crate list: built there,
# cargo finds the last release's crates, not the ones this Cargo.lock locks
# (2.4.0's dry run: itoa 1.0.18 against 1.0.19). Extracted again, it has these
make clean
make package install
printed=$(/usr/pkg/bin/proc --version | head -n 1)
[ "$printed" = "proc $version" ] || fail "the installed proc prints '$printed', not 'proc $version'"
echo "installed and run: $printed"
make clean
pkglint

# The repository's copies keep a bare $NetBSD$: trunk's expanded ID names
# whoever committed it last. b/ keeps trunk's, so the diff applies
for f in $files; do
    cp "$pkg/$f" "$b/$f"
    # shellcheck disable=SC2016 # the $ signs are pkgsrc's, not the shell's
    sed 's/\$NetBSD:[^$]*\$/$NetBSD$/' "$pkg/$f" > "$here/$f"
done

cd "$out"
status=0
diff -ruN a b > "proc-cli-$version-pkgsrc.diff" || status=$?
[ "$status" -eq 1 ] || fail "diff found no change from trunk, or failed ($status)"

release="NetBSD $(uname -r)"
echo "$synopsis" > synopsis
echo "$release, pkgsrc-$quarter" > environment
{
    printf 'Suggested commit message:\n\n'
    cat "$here/COMMIT_MSG"
    printf '\nFull list: https://github.com/yazeed/proc-cli/blob/v%s/CHANGELOG.md\n' "$version"
    printf 'Tested: make package, pkglint and the installed proc --version on\n%s against pkgsrc-%s.\n' "$release" "$quarter"
} > description
cat description
echo "diff: $out/proc-cli-$version-pkgsrc.diff"
