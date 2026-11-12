//! The docs cover the CLI: every command, flag, accepted value, exit code and
//! error reason proc has is written down where people and programs look.
//!
//! The commands, their flags and the values a flag accepts are read from the
//! clap definitions (`Cli::command()`), the reasons from the `REASON_*`
//! constants, so a new one fails here until the docs name it:
//! - each command's page on the website (`docs/commands/<cmd>.md`) lists every
//!   flag of that command with its short form, and every value it accepts
//! - the README, the agent reference (`skills/proc-cli/reference.md`) and
//!   docs.rs (`src/lib.rs`) name every command
//! - the website's one grouping of the commands (`docs/_data/commands.yml`,
//!   which the home page, the commands page and its sidebar read) holds every
//!   command once, and the README's tables and docs.rs group them the same way
//! - the agent reference's "Commands and flags" section names every flag, with
//!   its short form on the same line, and every accepted value
//! - the reference, the agent skill, the commands page, the README and docs.rs
//!   give every exit code and every `reason`
//! - the proc version the agent skill asks for is no older than the release
//!   that brought each flag it uses
//! - the packages that list every file they install (pkgsrc, FreeBSD) list
//!   the man page of every command

use clap::CommandFactory;
use proc_cli::cli::Cli;
use proc_cli::error::{
    REASON_COMMAND_LINE_ONLY, REASON_INVALID_INPUT, REASON_NEEDS_CONFIRMATION, REASON_NOT_FOUND,
    REASON_NOT_SUPPORTED, REASON_PERMISSION_DENIED, REASON_RULED_OUT, REASON_SYSTEM_ERROR,
};
use regex::Regex;
use std::path::PathBuf;

/// A file of the repository, with Windows checkouts' CRLF read as LF
fn read(path: &str) -> String {
    let full = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    std::fs::read_to_string(&full)
        .unwrap_or_else(|e| panic!("{}: {e}", full.display()))
        .replace("\r\n", "\n")
}

/// The commands `proc --help` lists (not the hidden `completions` and
/// `manpage`, nor `help`)
fn commands() -> Vec<clap::Command> {
    Cli::command()
        .get_subcommands()
        .filter(|c| !c.is_hide_set() && c.get_name() != "help")
        .cloned()
        .collect()
}

/// A command's flags as `(--long, Some(-s))`, without `--help` and `--version`
fn flags(cmd: &clap::Command) -> Vec<(String, Option<String>)> {
    cmd.get_arguments()
        .filter(|a| !a.is_hide_set())
        .filter_map(|a| {
            let long = a.get_long()?;
            if long == "help" || long == "version" {
                return None;
            }
            Some((format!("--{long}"), a.get_short().map(|s| format!("-{s}"))))
        })
        .collect()
}

/// The values a flag or argument accepts, when clap lists them (`--sort`,
/// `--status`), without the plain `true`/`false` of a switch
fn values(arg: &clap::Arg) -> Vec<String> {
    if !arg.get_action().takes_values() {
        return Vec::new();
    }
    arg.get_possible_values()
        .iter()
        .filter(|v| !v.is_hide_set())
        .map(|v| v.get_name().to_string())
        .collect()
}

/// Whether `word` is in `text` as a whole word (`pid` not inside `ppid`)
fn has_word(text: &str, word: &str) -> bool {
    Regex::new(&format!(
        r"(^|[^A-Za-z0-9_-]){}($|[^A-Za-z0-9_-])",
        regex::escape(word)
    ))
    .unwrap()
    .is_match(text)
}

/// The text of a section, from its heading to the next heading of the same
/// or a higher level
fn section<'a>(text: &'a str, heading: &str) -> &'a str {
    let start = text
        .find(&format!("\n{heading}\n"))
        .unwrap_or_else(|| panic!("no heading {heading:?}"));
    let level = heading.chars().take_while(|&c| c == '#').count();
    let body = &text[start + heading.len() + 2..];
    let end = body
        .match_indices("\n#")
        .find(|(i, _)| {
            let rest = &body[i + 1..];
            let n = rest.chars().take_while(|&c| c == '#').count();
            n <= level && rest[n..].starts_with(' ')
        })
        .map_or(body.len(), |(i, _)| i);
    &body[..end]
}

