#!/usr/bin/env python3
"""Writes proc-cli's PyPI wheels for one release into OUTDIR:
  pkg/pypi/build-wheels.py VERSION OUTDIR DIR
- one wheel per release binary (five), each holding that binary as the `proc`
  command (`proc.exe` on Windows), so `pip install proc-cli`, `pipx install
  proc-cli` and `uv tool install proc-cli` put `proc` on PATH;
- DIR holds the five binaries under their artifact names (proc-linux-x86_64,
  proc-linux-aarch64, proc-darwin-x86_64, proc-darwin-aarch64,
  proc-windows-x86_64.exe), as `actions/download-artifact` with
  `merge-multiple` lays them out. It fails, naming the file, if one is missing.
Nothing is compiled: a wheel is a zip, so this repacks the binaries the release
already built and checked, as nfpm does for the .deb and .rpm (pkg/nfpm). The
release workflow runs it (package-pypi), and so can anyone with the files.
Python standard library only. The zips are reproducible: sorted entries, fixed
timestamps, deflate.
"""
import base64
import hashlib
import io
import sys
import zipfile
from pathlib import Path

NAME = "proc-cli"
DIST = "proc_cli"  # the name as it appears in file and directory names
SUMMARY = "Semantic CLI tool for process management. Target by port, PID, name or path."
# The same expression as the License: line of pkg/copr/proc-cli.spec: the
# binary holds proc (MIT) and the crates it is built from
LICENSE_EXPRESSION = (
    "MIT AND MPL-2.0 AND Unicode-3.0 AND (Apache-2.0 OR MIT) AND (Unlicense OR MIT)"
    " AND (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT)"
)
AUTHOR = "Yazeed Al Oyoun <me@yazeed.com>"
KEYWORDS = "process,port,kill,lsof,eaddrinuse,cli"
CLASSIFIERS = [
    "Environment :: Console",
    "Operating System :: MacOS",
    "Operating System :: POSIX :: Linux",
    "Operating System :: Microsoft :: Windows",
    "Programming Language :: Rust",
    "Topic :: System :: Systems Administration",
    "Topic :: Utilities",
]
PROJECT_URLS = [
    "Homepage, https://proc-cli.com",
    "Source, https://github.com/yazeed/proc-cli",
    "Issues, https://github.com/yazeed/proc-cli/issues",
    "Changelog, https://proc-cli.com/changelog",
]

# The lowest macOS each darwin binary runs on, as the v2.2.0 binaries say:
# `vtool -show-build` printed LC_VERSION_MIN_MACOSX version 10.12 for
# proc-darwin-x86_64 and LC_BUILD_VERSION minos 11.0 for proc-darwin-aarch64
MACOS_MIN_X86_64 = "10_12"
MACOS_MIN_ARM64 = "11_0"

# artifact name -> (platform tags the wheel claims, name of the command in the wheel)
# The file name carries the tags joined with dots; WHEEL lists them one by one.
LINUX_X86_64 = [
    "manylinux_2_17_x86_64",
    "manylinux2014_x86_64",
    "musllinux_1_1_x86_64",
]
LINUX_AARCH64 = [
    "manylinux_2_17_aarch64",
    "manylinux2014_aarch64",
    "musllinux_1_1_aarch64",
]
WHEELS = {
    "proc-linux-x86_64": (LINUX_X86_64, "proc"),
    "proc-linux-aarch64": (LINUX_AARCH64, "proc"),
    "proc-darwin-x86_64": ([f"macosx_{MACOS_MIN_X86_64}_x86_64"], "proc"),
    "proc-darwin-aarch64": ([f"macosx_{MACOS_MIN_ARM64}_arm64"], "proc"),
    "proc-windows-x86_64.exe": (["win_amd64"], "proc.exe"),
}

EPOCH = (1980, 1, 1, 0, 0, 0)
ROOT = Path(__file__).resolve().parent.parent.parent


def record_line(path: str, data: bytes) -> str:
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=")
    return f"{path},sha256={digest.decode()},{len(data)}"


def metadata(version: str) -> str:
    headers = [
        "Metadata-Version: 2.4",
        f"Name: {NAME}",
        f"Version: {version}",
        f"Summary: {SUMMARY}",
        f"Keywords: {KEYWORDS}",
        f"Author-email: {AUTHOR}",
        f"License-Expression: {LICENSE_EXPRESSION}",
        "License-File: LICENSE",
        *[f"Classifier: {c}" for c in CLASSIFIERS],
        *[f"Project-URL: {u}" for u in PROJECT_URLS],
        "Requires-Python: >=3.8",
        "Description-Content-Type: text/markdown",
    ]
    readme = (ROOT / "pkg/pypi/README.md").read_text(encoding="utf-8")
    return "\n".join(headers) + "\n\n" + readme


def wheel_file(tags: list) -> str:
    lines = [
        "Wheel-Version: 1.0",
        "Generator: proc-cli pkg/pypi/build-wheels.py",
        "Root-Is-Purelib: false",
        *[f"Tag: py3-none-{t}" for t in tags],
    ]
    return "\n".join(lines) + "\n"


def build(version: str, out: Path, artifact: Path, tags: list, command: str) -> Path:
    dist_info = f"{DIST}-{version}.dist-info"
    # name, content, unix mode; RECORD is last and lists every other file
    files = [
        (f"{DIST}-{version}.data/scripts/{command}", artifact.read_bytes(), 0o755),
        (f"{dist_info}/METADATA", metadata(version).encode("utf-8"), 0o644),
        (f"{dist_info}/WHEEL", wheel_file(tags).encode("utf-8"), 0o644),
        (f"{dist_info}/licenses/LICENSE", (ROOT / "LICENSE").read_bytes(), 0o644),
    ]
    files.sort(key=lambda f: f[0])
    record = [record_line(name, data) for name, data, _ in files]
    record.append(f"{dist_info}/RECORD,,")
    files.append((f"{dist_info}/RECORD", ("\n".join(record) + "\n").encode("utf-8"), 0o644))

    path = out / f"{DIST}-{version}-py3-none-{'.'.join(tags)}.whl"
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w", zipfile.ZIP_DEFLATED) as wheel:
        for name, data, mode in files:
            info = zipfile.ZipInfo(name, EPOCH)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.create_system = 3  # Unix, so the mode below is read back
            info.external_attr = (0o100000 | mode) << 16
            wheel.writestr(info, data)
    path.write_bytes(buffer.getvalue())
    return path


def main() -> int:
    if len(sys.argv) != 4:
        print("usage: build-wheels.py VERSION OUTDIR DIR", file=sys.stderr)
        return 2
    version, out, source = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3])
    missing = [name for name in WHEELS if not (source / name).is_file()]
    if missing:
        for name in missing:
            print(f"missing: {source / name}", file=sys.stderr)
        return 1
    out.mkdir(parents=True, exist_ok=True)
    for artifact, (tags, command) in WHEELS.items():
        print(f"wrote {build(version, out, source / artifact, tags, command)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
