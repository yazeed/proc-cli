#!/bin/sh
# Builds the Fedora source RPM of one release locally, to test the package:
#   pkg/copr/srpm.sh 2.2.0 [outdir]
# Needs rpm-build, cargo, curl and xz (on Fedora). Writes
# proc-cli-<version>-1.<dist>.src.rpm to outdir (default: the current
# directory). Releases don't need it: COPR builds each one itself
# (pkg/copr/copr-build.sh); both make their sources with sources.sh.
set -eu

version=${1:?usage: srpm.sh VERSION [OUTDIR]}
outdir=$(realpath "${2:-.}")
here=$(realpath "$(dirname "$0")")

# The sources stay in this temporary directory after the build.
work=$(mktemp -d)
echo "working in $work"
"$here/sources.sh" "$version" "$work/SOURCES" "$here/proc-cli.spec"
rpmbuild -bs --define "_topdir $work" --define "_srcrpmdir $outdir" "$work/SOURCES/proc-cli.spec"