/// Fails with every miss at once, so one run lists all the docs to fix
fn assert_none_missing(misses: Vec<String>) {
    assert!(
        misses.is_empty(),
        "{} missing from the docs:\n  {}",
        misses.len(),
        misses.join("\n  ")
    );
}

#[test]
fn every_command_has_its_page_and_is_named_everywhere() {
    let readme = read("README.md");
    let reference = read("skills/proc-cli/reference.md");
    let lib = read("src/lib.rs");
    let mut misses = Vec::new();
    for cmd in commands() {
        let name = cmd.get_name();
        let page =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("docs/commands/{name}.md"));
        if !page.exists() {
            misses.push(format!("docs/commands/{name}.md"));
        }
        for (file, text) in [
            ("README.md", &readme),
            ("skills/proc-cli/reference.md", &reference),
            ("src/lib.rs", &lib),
        ] {
            if !text.contains(&format!("`{name}")) {
                misses.push(format!("{file}: {name}"));
            }
        }
    }
    assert_none_missing(misses);
}

/// The groups of `docs/_data/commands.yml`, in order: each title with its
/// commands' names
fn website_groups() -> Vec<(String, Vec<String>)> {
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for line in read("docs/_data/commands.yml").lines() {
        if let Some(title) = line.strip_prefix("- title: ") {
            groups.push((title.trim().to_string(), Vec::new()));
        } else if let Some(name) = line.trim_start().strip_prefix("- name: ") {
            let (_, names) = groups.last_mut().expect("a command before any group");
            // `"on" # quoted: ...`: the value, without quotes or a comment
            let name = name.split(" #").next().unwrap().trim().trim_matches('"');
            names.push(name.to_string());
        }
    }
    groups
}

#[test]
fn the_commands_are_grouped_the_same_everywhere() {
    let groups = website_groups();
    let mut listed: Vec<String> = groups.iter().flat_map(|(_, n)| n.clone()).collect();
    listed.sort();
    let mut all: Vec<String> = commands()
        .iter()
        .map(|c| c.get_name().to_string())
        .collect();
    all.sort();
    assert_eq!(
        listed, all,
        "docs/_data/commands.yml lists every command once"
    );

    let readme = read("README.md");
    let lib = read("src/lib.rs");
    let cell = Regex::new(r"(?m)^\| `([a-z]+)").unwrap();
    let named = Regex::new(r"`([a-z]+)`").unwrap();
    for (title, names) in &groups {
        let table = section(&readme, &format!("### {title}"));
        let in_readme: Vec<String> = cell
            .captures_iter(table)
            .map(|c| c[1].to_string())
            .collect();
        let mut sorted_readme = in_readme.clone();
        sorted_readme.sort();
        let mut sorted_names = names.clone();
        sorted_names.sort();
        assert_eq!(sorted_readme, sorted_names, "README's {title} table");

        let line = lib
            .lines()
            .find(|l| l.starts_with(&format!("//! **{title}**")))
            .unwrap_or_else(|| panic!("src/lib.rs has a **{title}** line"));
        let mut in_lib: Vec<String> = named
            .captures_iter(line)
            .map(|c| c[1].to_string())
            .filter(|n| names.contains(n))
            .collect();
        in_lib.sort();
        assert_eq!(in_lib, sorted_names, "src/lib.rs's {title} line");
    }
}

