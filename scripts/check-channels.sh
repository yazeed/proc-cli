#!/bin/sh
# Every place proc is published, the version each one offers now, and its
# public page: the one list of proc's channels (RELEASE.md's package table
# links the same pages, and a docs test keeps the two in step).
#
#   scripts/check-channels.sh [VERSION]      (default: Cargo.toml's version)
#
# Each line: status, channel, what it offers, its page. A "release" channel is
# one the release run publishes itself; once the run is green it must offer
# VERSION, and any that doesn't makes the exit status 1. An "outside" channel
# waits on someone else (a reviewer, a committer, a community maintainer, an
# update bot): it is listed as "waiting" until it catches up, which doesn't
# fail the check. Reads public pages and APIs only, with curl and jq; nothing
# is written. RELEASE.md step 7 runs it.
set -u

here=$(cd "$(dirname "$0")/.." && pwd)
version=${1:-$(sed -n 's/^version = "\(.*\)"$/\1/p' "$here/Cargo.toml" | head -n 1)}
ua="proc-cli-channel-check (github.com/yazeed/proc-cli)"

# Each channel's public page, where it shows the version it offers. RELEASE.md's
# package table lists the same pages; tests/docs_coverage.rs checks each one is there
page_github='https://github.com/yazeed/proc-cli/releases'
page_crates='https://crates.io/crates/proc-cli'
page_docsrs='https://docs.rs/proc-cli'
page_npm='https://www.npmjs.com/package/proc-cli'
page_pypi='https://pypi.org/project/proc-cli/'
page_docker='https://hub.docker.com/r/yazeed/proc-cli'
page_docker_alias='https://hub.docker.com/r/yazeed/proc'
page_homebrew='https://github.com/yazeed/homebrew-proc'
page_scoop='https://github.com/yazeed/scoop-bucket-proc'
page_apt='https://proc-cli.com/apt/'
page_ppa='https://launchpad.net/~yazeedaloyoun/+archive/ubuntu/proc-cli'
page_copr='https://copr.fedorainfracloud.org/coprs/yazeed/proc-cli/'
page_plugin='https://github.com/yazeed/proc-cli/blob/main/.claude-plugin/marketplace.json'
page_site='https://proc-cli.com/install'
page_aur='https://aur.archlinux.org/packages/proc-cli'
page_aur_bin='https://aur.archlinux.org/packages/proc-cli-bin'
page_nixpkgs='https://search.nixos.org/packages?channel=unstable&show=proc-cli&query=proc-cli'
page_winget_pr='https://github.com/microsoft/winget-pkgs/pull/469298'
page_winget='https://github.com/microsoft/winget-pkgs/tree/master/manifests/y/yazeed/proc-cli'
page_freebsd_bug='https://bugs.freebsd.org/bugzilla/show_bug.cgi?id=300492'
page_freebsd='https://www.freshports.org/sysutils/proc-cli/'
page_pkgsrc='https://pkgsrc.se/sysutils/proc-cli'
page_skill='https://www.skills.sh/yazeed/proc-cli/proc-cli'
failed=0

get() { curl -fsSL --max-time 30 -A "$ua" "$@"; }
code() { curl -sL -o /dev/null --max-time 30 -A "$ua" -w '%{http_code}' "$1"; }

# report KIND NAME FOUND PAGE [WAITING-ON]
report() {
    kind=$1 name=$2 found=$3 page=$4 waiting=${5:-}
    if [ "$found" = "$version" ]; then
        status=ok
    elif [ "$kind" = outside ]; then
        status=waiting
        [ -n "$waiting" ] && found="$found (waiting on $waiting)"
    else
        status=BEHIND
        failed=1
    fi
    [ -n "$found" ] || found="(could not read)"
    printf '%-8s %-24s %-44s %s\n' "$status" "$name" "$found" "$page"
}

# The ones the release run publishes
report release "GitHub release" \
    "$(get https://api.github.com/repos/yazeed/proc-cli/releases/latest | jq -r '.tag_name | ltrimstr("v")')" \
    "$page_github"
report release crates.io \
    "$(get https://crates.io/api/v1/crates/proc-cli | jq -r .crate.max_version)" \
    "$page_crates"
