#!/usr/bin/env bash
# Local Linux line-coverage run for proc, using Rust's built-in instrumentation
# (no third-party binaries are downloaded). The suite runs in a rust:1-bookworm
# container with the repo mounted read-only; build output, profiles and the
# lcov file go to OUT_DIR, which is outside the repo. Then the coverage gate
# prints the per-file report.
#
# It builds as cargo-llvm-cov does in CI (`--cfg coverage`), so a process that a
# signal ends still writes its profile. It refuses to start with less than
# 10 GB free where OUT_DIR is, and on every exit it leaves only lcov.info and
# the logs in OUT_DIR: the build, crates and profiles (about 8 GB) are removed,
# and so is the rust image if this run pulled it.
#
# Usage: scripts/coverage-local.sh [--enforce --require NAME]
#   (--enforce needs a --require; this run produces one lcov file, lcov.info)
#   OUT_DIR   where to write target/, profiles/ and lcov.info
#             (default: ${TMPDIR:-/tmp}/proc-coverage)
#   PLATFORM  docker --platform value (default: the host's, so arm64 on Apple silicon)
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="${OUT_DIR:-${TMPDIR:-/tmp}/proc-coverage}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"

case "$out" in
  "$repo"|"$repo"/*) echo "OUT_DIR must be outside the repo ($repo)" >&2; exit 1 ;;
esac

# A run writes about 8 GB to OUT_DIR; stop before starting rather than fill
# the disk halfway through
need_gb=10
free_kb="$(df -Pk "$out" | awk 'NR == 2 { print $4 }')"
if [ "$free_kb" -lt $((need_gb * 1024 * 1024)) ]; then
  echo "Not enough disk space: $((free_kb / 1024 / 1024)) GB free in $out, a run needs $need_gb GB" >&2
  exit 1
fi

image=rust:1-bookworm
had_image=0
docker image inspect "$image" >/dev/null 2>&1 && had_image=1

cid_file="$out/container.id"
rm -f "$cid_file"
cleanup() {
  if [ -s "$cid_file" ]; then
    docker rm -f "$(cat "$cid_file")" >/dev/null 2>&1 || true
  fi
  rm -f "$cid_file"
  # the container removes these itself as root (on a Linux host they are
  # root's); this catches a run that stopped early
  rm -rf "${out:?}/target" "${out:?}/cargo-home" "${out:?}/profiles" \
    "${out:?}/timing-profiles" "${out:?}/coverage.profdata" 2>/dev/null || true
  if [ "$had_image" -eq 0 ]; then
    docker rmi "$image" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

rm -rf "$out/profiles" "$out/timing-profiles"
rm -f "$out/lcov.info"
mkdir -p "$out/profiles" "$out/timing-profiles"

# Runs inside the container. $OUT is the mounted output dir.
inner='
set -euo pipefail
export CARGO_TARGET_DIR=/out/target CARGO_HOME=/out/cargo-home
# --cfg coverage as cargo-llvm-cov sets it: a process a signal ends then writes
# its profile first (src/ui/interrupt.rs, write_coverage)
export RUSTFLAGS="-C instrument-coverage --cfg coverage"
rustup component add llvm-tools-preview >/dev/null 2>&1
sysroot="$(rustc --print sysroot)"
bin="$(find "$sysroot" -name llvm-profdata -type f | head -1)"
bin="$(dirname "$bin")"
# Same dependency paths coverage_gate.py ignores, whatever CARGO_HOME is
cd /work

export LLVM_PROFILE_FILE=/out/profiles/%p-%m.profraw
# Keep going after a failing test binary so the report still covers everything
set +e
nice -n 10 cargo test -j 4 --no-fail-fast -- --test-threads=4 2>&1 | tee /out/test.log
test_status=${PIPESTATUS[0]}
set -e

"$bin/llvm-profdata" merge -sparse /out/profiles/*.profraw -o /out/coverage.profdata
objects=()
# Test programs: target/debug/deps up to Rust 1.99,
# target/debug/build/proc-cli/<hash>/out from 1.100 (beside the build script)
for f in /out/target/debug/deps/* /out/target/debug/build/proc-cli/*/out/*; do
  [ -f "$f" ] && [ -x "$f" ] || continue
  case "$(basename "$f")" in *.*|build_script_build*) continue ;; esac
  objects+=(-object "$f")
done
objects+=(-object /out/target/debug/proc)
"$bin/llvm-cov" export -format=lcov -instr-profile=/out/coverage.profdata \
  -ignore-filename-regex="(/registry/src/|/git/checkouts/|/rustc/)" "${objects[@]}" > /out/lcov.info

# Timing tests, one at a time, from the same instrumented build.
export LLVM_PROFILE_FILE=/out/timing-profiles/%p-%m.profraw
timing_bin="$( (ls -t /out/target/debug/deps/timing-* /out/target/debug/build/proc-cli/*/out/timing-* 2>/dev/null || true) | grep -v "\\." | head -1)"
echo "== timing tests under instrumentation =="
for t in $("$timing_bin" --list 2>/dev/null | sed -n "s/: test$//p"); do
  res="$("$timing_bin" --exact "$t" --test-threads=1 2>&1 | grep -E "^test result" || true)"
  echo "$t: $res"
done | tee /out/timing.log
# Keep lcov.info and the logs; the rest is about 8 GB
rm -rf /out/target /out/cargo-home /out/profiles /out/timing-profiles /out/coverage.profdata
exit "$test_status"
'

# Native architecture by default: under linux/amd64 emulation on Apple silicon,
# /proc/<pid>/exe points at /run/rosetta/rosetta and `proc for <exe>` tests fail.
case "$(uname -m)" in
  x86_64) host_platform=linux/amd64 ;;
  *) host_platform=linux/arm64 ;;
esac
platform_args=(--platform "${PLATFORM:-$host_platform}")

set +e
docker run --rm --cidfile "$cid_file" --cpus 4 ${platform_args[@]+"${platform_args[@]}"} \
  -v "$repo":/work:ro -v "$out":/out \
  "$image" bash -c "$inner"
test_status=$?
set -e

echo
echo "lcov written to $out/lcov.info"
set +e
nice -n 10 uv run python "$repo/scripts/coverage_gate.py" "$out/lcov.info" \
  --exclusions "$repo/coverage-exclusions.txt" "$@"
gate_status=$?

if [ "$test_status" -ne 0 ]; then
  echo "WARNING: the test run exited $test_status (see $out/test.log)" >&2
fi
exit "$gate_status"
