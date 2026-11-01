# CLAUDE.md

Project-specific instructions for Claude Code.

## Working Style

**Always ask before committing.** Show the diff or summary and wait for explicit approval before running `git commit`.

**Always ask before pushing.** Never run `git push` or `git push --tags` without explicit user approval. This applies to every push — main branch, tags, all of it.

**Always ask before tagging.** Never run `git tag` without explicit user approval. Tags trigger CI releases to crates.io, npm, Homebrew, Scoop, and Docker — they cannot be easily undone.

**Ask before destructive actions.** This includes `git checkout`, `git reset`, file deletions, or anything that can't be easily undone.

**Show work before publishing.** Before running release workflows, publishing to package managers, or pushing tags, summarize what will happen and wait for approval.

**ALWAYS follow the RELEASE.md checklist for every version.** No exceptions — not for patch versions, not for "trivial" fixes. Every release must complete all steps: pre-release checks (fmt, clippy, test, build, update, audit), manual testing, version bumps, documentation review. Run the checklist, show the results, then ask for approval to commit/push/tag.

## Project Overview

proc is a semantic CLI tool for process management written in Rust. It provides a unified interface for process and port operations across macOS, Linux, Windows, FreeBSD and NetBSD.

**Tagline (use exactly):** "Semantic CLI tool for process management. Target by port, PID, name or path."

**Repository:** https://github.com/yazeed/proc-cli

## Architecture

```
src/
├── main.rs           # CLI entry point, routing, JSON errors, exit codes
├── cli.rs            # clap definitions (Cli, Commands)
├── test_support.rs   # Helpers shared by tests
├── lib.rs            # Library exports
├── error.rs          # Error types and exit codes
├── commands/         # One file per command
│   ├── mod.rs        # Exports all commands
│   ├── kill.rs       # proc kill
│   ├── stop.rs       # proc stop (supports --signal)
│   ├── on.rs         # proc on (bidirectional port/process lookup)
│   ├── by.rs         # proc by (filter by name)
│   ├── find_in.rs    # proc in (filter by directory, named find_in to avoid Rust keyword)
│   ├── for_file.rs   # proc for (what has a file open)
│   ├── list.rs       # proc list
│   ├── info.rs       # proc info
│   ├── ports.rs      # proc ports
│   ├── tree.rs       # proc tree
│   ├── stuck.rs      # proc stuck
│   ├── unstick.rs    # proc unstick
│   ├── freeze.rs     # proc freeze (SIGSTOP)
│   ├── thaw.rs       # proc thaw (SIGCONT)
│   ├── orphans.rs    # proc orphans
│   ├── why.rs        # proc why (ancestry tracing)
│   ├── free.rs       # proc free (kill + verify port freed)
│   ├── wait.rs       # proc wait (block until exit)
│   └── watch.rs      # proc watch (real-time monitoring)
├── core/             # Shared logic
│   ├── mod.rs        # Exports
│   ├── apply.rs      # apply_to_each (run an action on each process)
│   ├── filters.rs    # Shared filter utilities (resolve_in_dir)
│   ├── name_match.rs # How a name target and --by match a process (exact by default, --cmd for substring)
│   ├── target.rs     # Target parsing (:port, PID, name) and resolution
│   ├── process.rs    # Process operations
│   └── port.rs       # Port operations
└── ui/               # Output formatting
    ├── mod.rs
    ├── format.rs     # Shared formatting (format_duration, truncate_*, colorize_status)
    └── output.rs     # Printer, OutputFormat (Human/Json)
```

## Code Style

### Command Pattern

Each command follows this structure:

```rust
//! `proc <cmd>` - Brief description
//!
//! Examples:
//!   proc cmd target          # What it does
//!   proc cmd :3000           # Port example
//!   proc cmd --flag          # Flag example

use crate::core::{flatten_targets, resolve_targets_excluding_self, NameMatch, ...};
use crate::error::{ProcError, Result};
use crate::ui::Printer;
use clap::Args;

/// Brief doc for --help
#[derive(Args, Debug)]
pub struct CmdCommand {
    /// Target(s): process name, PID, or :port (separate arguments for several)
    #[arg(required = true, num_args = 1.., value_parser = crate::core::validate_target_arg)]
    pub target: Vec<String>,

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Output as JSON
    #[arg(long, short = 'j')]
    pub json: bool,

    /// Show verbose output
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// Match names as a part of the name or command line
    #[arg(long)]
    pub cmd: bool,
}

impl CmdCommand {
    pub fn execute(&self) -> Result<()> {
        // 1. Setup printer
        let printer = Printer::from_flags(self.json, self.verbose);

        // 2. Flatten and resolve targets (see src/commands/kill.rs for the full version)
        let targets = flatten_targets(&self.target);
        let mode = NameMatch::from_flag(self.cmd);
        let resolved = resolve_targets_excluding_self(&targets, mode);

        // 3. Warn about not-found targets, and put them in the JSON results
        if let Some(warning) = resolved.warning() {
            printer.warning(&warning);
        }
        let printer = printer.with_not_found(&resolved.not_found);
        let processes = resolved.processes;

        // 4. Handle dry-run if applicable
        // 5. Confirm destructive actions: `if !printer.ask_confirm("cmd", &processes, self.yes)? { return Ok(()); }`
        // 6. Execute operation
        // 7. Print results

        Ok(())
    }
}
```

### Conventions

- **Unified targets**: `:port`, `PID`, `name` work everywhere
- **Multi-target**: Separate arguments (`:3000 :8080 node`); comma lists still work but are deprecated. Deduplicated by PID
- **Confirmation**: Destructive commands need `--yes` or an interactive yes. `--json` and a missing terminal are not consent: they exit 4 (`needs_confirmation`). `--dry-run` previews without `--yes`
- **Dry-run**: Destructive commands support `--dry-run`
- **JSON output**: All commands support `--json` for scripting
- **Short flags**: `-y` (yes), `-j` (json), `-v` (verbose)

