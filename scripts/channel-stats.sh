#!/bin/sh
# How much proc is used, from every place that counts it: the repository's
# stars, visits and clones, and the downloads each channel reports.
#
#   scripts/channel-stats.sh
#
# Each line: source, what is counted, the number. Reads public APIs and pages
# with curl and jq; the repository's visits, clones and referrers come from
# GitHub's traffic API through `gh`, which needs push access, and are skipped
# without it. Nothing is written.
#
# What the numbers are: GitHub keeps visits and clones for 14 days only, so
# keep a dated copy of the output to compare over time. Release-file
# downloads, Docker pulls, PPA and COPR include our own CI and test installs,
# and npm and PyPI count automated mirrors, so a number is best read against
# its own earlier value, not as people. Homebrew (our own tap), Scoop, winget,
# the apt repository on GitHub Pages, Nix, FreeBSD and pkgsrc publish no
# counts, and AUR only votes and a popularity score.
set -u

ua="proc-cli-channel-stats (github.com/yazeed/proc-cli)"
# Retried, since pypistats answers 429 to quick repeats and Launchpad
# sometimes stalls (an answer takes under a second when it doesn't). A source
# that still fails shows as "(could not read)"
get() { curl -fsL --max-time 10 --retry 2 --retry-delay 5 -A "$ua" "$@"; }
# GitHub's REST API through gh when it is signed in (a higher rate limit),
# through curl otherwise
gh_get() {
    if gh auth status >/dev/null 2>&1; then gh api "$1"; else get "https://api.github.com/$1"; fi
}

# An empty value, or jq's null from an answer without the field, is a source
# that could not be read, never a 0
row() {
    value=${3:-}
    [ -n "$value" ] && [ "$value" != null ] || value="(could not read)"
    printf '%-14s %-40s %s\n' "$1" "$2" "$value"
}

echo "proc-cli usage, $(date -u '+%Y-%m-%d %H:%M UTC')"
echo

repo=$(gh_get repos/yazeed/proc-cli)
row GitHub stars "$(printf '%s' "$repo" | jq -r .stargazers_count)"
row GitHub forks "$(printf '%s' "$repo" | jq -r .forks_count)"
row GitHub watchers "$(printf '%s' "$repo" | jq -r .subscribers_count)"
releases=$(gh_get 'repos/yazeed/proc-cli/releases?per_page=100')
row GitHub "release-file downloads, all releases" \
    "$(printf '%s' "$releases" | jq '[.[].assets[].download_count] | add // 0')"
row GitHub "release-file downloads, $(printf '%s' "$releases" | jq -r '.[0].tag_name')" \
    "$(printf '%s' "$releases" | jq '[.[0].assets[].download_count] | add // 0')"
if gh api repos/yazeed/proc-cli/traffic/views >/dev/null 2>&1; then
    row GitHub "visits, last 14 days (unique)" \
        "$(gh api repos/yazeed/proc-cli/traffic/views --jq '"\(.count) (\(.uniques))"')"
    row GitHub "clones, last 14 days (unique)" \
        "$(gh api repos/yazeed/proc-cli/traffic/clones --jq '"\(.count) (\(.uniques))"')"
    gh api repos/yazeed/proc-cli/traffic/popular/referrers \
        --jq '.[:5][] | "\(.referrer)\t\(.count) (\(.uniques))"' |
        while IFS="$(printf '\t')" read -r from n; do
            row GitHub "visits from $from (unique)" "$n"
        done
else
    row GitHub "visits, clones, referrers" "(skipped: needs gh signed in with push access)"
fi

crate=$(get https://crates.io/api/v1/crates/proc-cli)
row crates.io "downloads, all time" "$(printf '%s' "$crate" | jq -r .crate.downloads)"
row crates.io "downloads, last 90 days" "$(printf '%s' "$crate" | jq -r .crate.recent_downloads)"

for period in day week month; do
    row npm "downloads, last $period" \
        "$(get "https://api.npmjs.org/downloads/point/last-$period/proc-cli" | jq -r .downloads)"
done

pypi=$(get https://pypistats.org/api/packages/proc-cli/recent)
for period in day week month; do
    row PyPI "downloads, last $period" "$(printf '%s' "$pypi" | jq -r ".data.last_$period")"
done

for image in proc-cli proc; do
    row "Docker Hub" "pulls, yazeed/$image" \
        "$(get "https://hub.docker.com/v2/repositories/yazeed/$image/" | jq -r .pull_count)"
done

# Launchpad counts each binary package (one per Ubuntu release, CPU and
# version) on its own; this adds them all, current and superseded
ppa=0 unread=0
links=$(get 'https://api.launchpad.net/1.0/~yazeedaloyoun/+archive/ubuntu/proc-cli?ws.op=getPublishedBinaries&ws.size=300' |
    jq -r '.entries[].self_link')
for link in $links; do
    if n=$(get "$link?ws.op=getDownloadCount"); then ppa=$((ppa + n)); else unread=$((unread + 1)); fi
done
[ "$unread" -eq 0 ] || ppa="$ppa ($unread packages could not be read)"
[ -n "$links" ] || ppa=""
row "Ubuntu PPA" "package downloads, all versions" "$ppa"

# COPR has no API for counts: its project page shows, beside each release,
# how many times that release's .repo file was downloaded
copr=$(get https://copr.fedorainfracloud.org/coprs/yazeed/proc-cli/ |
    sed -n 's/.*(\([0-9][0-9]*\) downloads\{0,1\}).*/\1/p' | awk '{ s += $1 } END { if (NR) print s }')
row COPR ".repo file downloads, all releases" "$copr"

get 'https://aur.archlinux.org/rpc/v5/info?arg[]=proc-cli&arg[]=proc-cli-bin' |
    jq -r '.results[] | "\(.Name)\t\(.NumVotes) votes, popularity \(.Popularity * 100 | round / 100)"' |
    while IFS="$(printf '\t')" read -r name n; do
        row AUR "$name" "$n"
    done

row skills.sh installs \
    "$(get https://www.skills.sh/yazeed/proc-cli/proc-cli | sed -n 's/.*"userInteractionCount":\([0-9]*\).*/\1/p' | head -n 1)"
