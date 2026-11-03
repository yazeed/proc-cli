# Contributing to proc

Thank you for your interest in contributing to proc! This document provides guidelines and instructions for contributing.

## Code of Conduct

By participating in this project, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

## How to Contribute

### Reporting Bugs

Before creating a bug report, please check the [existing issues](https://github.com/yazeed/proc-cli/issues) to see if the problem has already been reported.

When creating a bug report, please include:

1. **Environment details**:
   - Operating system and version
   - Rust version (`rustc --version`)
   - proc version (`proc --version`). A build from `main` prints `-dev (branch@commit, N commits after vX.Y.Z)` after the number, so include the whole line

2. **Steps to reproduce**:
   - What commands did you run?
   - What did you expect to happen?
   - What actually happened?

3. **Additional context**:
   - Error messages (full output)
   - Screenshots if applicable

### Suggesting Features

Feature suggestions are welcome! Please create an issue with:

1. A clear description of the feature
2. Use cases and examples
3. Any alternatives you've considered

### Pull Requests

1. **Fork the repository** and create your branch from `main`

2. **Set up the development environment**:
   ```bash
   git clone https://github.com/yazeed/proc-cli
   cd proc-cli
   cargo build
   ```

3. **Make your changes**:
   - Write clear, concise commit messages
   - Add tests for new functionality
   - Update documentation as needed

4. **Run the test suite**:
   ```bash
   cargo test
   cargo fmt -- --check
   cargo clippy --all-targets -- -D warnings
   ```

5. **Submit a pull request**:
   - Provide a clear description of the changes
   - Reference any related issues as `(#123)`. A fixed issue is closed when the release that contains the fix ships
   - Leave **Allow edits by maintainers** on, so small fixes (a conflict, a CHANGELOG line) can go straight onto your branch
   - PRs are merged with **Rebase and merge**, so your commits land on `main` with you as their author

## Development Setup

### Prerequisites

- Rust 1.88 or later (the sysinfo dependency requires it). Work on the latest stable (`rustup update stable`): CI lints and tests with it, and an older toolchain can miss lints CI reports
- Git

### Building

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run tests
cargo test

# Run with verbose output
cargo test -- --nocapture
```

### Code Style

We follow standard Rust conventions:

```bash
# Format code
cargo fmt

# Check formatting
cargo fmt -- --check

# Run linter (tests included, as CI runs it)
cargo clippy --all-targets -- -D warnings
```

### Project Structure

```
proc/
├── src/
│   ├── main.rs           # CLI entry point, routing, JSON errors, exit codes
│   ├── cli.rs            # clap definitions (Cli, Commands)
│   ├── lib.rs            # Library exports
│   ├── error.rs          # Error types and exit codes
│   ├── test_support.rs   # Helpers shared by tests
│   ├── commands/         # One file per command
│   │   ├── mod.rs
│   │   ├── by.rs, find_in.rs, for_file.rs, list.rs, on.rs, ports.rs
│   │   ├── held.rs       # What on, info, for and why share: ports, connections, the card
│   │   ├── info.rs, tree.rs, why.rs, stuck.rs, orphans.rs, watch.rs, wait.rs
│   │   └── kill.rs, stop.rs, free.rs, freeze.rs, thaw.rs, unstick.rs
│   ├── core/             # Core abstractions
│   │   ├── mod.rs
│   │   ├── apply.rs      # Run an action on each target
│   │   ├── connections.rs # Connections to a listening port: counts, states, peers
│   │   ├── filters.rs    # Shared filter utilities
│   │   ├── name_match.rs # Exact name vs --cmd matching
│   │   ├── port.rs
│   │   ├── process.rs
│   │   └── target.rs     # Target parsing and resolution
│   └── ui/               # Output formatting
│       ├── mod.rs
│       ├── card.rs       # The process card on, info, for and why print
│       ├── format.rs
│       ├── output.rs
│       └── table.rs      # The one process table
├── tests/                # Integration tests
├── scripts/              # Coverage tooling
├── Cargo.toml
└── README.md
```

## Areas to Contribute

### Good First Issues

Look for issues labeled `good first issue` - these are great starting points:

- Documentation improvements
- Error message enhancements
- Test coverage improvements

### Medium Complexity

- Platform-specific bug fixes
- Performance optimizations
- Output formatting improvements

### Advanced

- New commands
- Cross-platform compatibility
- Advanced features (watch mode, process trees)

## Testing

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_find_all_processes

# Run tests with output
cargo test -- --nocapture
```

### Writing Tests

- Unit tests go in the same file as the code being tested
- Integration tests go in the `tests/` directory
- Use descriptive test names

Example:
```rust
#[test]
fn test_find_process_by_name() {
    let processes = Process::find_by_name("cargo", NameMatch::Exact).unwrap();
    assert!(!processes.is_empty());
}
```

### What a change is tested for

Line coverage says a line ran, not that its behaviour is pinned. A change that adds or alters a command, flag, argument or output gets tests for the cases people and agents meet, in the suite that holds that kind of case:

- **Behaviour through the real binary:** the command's own file (`cmd_actions.rs`, `cmd_find.rs`, `cmd_free.rs`, `cmd_inspect.rs`, `cmd_list.rs`, `cmd_ports.rs`, `cmd_watch.rs`), with every target form it takes (`:port`, PID, name, several targets) and every filter (`--in`, `--by`, `--start-time`)
- **A new flag:** a case in the flag matrix (`tests/flags.rs`); `every_flag_is_exercised` fails until there is one
- **JSON:** one document, `action`, `success`, exit codes and `reason` (`cli_json.rs`, and the checker in `tests/contract/` that runs on every JSON document a test reads); `not_found` for targets that match nothing (`cmd_not_found.rs`)
- **Bad input:** missing or bad values, unknown flags, empty arguments (`parser_errors.rs`, generated from the CLI definition), and inputs that used to look like an answer (`silent_failures.rs`)
- **Pipes and closed output:** stdout or stderr that can't take the bytes, a reader that stops early (`hostile_io.rs`)
- **A terminal, or none:** confirmation prompts and the `watch` screen through a pseudo-terminal (`terminal.rs`); the same command without a terminal must exit 4, not ask. On Unix proc runs there as a shell's foreground job, through `examples/session`, so its exit doesn't revoke the terminal before the test has read it (FreeBSD and NetBSD drop unread output then)
- **Ordering and several holders:** stable output order (`order.rs`), a port held over IPv4 and IPv6 at once (`cmd_port_holders.rs`)
- **Anything that waits:** `timing.rs`, so a timeout or a wait that never returns fails fast
- **Docs:** examples never pair `--yes` with a name (`doc_examples.rs`)

Tests aim proc only at processes they started themselves (a copy of `examples/listener.rs` under its own name in a fresh temp directory, or a shell the test owns), so the suite can run on a machine that is busy with other work.

### Nix

`nix flake check -L` builds proc from `Cargo.lock` and runs this whole suite in Nix's build sandbox, on Linux and macOS in CI. The sandbox has no real `$HOME`, a bare `PATH`, its own process namespace, and the system's `coreutils` may be one multi-call binary, so a test must not assume a home directory, a tool the machine happens to have, that `sleep` runs under another name, or that the machine has more than one process tree. `flake.nix` sets `$HOME` for the terminal tests and adds the tools the tests call, since the `PATH` has neither `/bin` nor `/usr/sbin` (`procps` on Linux; Apple's `ps`, `darwin.ps`, and `lsof` on macOS, where proc reads ports with `lsof`); it never turns the tests off. A test the sandbox must stop is skipped by exact name in `checkFlags`, with the reason beside it. Without Nix installed, run the check in the `nixos/nix` Docker image (`--privileged` for the sandbox, and `NIX_CONFIG="experimental-features = nix-command flakes"`).

### FreeBSD and NetBSD

GitHub has no BSD runners, so the `bsd` job in CI boots FreeBSD 15.1 and NetBSD 11.0 virtual machines on a Linux runner (`vmactions/freebsd-vm`, `vmactions/netbsd-vm`) and runs the whole suite in each, with only Rust installed (ports come from `sockstat` and open files from `fstat`, both in the base system). The same job then builds the release binary natively with `scripts/bsd-build.sh` and runs it; release.yml's `build-bsd` job does the same for a tag. They aren't cross-compiled: cross's FreeBSD image has no `libgeom` and its NetBSD image no `libkvm` or `libexecinfo`, which proc links. Neither OS is in the coverage merge, so code that only exists on a BSD is checked by that job's tests alone. Nix's macOS sandbox is another environment the plain macOS jobs don't have: it refuses local network listeners without `__darwinAllowLocalNetworking` and can't read `/etc/passwd`, so the CI Nix job on macOS runs with `sandbox = relaxed`, as nixpkgs reviewers build.

Two rules keep proc correct there:

- **Every sysinfo refresh goes through `sysinfo_locked`** (`src/core/process.rs`), or `refreshed_system()` for a full snapshot. FreeBSD's libkvm reads command lines into buffers shared by the whole program, so two threads refreshing at once crash proc (the tests run in parallel threads and found it). The lock also points stderr at `/dev/null` while it refreshes, because libprocstat prints a warning there for every process that exits mid-read.
- **CPU time comes from `cpu_time_ms`**, never from sysinfo's `accumulated_cpu_time` directly. On NetBSD sysinfo counts it in hundredths of a second and rounds it to whole seconds, so a process spinning at 100% read 0% over a 0.2 s window; `cpu_time_ms` reads the kernel's counter there.

Four tests of a deleted working directory are ignored on both, with the reason in the attribute: once a directory is deleted, FreeBSD shows `-` for the path and NetBSD none at all, so `proc in` has nothing to match.

### Coverage

CI fails a change that lowers line coverage or leaves a flag without a test. The `Coverage gate` job checks:

- 100% of the lines in `src/`, merged across Linux, macOS and Windows. A line counts as covered if it ran on any of the three, so code that only exists on one OS is measured on that OS
- Every long and short flag of every process command is exercised: `every_flag_is_exercised` in `tests/flags.rs` walks the CLI definition and fails for a long flag, or a short flag (`-j`), that no test spells. It skips `completions` and `manpage`, `--help`/`-h` and `--version`/`-V`
- All three lcov files are present. If one OS's coverage job fails, the gate fails instead of judging a partial merge
- No file in `src/` that contains code has no coverage data, which would mean it was never compiled or linked

When the gate is given several lcov files (as in CI), it prints each file's own total right after the merged `TOTAL` line, labelled by file name (`lcov-windows-latest.info`, `lcov-macos-latest.info`, ...). With one file it prints no extra lines. Those totals are for information only and decide nothing: the merged 100% is the only rule. The percentage is cut to one decimal, not rounded, so an OS that misses a single line never reads as 100.0%. A low total shows that an OS runs far fewer lines than the others, which is worth a test on that OS, not an exclusion.

To measure locally:

```bash
scripts/coverage-local.sh
```

It runs the suite in a Linux container at your machine's architecture and prints the same per-file report, listing every uncovered line. It covers Linux only: lines that exist only on macOS or Windows show as uncovered locally and are measured in CI.

Lines that cannot be exercised go in `coverage-exclusions.txt`, one `src/path.rs:START-END  reason` per line. An entry needs a reason that says why the line cannot run, and the maintainer's approval before it is added. An entry that hides no uncovered line is stale and fails the gate, so delete it when the code it covered gets a test.

## Commit Messages

Follow conventional commit format:

```
type(scope): description

[optional body]

[optional footer]
```

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting)
- `refactor`: Code refactoring
- `test`: Adding or updating tests
- `chore`: Maintenance tasks

Examples:
```
feat(commands): add proc watch command
fix(ports): handle IPv6 addresses correctly
docs: update installation instructions
```

## Release Process

Releases are managed by maintainers and follow [RELEASE.md](RELEASE.md).

## Questions?

- Open a [Discussion](https://github.com/yazeed/proc-cli/discussions) for questions
- Reach out to maintainers

Thank you for contributing to proc!
