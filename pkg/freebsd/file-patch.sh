#!/bin/sh
# Files the patch update-port.sh wrote with FreeBSD's Bugzilla, as the port's
# maintainer, with maintainer-approval set to +:
#   pkg/freebsd/file-patch.sh VERSION OUT_DIR [--file]
# A "new" patch is attached to bug 300492 (the new port), and the patches it
# replaces there are marked obsolete after it. An "update" patch, once the
# port is committed, is a new bug in Ports & Packages, Individual Port(s),
# "[PATCH] sysutils/proc-cli: Update to VERSION". Run again, it files nothing
# it has filed already. Without --file it only says what it would file.
# --file needs FREEBSD_BUGZILLA_API_KEY, an API key of the maintainer's
# account (Bugzilla 5.0 takes it as a query parameter only), and curl and jq.
# The release's update-freebsd job runs it (RELEASE.md, "After the tag:
# FreeBSD port").
set -eu

version=${1:?usage: file-patch.sh VERSION OUT_DIR [--file]}
out=${2:?usage: file-patch.sh VERSION OUT_DIR [--file]}
file=${3:-}
api=https://bugs.freebsd.org/bugzilla/rest
maintainer=me@yazeed.com
new_port_bug=300492
patch=$out/proc-cli-$version-freebsd.patch

fail() { echo "file-patch.sh: $*" >&2; exit 1; }

[ -s "$patch" ] || fail "no patch at $patch: run update-port.sh $version first"
kind=$(cat "$out/kind")
case $kind in
    new) summary="sysutils/proc-cli $version: new port (git format-patch)"
         title="bug $new_port_bug" ;;
    update) summary="[PATCH] sysutils/proc-cli: Update to $version"
            title="a new bug, '$summary'" ;;
    *) fail "$out/kind says '$kind', not new or update" ;;
esac

if [ "$file" != --file ]; then
    echo "would file $patch on $title (not filed: no --file)"
    exit 0
fi
key=${FREEBSD_BUGZILLA_API_KEY:?--file needs FREEBSD_BUGZILLA_API_KEY}

# METHOD PATH [BODY_FILE] [QUERY]: the response, or the error Bugzilla gives
call() {
    if [ -n "${3:-}" ]; then
        response=$(curl -sS -X "$1" -H 'Content-Type: application/json' --data "@$3" \
            "$api/$2?api_key=$key${4:-}")
    else
        response=$(curl -sS -X "$1" "$api/$2?api_key=$key${4:-}")
    fi
    if [ "$(printf '%s' "$response" | jq -r '.error // false')" = true ]; then
        fail "Bugzilla refused $1 $2: $(printf '%s' "$response" | jq -r .message)"
    fi
    printf '%s' "$response"
}

# The patch, as an attachment with maintainer-approval +
body=$(mktemp)
trap 'rm -f "$body"' EXIT
attach() {
    jq -n --arg data "$(base64 < "$patch" | tr -d '\n')" --arg summary "$summary" \
        --arg name "$(basename "$patch")" --argjson bug "$1" \
        '{ids: [$bug], data: $data, file_name: $name, summary: $summary,
          content_type: "text/plain", is_patch: true,
          flags: [{name: "maintainer-approval", status: "+"}]}' > "$body"
    id=$(call POST "bug/$1/attachment" "$body" | jq -r '.ids[0]')
    echo "attached $id to https://bugs.freebsd.org/bugzilla/show_bug.cgi?id=$1"
}

if [ "$kind" = new ]; then
    current=$(call GET "bug/$new_port_bug/attachment" "" "&include_fields=id,creator,summary,is_obsolete,is_patch" |
        jq -r --arg bug "$new_port_bug" --arg me "$maintainer" \
            '.bugs[$bug][] | select(.is_obsolete == 0 and .is_patch == 1 and .creator == $me) | "\(.id)\t\(.summary)"')
    if printf '%s\n' "$current" | cut -f 2 | grep -qxF "$summary"; then
        echo "bug $new_port_bug already has '$summary'"
        exit 0
    fi
    attach "$new_port_bug"
    for old in $(printf '%s\n' "$current" | cut -f 1); do
        jq -n --argjson id "$old" '{ids: [$id], is_obsolete: true}' > "$body"
        call PUT "bug/attachment/$old" "$body" > /dev/null
        echo "marked attachment $old obsolete"
    done
else
    query="&product=Ports%20%26%20Packages&creator=$maintainer&resolution=---&include_fields=id,summary"
    existing=$(call GET bug "" "$query" | jq -r --arg summary "$summary" '.bugs[] | select(.summary == $summary) | .id' | head -n 1)
    if [ -n "$existing" ]; then
        echo "bug $existing is already '$summary'"
        exit 0
    fi
    jq -n --arg summary "$summary" --arg version "$version" \
        '{product: "Ports & Packages", component: "Individual Port(s)", version: "Latest",
          op_sys: "Any", platform: "Any", summary: $summary,
          description: "Update to \($version): https://github.com/yazeed/proc-cli/releases/tag/v\($version)"}' > "$body"
    bug=$(call POST bug "$body" | jq -r '.id')
    echo "opened https://bugs.freebsd.org/bugzilla/show_bug.cgi?id=$bug"
    attach "$bug"
fi