### Shared Utilities

Use the shared modules instead of writing local helpers:

- **`crate::core::resolve_in_dir`** — resolves `--in` directory paths (handles `.`, relative, absolute)
- **`crate::ui::format_duration`** — formats seconds as human-readable duration
- **`crate::ui::truncate_string`** / **`truncate_path`** — truncates with `…`
- **`crate::ui::colorize_status`** — colors process status strings
- **`printer.ask_confirm(action, &processes, yes)`** — the confirmation prompt; errors with `ConfirmationRequired` where nobody can answer (`--json`, no terminal). It builds on `crate::ui::must_confirm`
- **`printer.print_confirmation(action, &processes)`** — only the display half (the `⚠` list), called by `ask_confirm`
- **`printer.with_not_found(&resolved.not_found)`** — makes the printer put `not_found` in the JSON it writes
- **`crate::core::flatten_targets`** — splits target arguments (and deprecated comma lists) into one list
- **`crate::core::NameMatch`** — exact name by default, `--cmd` for a part of the name or command line
- **`crate::core::apply_to_each`** — runs an action on each process and returns `(succeeded, failed)`
- **`printer.print_action_result(action, &succeeded, &failed)`** — unified result summary for destructive commands

### Error Handling

Use `ProcError` variants from `src/error.rs`. Return `Result<()>` from execute methods.

## Building & Testing

```bash
cargo build              # Debug build
cargo build --release    # Release build
cargo test               # Run tests
cargo fmt                # Format code
cargo clippy             # Lint
```

Pre-commit hook runs `cargo fmt --check` automatically.

Every change to a command, flag, argument or output gets tests for its cases (targets, filters, JSON, bad input, pipes, no terminal), in the suite CONTRIBUTING.md's "What a change is tested for" names. CI enforces 100% line coverage and a test for every flag.

## Unreleased Work

Work is committed and pushed to `main` without a tag, and every change goes under `## [Unreleased]` in `CHANGELOG.md` in the same commit. Every place that describes proc is updated with the change, so `main` describes the next release:

- `README.md`, and `pkg/npm/README.md` (the npm page)
- `skills/proc-cli/` (`SKILL.md`, `reference.md`) and `.claude-plugin/marketplace.json` descriptions
- the website, `docs/`: `index.html`, `install.md`, `faq.md`, `skill.md`, `commands/` (`index.md` and each command's page) and `_data/`
- the Rust docs on docs.rs: `src/lib.rs` and the doc comments of public items
- `--help`, the man page and completions, which come from the doc comments in `src/cli.rs` and `src/commands/`

To find them, search for what the change makes untrue (the old flag, message, exit code or behaviour) across all of these, not only the file you changed. Nothing is tagged or version-bumped until the maintainer decides to ship; the release then turns `[Unreleased]` into the new version (see RELEASE.md).

## Releasing

**ALWAYS follow [RELEASE.md](RELEASE.md) for every release.** Do not skip steps. This includes:

1. Pre-release checks: `rustup update stable` and `cargo update` (first), `cargo +<rust-version> check --all-targets` (the oldest supported Rust, from Cargo.toml), `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build --release`, `cargo audit`, `cargo check --target x86_64-pc-windows-msvc --all-targets`
2. Manual testing with the release binary (`./target/release/proc --version`, `--help`, new features)
3. Version bump in ALL files: `Cargo.toml` (and `Cargo.lock`; `flake.nix` reads both and needs no bump), `pkg/npm/package.json`, `.claude-plugin/marketplace.json`, `docs/_data/release.yml`
4. Documentation: `CHANGELOG.md` (then `python3 scripts/changelog_site.py` writes `docs/changelog.md` from it), `README.md`, `ROADMAP.md`, `src/lib.rs`, `skills/proc-cli/*.md`, `docs/commands/` — verify all reflect new changes
5. Commit, tag, push (tag push triggers CI for all package managers); then move the nixpkgs package (`pkg/nixpkgs/`) to the new version and its pull request (RELEASE.md, "After the tag: nixpkgs")
6. Verify CI completes successfully

## Documentation Files

- `README.md` — Main documentation, installation, examples, shell completions, man page
- `CHANGELOG.md` — Keep a Changelog format, update with each release
- `RELEASE.md` — Release checklist and publishing process
- `ROADMAP.md` — The future only: candidate ideas, none promised until vetted against PHILOSOPHY.md. Update it with every change that adds, finishes or rethinks planned work; shipped items leave it (the CHANGELOG is the history)
- `PHILOSOPHY.md` — Project manifesto, principles, feature test: the criteria every ROADMAP idea is vetted against. Not tied to a version; it changes rarely and deliberately
- `CONTRIBUTING.md` — Contribution guidelines
- `SECURITY.md` — Security policy
- `skills/proc-cli/SKILL.md` — Agent Skills definition for LLM tool use
- `skills/proc-cli/reference.md` — JSON output schemas for the skill
- `docs/commands/` — GitHub Pages per-command documentation

## Philosophy (Summary)

See `PHILOSOPHY.md` for the full manifesto.

**Principles:** Semantic, Explicit, Complete, Fast, Lightweight, Reproducible, Obvious

**The Feature Test** — every feature must pass:
1. Does it fit "process and port management"?
2. Can it be expressed in one obvious command?
3. Does it make the common case effortless?
4. Is user intent explicit?
5. Does it deepen proc's command of its domain?
6. Does it follow our conventions?

**Out of scope:** Service management, containers, remote processes, GUIs, historical data.
