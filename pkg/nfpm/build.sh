#!/usr/bin/env bash
# Builds proc-cli_<version>_<arch>.deb and proc-cli-<version>-1.<rpm arch>.rpm
# from a static Linux binary, using nfpm (https://nfpm.goreleaser.com) from PATH.
#
# usage: build.sh <version> <arch: amd64|arm64> <proc binary> <docs dir> <output dir>
#   version    1.2.3, without the leading v
#   docs dir   what generate-docs.sh wrote
set -euo pipefail

version=$1
arch=$2
binary=$3
docs=$4
out=$5

case "$arch" in
  amd64 | arm64) ;;
  *) echo "arch must be amd64 or arm64, got '$arch'" >&2; exit 1 ;;
esac

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
mkdir -p "$out"
out=$(cd "$out" && pwd)

# nfpm.yaml names its input files through these variables
bin_dir=$(cd "$(dirname "$binary")" && pwd)
docs_dir=$(cd "$docs" && pwd)
bin_file=$bin_dir/$(basename "$binary")
export NFPM_ARCH="$arch" NFPM_VERSION="$version" NFPM_ROOT="$root"
export NFPM_BIN="$bin_file" NFPM_DOCS="$docs_dir"
for format in deb rpm; do
  nfpm package --config "$here/nfpm.yaml" --packager "$format" --target "$out/"
done
