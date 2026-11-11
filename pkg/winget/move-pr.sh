#!/bin/sh
# Moves the open microsoft/winget-pkgs pull request for yazeed.proc-cli's
# first version (from yazeed/winget-pkgs) to VERSION, until winget-pkgs has
# the package; from then on winget-releaser opens each release's own pull
# request, and this does nothing. The pull request waits for a volunteer
# moderator, so without this it would land at whatever version it was opened
# with (2.2.0, still there at the 2.4.0 release). One commit on top of the
# branch replaces the version folder's three manifests, then the title and
# description are moved too. Only the version, the download, its SHA-256 (the
# release's checksums.txt), the release date and the release notes link may
# change: anything else stops it. The new manifests are also written to
# pkg/winget/, which main keeps as what was submitted.
#   pkg/winget/move-pr.sh VERSION [--push]
# Without --push it prints the change and pushes nothing. Needs gh, jq and
# sha256sum or shasum; GH_TOKEN to read, and with --push WINGET_TOKEN's (a
# classic token with public_repo, which can push to the fork and edit the pull
# request). PR_HEAD names another commit of the branch to start from, to check
# the script against an earlier state without --push. The release's
# update-winget job runs it (RELEASE.md, step 6).
set -eu

version=${1:?usage: move-pr.sh VERSION [--push]}
push=${2:-}
here=$(cd "$(dirname "$0")" && pwd)
upstream=microsoft/winget-pkgs
fork=yazeed/winget-pkgs
dir=manifests/y/yazeed/proc-cli
zip=proc-windows-x86_64.exe.zip
files="yazeed.proc-cli.yaml yazeed.proc-cli.locale.en-US.yaml yazeed.proc-cli.installer.yaml"

fail() { echo "move-pr.sh: $*" >&2; exit 1; }
sha256() { if command -v sha256sum > /dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }

if gh api "repos/$upstream/contents/$dir" > /dev/null 2>&1; then
    echo "winget-pkgs has yazeed.proc-cli: winget-releaser opens each release's pull request"
    exit 0
fi

prs=$(gh pr list -R "$upstream" --state open --author yazeed --limit 100 \
    --json number,title,headRefName,headRepositoryOwner \
    --jq '[.[] | select(.title | startswith("New package: yazeed.proc-cli version "))]')
[ "$(echo "$prs" | jq length)" -eq 1 ] ||
    fail "expected one open 'New package: yazeed.proc-cli' pull request on $upstream, found $(echo "$prs" | jq length)"
number=$(echo "$prs" | jq -r '.[0].number')
branch=$(echo "$prs" | jq -r '.[0].headRefName')
[ "$(echo "$prs" | jq -r '.[0].headRepositoryOwner.login')" = yazeed ] || fail "#$number is not from $fork"

head=$(gh api "repos/$fork/branches/$branch" --jq .commit.sha)
from=${PR_HEAD:-$head}
old=$(gh api "repos/$fork/contents/$dir?ref=$from" --jq '[.[] | select(.type == "dir") | .name] | if length == 1 then .[0] else error("not one version folder") end') ||
    fail "$fork's $branch at $from does not hold one version folder under $dir"
if [ "$old" = "$version" ]; then
    echo "$upstream#$number already offers $version"
    exit 0
fi

work=$(mktemp -d)
trap 'rm -r "$work"' EXIT
if ! gh release view "v$version" -R yazeed/proc-cli > /dev/null 2>&1; then
    [ "$push" != --push ] || fail "v$version has no release to point the manifests at"
    echo "v$version has no release yet: a tag's run moves $upstream#$number from $old to it"
    exit 0
fi
gh release download "v$version" -R yazeed/proc-cli -D "$work" -p "$zip" -p checksums.txt
sha=$(sha256 "$work/$zip")
grep -qx "$sha  $zip" "$work/checksums.txt" || fail "$zip's SHA-256 $sha is not the one in v$version's checksums.txt"
sha=$(echo "$sha" | tr '[:lower:]' '[:upper:]')
date=$(gh release view "v$version" -R yazeed/proc-cli --json publishedAt --jq '.publishedAt[0:10]')

