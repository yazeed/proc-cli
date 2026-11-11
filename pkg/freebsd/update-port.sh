#!/bin/sh
# Moves the FreeBSD port sysutils/proc-cli to VERSION in a ports tree, builds
# and checks it, copies its files back to pkg/freebsd/ and writes the patch
# for Bugzilla (RELEASE.md, "After the tag: FreeBSD port"):
#   pkg/freebsd/update-port.sh VERSION OUT_DIR
# OUT_DIR gets proc-cli-VERSION-freebsd.patch and `kind`: "new" while the
# port is not in the tree (the patch adds it, with its SUBDIR line), "update"
# once it is (the patch moves the tree's port to VERSION). Runs as root on
# FreeBSD with git, portlint and the rust package from pkg's latest branch,
# which matches the lang/rust of the ports tree's main branch. The release's
# update-freebsd job runs it in a vmactions VM; PORTS names an existing tree
# (default /usr/ports, cloned if missing). Before the tag, VERSION has no
# download: PROC_SOURCE then names a tarball of the checkout with
# proc-cli-VERSION/ at its top (git archive --prefix=proc-cli-VERSION/, as the
# release's dry run makes it), and the port takes it as its distfile.
set -eu

version=${1:?usage: update-port.sh VERSION OUT_DIR}
out=${2:?usage: update-port.sh VERSION OUT_DIR}
here=$(cd "$(dirname "$0")" && pwd)
ports=${PORTS:-/usr/ports}
port=$ports/sysutils/proc-cli
files="Makefile Makefile.crates distinfo pkg-descr"
# The framework finds its makefiles through PORTSDIR (default /usr/ports)
# DEVELOPER=yes makes stage-qa run all its checks
export BATCH=yes DEVELOPER=yes LC_ALL=C PORTSDIR="$ports"

fail() { echo "update-port.sh: $*" >&2; exit 1; }

mkdir -p "$out"
out=$(cd "$out" && pwd)
[ -e "$ports/.git" ] || git clone --quiet --depth 1 https://git.FreeBSD.org/ports.git "$ports"

# A committed port is the tree's own, which a committer may have changed;
# until then it is ours
if [ -d "$port" ]; then
    kind=update
else
    kind=new
    mkdir "$port"
    for f in $files; do cp "$here/$f" "$port/"; done
fi

[ "$(grep -c '^DISTVERSION=' "$port/Makefile")" -eq 1 ] || fail "expected one DISTVERSION= line in the port's Makefile"
sed -i '' "s/^DISTVERSION=.*/DISTVERSION=	$version/" "$port/Makefile"

cd "$port"
# The name GitHub's download of the tag has in distinfo; a wrong one here
# would make makesum fetch it, and fail, since the tag doesn't exist yet
if [ -n "${PROC_SOURCE:-}" ]; then
    distdir=$(make -V DISTDIR)
    mkdir -p "$distdir"
    cp "$PROC_SOURCE" "$distdir/yazeed-proc-cli-v${version}_GH0.tar.gz"
fi
make makesum
# Extracted first: otherwise cargo-crates prints the framework's progress
# lines (===> ...) into the list too
make extract
make cargo-crates > Makefile.crates.new
head -n 1 Makefile.crates.new | grep -q '^CARGO_CRATES=' ||
    fail "make cargo-crates did not start with CARGO_CRATES=: $(head -n 1 Makefile.crates.new)"
! grep -q '^===>' Makefile.crates.new || fail "make cargo-crates printed progress lines into the crate list"
mv Makefile.crates.new Makefile.crates
make makesum

# The work directory was extracted with the old crate list: built there, proc
# would get the last release's crates, not the ones this Cargo.lock locks
# (2.4.0's dry run built itoa 1.0.18 against 1.0.19). Extracted again, it has
# the ones the port lists
make clean
make stage check-plist stage-qa package
pkg add -f "work/pkg/proc-cli-$version.pkg"
printed=$(/usr/local/bin/proc --version | head -n 1)
[ "$printed" = "proc $version" ] || fail "the installed proc prints '$printed', not 'proc $version'"
echo "installed and run: $printed"
make clean

# A new port is listed in sysutils/Makefile, in alphabetical order
if [ "$kind" = new ] && ! grep -q '^    SUBDIR += proc-cli$' "$ports/sysutils/Makefile"; then
    awk '!done && /^    SUBDIR \+= / && $3 > "proc-cli" { print "    SUBDIR += proc-cli"; done = 1 } { print }' \
        "$ports/sysutils/Makefile" > "$out/sysutils-Makefile"
    cp "$out/sysutils-Makefile" "$ports/sysutils/Makefile"
    rm "$out/sysutils-Makefile"
    grep -q '^    SUBDIR += proc-cli$' "$ports/sysutils/Makefile" || fail "found no place for proc-cli in sysutils/Makefile"
fi

for f in $files; do cp "$port/$f" "$here/"; done

# portlint checks the port's files are in git, so they are added first
cd "$ports"
git add sysutils/proc-cli sysutils/Makefile
(cd "$port" && portlint -AC)

if [ "$kind" = new ]; then
    printf 'sysutils/proc-cli: Add new port\n\n%s\n\nWWW: https://proc-cli.com/\n' "$(cat "$port/pkg-descr")" > "$out/message"
else
    printf 'sysutils/proc-cli: Update to %s\n' "$version" > "$out/message"
fi
git -c user.name="Yazeed Al Oyoun" -c user.email=me@yazeed.com commit --quiet -F "$out/message"
rm "$out/message"
git format-patch -1 --stdout > "$out/proc-cli-$version-freebsd.patch"
echo "$kind" > "$out/kind"
git show --stat HEAD
echo "patch ($kind): $out/proc-cli-$version-freebsd.patch"
