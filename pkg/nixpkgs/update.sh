#!/bin/sh
# Moves pkg/nixpkgs/package.nix to VERSION and checks it: puts lib.fakeHash in
# both hashes, builds with pkg/nixpkgs/build.nix to learn each real hash from
# the error that names it, then builds once more, which runs proc's whole test
# suite in the sandbox, and checks that the result prints "proc VERSION".
#   pkg/nixpkgs/update.sh VERSION
# Needs nix with nix-command and flakes. The release's update-nixpkgs job runs
# it; locally, run it in the nixos/nix image (RELEASE.md, "After the tag:
# nixpkgs").
set -eu

version=${1:?usage: update.sh VERSION}
here=$(cd "$(dirname "$0")" && pwd)
package="$here/package.nix"
log=$(mktemp)
trap 'rm -f "$log"' EXIT

fail() { echo "update.sh: $*" >&2; exit 1; }

# Replace the one line that starts with PREFIX in package.nix with LINE. Shell
# and grep only: the nixos/nix image has neither awk nor sed
set_line() {
    count=$(grep -c "^$1" "$package" || true)
    [ "$count" -eq 1 ] || fail "expected one line starting '$1' in package.nix, found $count"
    while IFS= read -r current || [ -n "$current" ]; do
        case $current in
            "$1"*) printf '%s\n' "$2" ;;
            *) printf '%s\n' "$current" ;;
        esac
    done < "$package" > "$package.new"
    mv "$package.new" "$package"
}

# The hash nix reports as "got:" for a build that failed on a fake one
hash_from_build() {
    if nix-build "$here/build.nix" --no-out-link > "$log" 2>&1; then
        fail "the build passed with a fake hash; package.nix is not what this script expects"
    fi
    got=$(grep -o 'got: *sha256-[A-Za-z0-9+/=]*' "$log" | head -n 1)
    got=${got#got:}
    got=${got##* }
    [ -n "$got" ] || { tail -n 30 "$log" >&2; fail "the build failed without naming a hash (above)"; }
    echo "$got"
}

set_line '  version = ' "  version = \"$version\";"
set_line '    hash = ' '    hash = lib.fakeHash;'
set_line '  cargoHash = ' '  cargoHash = lib.fakeHash;'

src_hash=$(hash_from_build)
echo "source hash: $src_hash"
set_line '    hash = ' "    hash = \"$src_hash\";"

cargo_hash=$(hash_from_build)
echo "cargo hash: $cargo_hash"
set_line '  cargoHash = ' "  cargoHash = \"$cargo_hash\";"

# The real build: compiles proc and runs its tests in the sandbox
if ! nix-build "$here/build.nix" -o "$here/result" > "$log" 2>&1; then
    tail -n 60 "$log" >&2
    fail "the build with the real hashes failed (above)"
fi
printed=$("$here/result/bin/proc" --version | head -n 1)
rm -f "$here/result"
[ "$printed" = "proc $version" ] || fail "the built proc prints '$printed', not 'proc $version'"
echo "built and tested: $printed"