o=$(echo "$old" | sed 's/\./\\./g')
for f in $files; do
    gh api "repos/$fork/contents/$dir/$old/$f?ref=$from" --jq .content | base64 -d > "$work/old-$f"
    sed -e "s/^PackageVersion: $o\$/PackageVersion: $version/" \
        -e "s#/releases/tag/v$o\$#/releases/tag/v$version#" \
        -e "s#/releases/download/v$o/$zip\$#/releases/download/v$version/$zip#" \
        -e "s/^ReleaseDate: .*/ReleaseDate: $date/" \
        -e "s/^  InstallerSha256: .*/  InstallerSha256: $sha/" \
        "$work/old-$f" > "$work/$f"
    ! grep -qF "$old" "$work/$f" || fail "$f still names $old after the move: $(grep -F "$old" "$work/$f")"
    # Every changed line is one of those
    other=$(diff "$work/old-$f" "$work/$f" | grep '^[<>]' |
        grep -v -e '^[<>] PackageVersion: ' -e '^[<>] ReleaseNotesUrl: ' -e '^[<>]   InstallerUrl: ' \
            -e '^[<>] ReleaseDate: ' -e '^[<>]   InstallerSha256: ' || true)
    [ -z "$other" ] || fail "$f would change more than the version lines:
$other"
done
grep -qx "  InstallerUrl: https://github.com/yazeed/proc-cli/releases/download/v$version/$zip" "$work/yazeed.proc-cli.installer.yaml" ||
    fail "the installer manifest's InstallerUrl is not v$version's $zip"

title="New package: yazeed.proc-cli version $version"
gh pr view "$number" -R "$upstream" --json body --jq .body > "$work/old-body"
sed "s/$o/$version/g" "$work/old-body" > "$work/body"

for f in $files; do diff -u "$work/old-$f" "$work/$f" || true; done
diff -u "$work/old-body" "$work/body" || true
for f in $files; do cp "$work/$f" "$here/$f"; done

if [ "$push" != --push ]; then
    echo "not pushed: with --push, $upstream#$number moves from $old to $version, titled '$title'"
    exit 0
fi
[ "$from" = "$head" ] || fail "PR_HEAD is only for checking, without --push"

# One commit on top of the branch; the ref moves only if the branch is still
# at the commit read above
tree=$(gh api "repos/$fork/commits/$head" --jq .commit.tree.sha)
jq -n --arg base "$tree" --arg dir "$dir" --arg old "$old" --arg new "$version" \
    --rawfile v "$work/yazeed.proc-cli.yaml" \
    --rawfile l "$work/yazeed.proc-cli.locale.en-US.yaml" \
    --rawfile i "$work/yazeed.proc-cli.installer.yaml" '
    {base_tree: $base, tree: (
      [ "yazeed.proc-cli.yaml", "yazeed.proc-cli.locale.en-US.yaml", "yazeed.proc-cli.installer.yaml" ]
      | map({path: "\($dir)/\($old)/\(.)", mode: "100644", type: "blob", sha: null})
      + [ {name: "yazeed.proc-cli.yaml", content: $v},
          {name: "yazeed.proc-cli.locale.en-US.yaml", content: $l},
          {name: "yazeed.proc-cli.installer.yaml", content: $i} ]
        | map(if .name then {path: "\($dir)/\($new)/\(.name)", mode: "100644", type: "blob", content: .content} else . end))}' \
    > "$work/tree.json"
newtree=$(gh api -X POST "repos/$fork/git/trees" --input "$work/tree.json" --jq .sha)
commit=$(gh api -X POST "repos/$fork/git/commits" -f message="$title" -f tree="$newtree" -f "parents[]=$head" --jq .sha)
gh api -X PATCH "repos/$fork/git/refs/heads/$branch" -f sha="$commit" -F force=false > /dev/null ||
    fail "$branch moved while this ran: nothing was changed, run it again"
gh pr edit "$number" -R "$upstream" --title "$title" --body-file "$work/body" > /dev/null
echo "$upstream#$number moved from $old to $version ($commit)"