docs=$([ "$(code "https://docs.rs/proc-cli/$version/proc_cli/")" = 200 ] && echo "$version" || echo "not built yet")
report release docs.rs "$docs" "$page_docsrs"
report release npm \
    "$(get https://registry.npmjs.org/proc-cli/latest | jq -r .version)" \
    "$page_npm"
report release PyPI \
    "$(get https://pypi.org/pypi/proc-cli/json | jq -r .info.version)" \
    "$page_pypi"
for image in proc-cli proc; do
    tag=$(get "https://hub.docker.com/v2/repositories/yazeed/$image/tags/$version" 2>/dev/null | jq -r '.name // empty')
    if [ "$image" = proc-cli ]; then page=$page_docker; else page=$page_docker_alias; fi
    report release "Docker yazeed/$image" "${tag:-no $version tag}" "$page"
done
report release Homebrew \
    "$(get https://raw.githubusercontent.com/yazeed/homebrew-proc/main/Formula/proc.rb | sed -n 's/^ *version "\(.*\)"$/\1/p' | head -n 1)" \
    "$page_homebrew"
report release Scoop \
    "$(get https://raw.githubusercontent.com/yazeed/scoop-bucket-proc/master/bucket/proc.json | jq -r .version)" \
    "$page_scoop"
apt=$version
for arch in amd64 arm64; do
    get "https://proc-cli.com/apt/dists/stable/main/binary-$arch/Packages" | grep -qx "Version: $version" || apt="no $version for $arch"
done
report release "apt repository" "$apt" "$page_apt"
# Every series' published source must be this version (an older one still
# published means a series hasn't caught up)
ppa=$(get 'https://api.launchpad.net/1.0/~yazeedaloyoun/+archive/ubuntu/proc-cli?ws.op=getPublishedSources&source_name=proc-cli&status=Published' |
    jq -r '[.entries[].source_package_version | split("-")[0]] | unique | join(" ")')
report release "Ubuntu PPA" "$ppa" "$page_ppa"
copr=$(get 'https://copr.fedorainfracloud.org/api_3/package/?ownername=yazeed&projectname=proc-cli&packagename=proc-cli&with_latest_build=true' |
    jq -r '.builds.latest | if .state == "succeeded" then (.source_package.version | split("-")[0]) else "\(.source_package.version) \(.state)" end')
report release COPR "$copr" "$page_copr"
report release "Claude plugin" \
    "$(get https://raw.githubusercontent.com/yazeed/proc-cli/main/.claude-plugin/marketplace.json | jq -r .metadata.version)" \
    "$page_plugin"
site=$(get https://proc-cli.com/install | grep -q "$version" && echo "$version" || echo "older")
report release "proc-cli.com" "$site" "$page_site"

# The ones that wait on someone else
for p in proc-cli proc-cli-bin; do
    report outside "AUR $p" \
        "$(get "https://aur.archlinux.org/rpc/v5/info?arg[]=$p" | jq -r '.results[0].Version | split("-")[0]')" \
        "$(if [ "$p" = proc-cli ]; then echo "$page_aur"; else echo "$page_aur_bin"; fi)" "its community maintainer"
done
report outside "nixpkgs (unstable)" \
    "$(get https://raw.githubusercontent.com/NixOS/nixpkgs/nixpkgs-unstable/pkgs/by-name/pr/proc-cli/package.nix | sed -n 's/^  version = "\(.*\)";$/\1/p')" \
    "$page_nixpkgs" \
    "the update bot's pull request, merged by the maintainer, then the channel"
# winget: the version folder once merged, the first pull request's title until then
if [ "$(code "https://github.com/microsoft/winget-pkgs/tree/master/manifests/y/yazeed/proc-cli/$version")" = 200 ]; then
    report outside winget "$version" "$page_winget"
else
    pr=$(get https://api.github.com/repos/microsoft/winget-pkgs/pulls/469298 | jq -r '.title | sub("^New package: yazeed.proc-cli version "; "")')
    report outside winget "$pr in the pull request" "$page_winget_pr" "a moderator"
fi
# FreeBSD: the ports tree once committed, the bug's current patch until then
if [ "$(code https://cgit.freebsd.org/ports/plain/sysutils/proc-cli/Makefile)" = 200 ]; then
    report outside "FreeBSD port" \
        "$(get https://cgit.freebsd.org/ports/plain/sysutils/proc-cli/Makefile | sed -n 's/^DISTVERSION=[[:space:]]*//p')" \
        "$page_freebsd" "a committer"
else
    patch=$(get 'https://bugs.freebsd.org/bugzilla/rest/bug/300492/attachment?include_fields=file_name,is_obsolete' |
        jq -r '[.bugs["300492"][] | select(.is_obsolete == 0) | .file_name][-1] // "" | capture("proc-cli-(?<v>[0-9.]+)-").v // ""')
    report outside "FreeBSD port" "$patch on bug 300492" \
        "$page_freebsd_bug" "a ports committer"
fi
report outside pkgsrc \
    "$(get https://raw.githubusercontent.com/NetBSD/pkgsrc/trunk/sysutils/proc-cli/Makefile | sed -n 's/^DISTNAME=[[:space:]]*proc-cli-//p')" \
    "$page_pkgsrc" "a pkgsrc developer to commit the update"
# skills.sh keeps its own copy of SKILL.md: current once it matches the tag's
stored=$(get https://skills.sh/api/download/yazeed/proc-cli/proc-cli | jq -r '.files[] | select(.path == "SKILL.md") | .contents')
tagged=$(get "https://raw.githubusercontent.com/yazeed/proc-cli/v$version/skills/proc-cli/SKILL.md")
if [ -n "$stored" ] && [ "$(printf '%s' "$stored" | sed -e :a -e '/^\n*$/{$d;N;ba' -e '}')" = "$(printf '%s' "$tagged" | sed -e :a -e '/^\n*$/{$d;N;ba' -e '}')" ]; then
    skill=$version
else
    skill="an older SKILL.md"
fi
report outside skills.sh "$skill" "$page_skill" \
    "a refresh (install it again the next day, RELEASE.md step 7)"

exit "$failed"