#[test]
fn the_packages_that_list_their_files_list_a_man_page_per_command() {
    // pkgsrc and FreeBSD name every file a package installs; one missing from
    // the list fails their build, one extra is left behind
    let pkgsrc = read("pkg/pkgsrc/PLIST");
    let freebsd = read("pkg/freebsd/Makefile");
    let mut misses = Vec::new();
    for cmd in commands() {
        let name = cmd.get_name();
        if !pkgsrc.contains(&format!("man/man1/proc-{name}.1\n")) {
            misses.push(format!("pkg/pkgsrc/PLIST: man/man1/proc-{name}.1"));
        }
        if !freebsd.contains(&format!("share/man/man1/proc-{name}.1.gz")) {
            misses.push(format!(
                "pkg/freebsd/Makefile: share/man/man1/proc-{name}.1.gz"
            ));
        }
    }
    assert_none_missing(misses);
}

#[test]
fn every_flag_is_on_its_command_page_with_its_short_form() {
    let mut misses = Vec::new();
    for cmd in commands() {
        let name = cmd.get_name();
        let page = read(&format!("docs/commands/{name}.md"));
        for (long, short) in flags(&cmd) {
            // The options table row for the flag: `| `--flag ...` | `-f` | ... |`
            let row = page.lines().find(|l| {
                l.starts_with(&format!("| `{long}`")) || l.starts_with(&format!("| `{long} "))
            });
            match (row, short) {
                (None, _) => misses.push(format!("docs/commands/{name}.md: {long}")),
                (Some(row), Some(short)) if !row.contains(&format!("`{short}`")) => misses.push(
                    format!("docs/commands/{name}.md: {short} on the {long} row"),
                ),
                _ => {}
            }
        }
    }
    assert_none_missing(misses);
}

#[test]
fn every_accepted_value_is_on_the_command_page_and_in_the_reference() {
    let reference = read("skills/proc-cli/reference.md");
    let flags_section = section(&reference, "## Commands and flags");
    let mut misses = Vec::new();
    for cmd in commands() {
        let name = cmd.get_name();
        let page = read(&format!("docs/commands/{name}.md"));
        for arg in cmd.get_arguments() {
            for value in values(arg) {
                let flag = arg
                    .get_long()
                    .map_or(arg.get_id().to_string(), |l| format!("--{l}"));
                if !has_word(&page, &value) {
                    misses.push(format!("docs/commands/{name}.md: {flag} {value}"));
                }
                if !has_word(flags_section, &value) {
                    misses.push(format!(
                        "reference.md, Commands and flags: {name} {flag} {value}"
                    ));
                }
            }
        }
    }
    misses.dedup();
    assert_none_missing(misses);
}

#[test]
fn every_flag_is_in_the_reference_and_the_readme() {
    let reference = read("skills/proc-cli/reference.md");
    let flags_section = section(&reference, "## Commands and flags");
    let readme = read("README.md");
    let mut misses = Vec::new();
    for cmd in commands() {
        for (long, _) in flags(&cmd) {
            if !has_word(flags_section, &long) {
                misses.push(format!(
                    "reference.md, Commands and flags: {long} ({})",
                    cmd.get_name()
                ));
            }
            if !has_word(&readme, &long) {
                misses.push(format!("README.md: {long} ({})", cmd.get_name()));
            }
        }
    }
    misses.sort();
    misses.dedup();
    assert_none_missing(misses);
}

#[test]
fn every_short_flag_is_beside_its_long_form_in_the_reference() {
    let reference = read("skills/proc-cli/reference.md");
    let flags_section = section(&reference, "## Commands and flags");
    let mut misses = Vec::new();
    for cmd in commands() {
        for (long, short) in flags(&cmd) {
            let Some(short) = short else { continue };
            let beside = flags_section
                .lines()
                .any(|line| has_word(line, &long) && line.contains(&format!("`{short}`")));
            if !beside {
                misses.push(format!(
                    "reference.md, Commands and flags: {short} on the {long} line ({})",
                    cmd.get_name()
                ));
            }
        }
    }
    misses.sort();
    misses.dedup();
    assert_none_missing(misses);
}

