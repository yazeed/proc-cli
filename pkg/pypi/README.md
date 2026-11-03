# proc-cli

Semantic CLI tool for process management. Target by port, PID, name or path.

## Install

```bash
pip install proc-cli
pipx install proc-cli
uv tool install proc-cli
```

## Usage

```bash
proc on :3000           # what's on port 3000?
proc on 1234            # what is PID 1234, and what ports is it using?
proc kill :3000 :8080   # kill multiple targets
proc by node --in .     # node processes in current directory
```

## Documentation

**[Full documentation on GitHub](https://github.com/yazeed/proc-cli)**

## About

The wheel holds the native `proc` binary for your platform, the same file as the one on the GitHub release, written in Rust. No Python code runs: `proc` is the binary itself, and pip puts it on your PATH.

Wheels are published for macOS (Apple silicon and Intel), Linux (x86_64 and ARM64, glibc and musl, so Alpine too) and Windows (x86_64). On any other platform pip finds no wheel; FreeBSD and NetBSD users take the binaries from the [releases page](https://github.com/yazeed/proc-cli/releases).

## License

MIT
