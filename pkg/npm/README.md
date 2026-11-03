# proc-cli

Semantic CLI tool for process management. Target by port, PID, name or path.

## Install

```bash
npm install -g proc-cli
yarn global add proc-cli
pnpm add -g proc-cli
bun install -g proc-cli
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

This package downloads the native `proc` binary for your platform from the GitHub release and verifies its SHA-256 checksum. The binary is written in Rust for maximum performance.

The package has no install script. The download happens the first time you run `proc`, so that run needs network access. To download ahead of time, for example in an image that will run offline, run `node install.js` in the package directory. If the package directory isn't writable, the binary is stored in `~/.cache/proc-cli/<version>` (`%LOCALAPPDATA%\proc-cli\<version>` on Windows).

The `proc` command ends the way the binary did: with its exit code, or, when a signal ended it, by the same signal, so a shell and a script see what they would without npm.

## License

MIT