#[test]
fn every_exit_code_and_reason_is_explained() {
    let reasons = [
        REASON_SYSTEM_ERROR,
        REASON_NOT_FOUND,
        REASON_COMMAND_LINE_ONLY,
        REASON_RULED_OUT,
        REASON_PERMISSION_DENIED,
        REASON_INVALID_INPUT,
        REASON_NEEDS_CONFIRMATION,
        REASON_NOT_SUPPORTED,
    ];
    // An exit code is a row of a table (`| 3 |` or `` | `3` | ``), or in
    // docs.rs's prose `` `3` ``
    let row = |code: u8| Regex::new(&format!(r"(?m)^\| `?{code}`? \|")).unwrap();
    let mut misses = Vec::new();
    for file in [
        "skills/proc-cli/reference.md",
        "skills/proc-cli/SKILL.md",
        "docs/commands/index.md",
        "README.md",
        "src/lib.rs",
    ] {
        let text = read(file);
        for reason in reasons {
            if !text.contains(&format!("`{reason}`")) && !text.contains(&format!("\"{reason}\"")) {
                misses.push(format!("{file}: reason {reason}"));
            }
        }
        for code in 0..=4u8 {
            let found = if file == "src/lib.rs" {
                text.contains(&format!("`{code}`")) || row(code).is_match(&text.replace("//! ", ""))
            } else {
                row(code).is_match(&text)
            };
            if !found {
                misses.push(format!("{file}: exit code {code}"));
            }
        }
    }
    assert_none_missing(misses);
}

/// `x.y.z` as numbers, for comparing versions
fn version(text: &str) -> (u32, u32, u32) {
    let mut parts = text.split('.').map(|p| p.parse::<u32>().unwrap());
    (
        parts.next().unwrap(),
        parts.next().unwrap(),
        parts.next().unwrap_or(0),
    )
}

#[test]
fn the_skill_asks_for_a_proc_that_has_every_flag_it_uses() {
    let skill = read("skills/proc-cli/SKILL.md");
    let asked = Regex::new(r"Requires the proc CLI (\d+\.\d+(?:\.\d+)?) or later")
        .unwrap()
        .captures(&skill)
        .expect("SKILL.md's compatibility line names the proc version it needs")[1]
        .to_string();
    assert!(
        skill.contains(&format!("This skill is for proc {asked} or later")),
        "SKILL.md's body asks for the same version as its compatibility line, {asked}"
    );

    // The release that first names each flag, oldest first
    let changelog = read("CHANGELOG.md");
    let release = Regex::new(r"(?m)^## \[(\d+\.\d+\.\d+)\]").unwrap();
    let mut releases: Vec<(&str, &str)> = Vec::new();
    let starts: Vec<_> = release.captures_iter(&changelog).collect();
    for (i, cap) in starts.iter().enumerate() {
        let start = cap.get(0).unwrap().end();
        let end = starts
            .get(i + 1)
            .map_or(changelog.len(), |c| c.get(0).unwrap().start());
        releases.push((cap.get(1).unwrap().as_str(), &changelog[start..end]));
    }
    releases.reverse();

    let known: Vec<String> = commands()
        .iter()
        .flat_map(flags)
        .map(|(long, _)| long)
        .collect();
    let used = Regex::new(r"--[a-z][a-z-]+").unwrap();
    let mut too_new = Vec::new();
    for flag in used.find_iter(&skill).map(|m| m.as_str()) {
        if !known.iter().any(|k| k == flag) {
            continue;
        }
        let first = releases.iter().find(|(_, text)| has_word(text, flag));
        if let Some((since, _)) = first {
            if version(since) > version(&asked) {
                too_new.push(format!("{flag} (since {since})"));
            }
        }
    }
    too_new.sort();
    too_new.dedup();
    assert!(
        too_new.is_empty(),
        "SKILL.md asks for proc {asked} but uses flags from later releases: {}",
        too_new.join(", ")
    );
}
