#!/bin/sh
# Brings pkg/nixpkgs/package.nix (moved to VERSION by update.sh) to the nixpkgs
# pull request, which is NixOS/nixpkgs from yazeed/nixpkgs, branch
# proc-cli-init, with two commits: "maintainers: add yazeed" and
# "proc-cli: init at X.Y.Z". The second is amended to the new package.nix and
# version, and with --push force-pushed with a lease on the commit it replaced,
# and the pull request is retitled to match. Only the version and hash lines
# may differ from the branch: anything else stops it, to be brought over by
# hand. Once nixpkgs has proc-cli its update bot opens each update, and this
# does nothing.
#   pkg/nixpkgs/update-fork.sh VERSION [--push]
# Needs git and gh (GH_TOKEN to read); --push needs NIXPKGS_FORK_TOKEN, a token
# that can push to yazeed/nixpkgs and edit the pull request. The release's
# update-nixpkgs job runs it (RELEASE.md, "After the tag: nixpkgs").
set -eu

version=${1:?usage: update-fork.sh VERSION [--push]}
push=${2:-}
here=$(cd "$(dirname "$0")" && pwd)
branch=proc-cli-init
file=pkgs/by-name/pr/proc-cli/package.nix

fail() { echo "update-fork.sh: $*" >&2; exit 1; }

grep -qx "  version = \"$version\";" "$here/package.nix" ||
    fail "pkg/nixpkgs/package.nix is not at $version: run update.sh $version first"

if gh api "repos/NixOS/nixpkgs/contents/$file" > /dev/null 2>&1; then
    echo "nixpkgs has proc-cli: its update bot opens the update, so the fork is left alone"
    exit 0
fi

# The branch's last commits and the one file, not the whole of nixpkgs
work=$(mktemp -d)
git clone --quiet --filter=blob:none --no-checkout --depth 3 --branch "$branch" \
    https://github.com/yazeed/nixpkgs.git "$work"
git -C "$work" sparse-checkout set --no-cone "/$file"
git -C "$work" checkout --quiet "$branch"

old=$(git -C "$work" rev-parse HEAD)
subject=$(git -C "$work" log -1 --format=%s)
case $subject in
    "proc-cli: init at "*) ;;
    *) fail "$branch's last commit is '$subject', not 'proc-cli: init at ...'" ;;
esac
below=$(git -C "$work" log -1 --format=%s HEAD~1)
[ "$below" = "maintainers: add yazeed" ] ||
    fail "the commit below it is '$below', not 'maintainers: add yazeed'"

cp "$here/package.nix" "$work/$file"
if git -C "$work" diff --quiet -- "$file" && [ "$subject" = "proc-cli: init at $version" ]; then
    echo "$branch already has proc-cli $version ($old)"
    exit 0
fi

# Every changed line is the version or a hash
other=$(git -C "$work" diff -U0 -- "$file" | grep '^[-+]' | grep -v -e '^--- ' -e '^+++ ' |
    grep -v -e '^[-+]  version = ' -e '^[-+]    hash = ' -e '^[-+]  cargoHash = ' || true)
if [ -n "$other" ]; then
    git -C "$work" diff -- "$file" >&2
    fail "package.nix differs from $branch in more than the version and hashes (above): bring that to the pull request by hand"
fi

# The same author and message, with the new version in the subject
name=$(git -C "$work" log -1 --format=%an)
email=$(git -C "$work" log -1 --format=%ae)
body=$(git -C "$work" log -1 --format=%b)
git -C "$work" add "$file"
GIT_COMMITTER_NAME=$name GIT_COMMITTER_EMAIL=$email \
    git -C "$work" -c user.name="$name" -c user.email="$email" \
    commit --quiet --amend -m "proc-cli: init at $version" -m "$body"
git -C "$work" log -1 --format='%H%n%an <%ae>%n%n%B'
git -C "$work" diff "$old" HEAD

if [ "$push" != --push ]; then
    echo "not pushed (no --push): $branch stays at $old"
    exit 0
fi

token=${NIXPKGS_FORK_TOKEN:?--push needs NIXPKGS_FORK_TOKEN}
auth=$(printf 'x-access-token:%s' "$token" | base64 | tr -d '\n')
git -C "$work" -c http.extraHeader="Authorization: Basic $auth" push --quiet \
    --force-with-lease="$branch:$old" https://github.com/yazeed/nixpkgs.git "HEAD:$branch"
echo "pushed $branch: $old -> $(git -C "$work" rev-parse HEAD)"

pr=$(gh pr list -R NixOS/nixpkgs --head "$branch" --author yazeed --state open --json number --jq '.[0].number')
[ -n "$pr" ] || fail "no open pull request from yazeed:$branch on NixOS/nixpkgs"
# The push is done by now, so a refused retitle is a warning with the command
if GH_TOKEN=$token gh pr edit "$pr" -R NixOS/nixpkgs --title "proc-cli: init at $version" > /dev/null; then
    echo "https://github.com/NixOS/nixpkgs/pull/$pr is now 'proc-cli: init at $version'"
else
    echo "::warning::the pull request kept its old title; retitle it: gh pr edit $pr -R NixOS/nixpkgs --title 'proc-cli: init at $version'"
fi
