//! proc - Semantic Process Management CLI
//!
//! A semantic command-line tool for process management.

use clap::{CommandFactory, Parser};
use clap_complete::generate;
use colored::Colorize;
use proc_cli::cli::{Cli, Commands};
use proc_cli::core::{comma_notice, Process};
use proc_cli::error::{ExitCode, ProcError, REASON_INVALID_INPUT};
use proc_cli::ui::out;
use proc_cli::ui::print_note;
use proc_cli::{err_println, out_println};
use std::process;

/// What a bare `proc` prints.
///
/// The full command list is a menu, not an orientation: it tells a newcomer
/// what exists without telling them how the tool thinks. The one thing worth
/// learning first is that a target is the same everywhere, so that is what
/// this leads with — and every line here is a command that runs as shown.
fn print_orientation() {
    out_println!(
        "{} {}",
        "proc".bold(),
        "— semantic process management".bright_black()
    );
    out_println!();
    out_println!(
        "  {}",
        "A target is the same everywhere: :port, PID, or name.".bright_black()
    );
    out_println!();
    for (cmd, what) in [
        ("proc on :3000", "what's on port 3000"),
        ("proc in .", "what's running in this directory"),
        ("proc by node", "processes named node"),
        ("proc kill :3000", "stop whatever holds the port"),
    ] {
        out_println!(
            "    {} {}",
            format!("{:<20}", cmd).cyan(),
            what.bright_black()
        );
    }
    out_println!();
    out_println!(
        "  {} {}",
        format!("{:<8}", "Common:").bright_black(),
        "on · in · by · ports · info · kill · stop · free · why".white()
    );
    out_println!();
    out_println!(
        "  {} {}  {}",
        format!("{:<20}", "proc --help").cyan(),
        "all 19 commands".bright_black(),
        "· https://proc-cli.com".bright_black()
    );
}

/// Extract the unknown argument from a clap error message
fn extract_unknown_arg(msg: &str) -> Option<String> {
    // Pattern: "unexpected argument 'X' found"
    let start = msg.find("unexpected argument '")? + "unexpected argument '".len();
    let end = msg[start..].find('\'')?;
    Some(msg[start..start + end].to_string())
}

/// Simple edit distance for flag suggestion
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (m, n) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for (i, row) in dp.iter_mut().enumerate().take(m + 1) {
        row[0] = i;
    }
    for (j, val) in dp[0].iter_mut().enumerate().take(n + 1) {
        *val = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[m][n]
}

/// Find the closest matching flag for a subcommand
fn suggest_flag(unknown: &str, subcmd_name: &str) -> Option<String> {
    let cmd = Cli::command();
    let subcmd = cmd.find_subcommand(subcmd_name)?;
    let unknown_clean = unknown.trim_start_matches('-');

    let mut best: Option<String> = None;
    let mut best_dist = usize::MAX;

    for arg in subcmd.get_arguments() {
        if let Some(long) = arg.get_long() {
            // Check edit distance (skip very short flags and large length differences)
            let len_diff = (unknown_clean.len() as isize - long.len() as isize).unsigned_abs();
            if len_diff <= 2 && long.len() >= 3 {
                let dist = edit_distance(unknown_clean, long);
                if dist < best_dist {
                    best_dist = dist;
                    best = Some(format!("--{}", long));
                }
            }

            // Also check if unknown is a substring of the flag (e.g., "cpu" → "min-cpu")
            if long.contains(unknown_clean) && unknown_clean.len() >= 3 {
                return Some(format!("--{}", long));
            }
        }
    }

    // Only suggest if edit distance is reasonable
    let threshold = if unknown_clean.len() <= 4 { 2 } else { 3 };
    if best_dist <= threshold {
        best
    } else {
        None
    }
}

fn main() -> process::ExitCode {
    process::ExitCode::from(out::finish(run()))
}

/// Run the command and return its exit code. Writing goes through `out`, which
/// records a failed write instead of panicking; `main` folds that into the code.
fn run() -> u8 {
    // An argument that is not UTF-8 is invalid input, said before clap sees it:
    // clap's own message for it does not name the argument
    if let Some(arg) = first_non_utf8_arg() {
        return invalid_argument(&format!(
            "argument '{arg}' is not valid UTF-8; proc takes its arguments as UTF-8"
        ));
    }

    // An empty argument names nothing, and every command used to read it as
    // something: `--in "$DIR"` with DIR unset as the current directory, `in ""`
    // and `for ""` too, `list ""` as a name no process has
    if let Some(what) = first_empty_arg() {
        return invalid_argument(&format!(
            "{what} can't be empty: an empty value names no process, port, file or directory"
        ));
    }

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            use clap::error::ErrorKind;

            // Let help/version pass through as-is
            if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
                // clap draws it, with its own colour rules; a write that fails
                // is recorded by `out`, never a panic
                out::through_stdout(|| e.print());
                return 0;
            }

            // clap's tips go: its own "to pass '--x' as a value" one misleads,
            // and its "a similar ... exists" ones become proc's suggestion
            let msg = e.to_string();
            let parts = split_clap(&msg);

            // Try to suggest a similar flag
            let mut suggestion = None;
            if matches!(
                e.kind(),
                ErrorKind::UnknownArgument | ErrorKind::InvalidValue
            ) {
                let args = raw_args();
                let subcmd = args.iter().find(|a| !a.starts_with('-'));
                let unknown = extract_unknown_arg(&msg);
                if let (Some(subcmd), Some(unknown)) = (subcmd, unknown) {
                    suggestion = suggest_flag(&unknown, subcmd);
                }
            }
            // a subcommand or value clap found close to the one typed
            let similar = best_first(&parts.similar, e.kind() == ErrorKind::InvalidSubcommand);
            if suggestion.is_none() && !similar.is_empty() {
                suggestion = Some(similar.join("' or '"));
            }

            // Usage errors are invalid input, same as runtime validation errors
            let exit_code = ExitCode::InvalidInput as i32;

            // No target, but a filter: say that the filter does not select
            let hint = missing_target_hint(&e, &msg);

            if wants_json() {
                let mut error = parts.one_line();
                if let Some(ref suggestion) = suggestion {
                    error.push_str(&format!("\n  Try: did you mean '{}'?", suggestion));
                }
                if let Some(ref hint) = hint {
                    error.push_str(&format!("\n  {hint}"));
                }
                print_json_error(
                    &error,
                    exit_code,
                    Some(REASON_INVALID_INPUT),
                    None,
                    None,
                    None,
                );
            } else {
                err_println!("{}", parts.human());
                if let Some(suggestion) = suggestion {
                    err_println!("\n  tip: did you mean '{}'?", suggestion);
                }
                if let Some(hint) = hint {
                    err_println!("\n  tip: {hint}");
                }
            }

            return exit_code as u8;
        }
    };

    // Commas in targets still work but are deprecated: say so once, on stderr, in
    // human output only. Every command that splits targets on commas is covered
    // here, in one place, because the matches know which arguments are targets
    // and whether `--json` was given. The command line parsed above, so parsing
    // it again for its matches cannot fail.
    if let Some(notice) = Cli::command()
        .try_get_matches()
        .ok()
        .and_then(|matches| comma_notice(&matches))
    {
        print_note(&notice);
    }

    let Some(command) = cli.command else {
        print_orientation();
        return 0;
    };

    let result = match command {
        Commands::On(cmd) => cmd.execute(),
        Commands::For(cmd) => cmd.execute(),
        Commands::By(cmd) => cmd.execute(),
        Commands::In(cmd) => cmd.execute(),
        Commands::List(cmd) => cmd.execute(),
        Commands::Info(cmd) => cmd.execute(),
        Commands::Ports(cmd) => cmd.execute(),
        Commands::Kill(cmd) => cmd.execute(),
        Commands::Stop(cmd) => cmd.execute(),
        Commands::Tree(cmd) => cmd.execute(),
        Commands::Watch(cmd) => cmd.execute(),
        Commands::Stuck(cmd) => cmd.execute(),
        Commands::Unstick(cmd) => cmd.execute(),
        Commands::Freeze(cmd) => cmd.execute(),
        Commands::Thaw(cmd) => cmd.execute(),
        Commands::Orphans(cmd) => cmd.execute(),
        Commands::Why(cmd) => cmd.execute(),
        Commands::Free(cmd) => cmd.execute(),
        Commands::Wait(cmd) => cmd.execute(),
        // Generated into a buffer first: the generators panic on a write error
        Commands::Completions { shell } => {
            let mut script = Vec::new();
            generate(shell, &mut Cli::command(), "proc", &mut script);
            out::write_stdout(&script);
            Ok(())
        }
        // The title takes the command's version: the version line alone, not
        // the --version text with its URL and licence lines
        Commands::Manpage => {
            let mut page = Vec::new();
            clap_mangen::Man::new(Cli::command().version(env!("PROC_VERSION")))
                .render(&mut page)
                .map(|()| out::write_stdout(&page))
                .map_err(ProcError::from)
        }
    };

    if let Err(e) = result {
        let exit_code = ExitCode::from(&e);

        // The command already printed its result; only the exit code is left
        if matches!(e, ProcError::Reported { .. }) {
            return exit_code as u8;
        }

        // If --json was requested, output error as JSON to stdout
        if wants_json() {
            let confirmation = match &e {
                ProcError::ConfirmationRequired {
                    processes,
                    not_found,
                    ..
                } => Some((processes.as_slice(), not_found.as_deref())),
                _ => None,
            };
            let ruled_out_by = e.ruled_out_by();
            print_json_error(
                &e.to_string(),
                exit_code as i32,
                e.reason(),
                confirmation,
                ruled_out_by.as_deref(),
                e.left_out(),
            );
        } else {
            err_println!("{}", e);
        }

        return exit_code as u8;
    }

    0
}

/// Print an error as the standard JSON error object on stdout.
///
/// `reason` says why, as a fixed word to branch on; every error printed here
/// has one.
///
/// `confirmation` is `Some((processes, not_found))` only for a missing `--yes`:
/// the object then also says `needs_confirmation`, lists what would have been
/// changed and, for a command that takes targets, the targets that were not found.
///
/// `ruled_out_by` is `Some` only for a target its guards ruled out: the flags
/// of those guards (`["--start-time"]`). `left_out` is `Some` only when every
/// match was a process proc runs under: those processes, `{"pid", "name"}`.
fn print_json_error(
    error: &str,
    exit_code: i32,
    reason: Option<&str>,
    confirmation: Option<(&[Process], Option<&[String]>)>,
    ruled_out_by: Option<&[&str]>,
    left_out: Option<&[proc_cli::core::Ancestor]>,
) {
    let mut error_json = serde_json::json!({
        "action": action_name(),
        "success": false,
        "error": error,
        "exit_code": exit_code
    });
    if let Some(reason) = reason {
        error_json["reason"] = serde_json::json!(reason);
    }
    if let Some((processes, not_found)) = confirmation {
        error_json["needs_confirmation"] = serde_json::json!(true);
        error_json["processes"] = serde_json::json!(processes);
        if let Some(not_found) = not_found {
            error_json["not_found"] = serde_json::json!(not_found);
        }
    }
    if let Some(ruled_out_by) = ruled_out_by {
        error_json["ruled_out_by"] = serde_json::json!(ruled_out_by);
    }
    if let Some(left_out) = left_out {
        error_json["left_out"] = serde_json::json!(left_out);
    }
    out_println!("{}", serde_json::to_string_pretty(&error_json).unwrap());
}

/// Canonical command name for the JSON `action` field, resolving aliases
/// (e.g. `proc k` → "kill"). Falls back to the raw argument, or "proc".
fn action_name() -> String {
    let first = raw_args().into_iter().find(|a| !a.starts_with('-'));
    match first {
        Some(arg) => Cli::command()
            .find_subcommand(&arg)
            .map(|cmd| cmd.get_name().to_string())
            .unwrap_or(arg),
        None => "proc".to_string(),
    }
}

/// Check if --json or -j was passed in the command line arguments
fn wants_json() -> bool {
    raw_args()
        .into_iter()
        .take_while(|a| a != "--")
        .any(|a| is_json_flag(&a))
}

/// The arguments after the program name, as text. A byte that is not UTF-8
/// shows as U+FFFD. `std::env::args()` would panic on one; this never does.
fn raw_args() -> Vec<String> {
    std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

/// Report an argument proc refuses before clap reads the line: exit 4,
/// `invalid_input`
fn invalid_argument(message: &str) -> u8 {
    let exit_code = ExitCode::InvalidInput;
    if wants_json() {
        print_json_error(
            message,
            exit_code as i32,
            Some(REASON_INVALID_INPUT),
            None,
            None,
            None,
        );
    } else {
        err_println!("error: {message}");
    }
    exit_code as u8
}

/// What the first empty argument was given for: the flag of a `--in=` form,
/// or "an argument" for an empty word (`""`). Run after the UTF-8 check.
fn first_empty_arg() -> Option<String> {
    raw_args().into_iter().find_map(|arg| {
        if arg.is_empty() {
            return Some("an argument".to_string());
        }
        arg.strip_suffix('=')
            .filter(|flag| flag.starts_with('-'))
            .map(str::to_string)
    })
}

/// The first argument that is not valid UTF-8, in lossy form
fn first_non_utf8_arg() -> Option<String> {
    std::env::args_os()
        .skip(1)
        .find(|arg| arg.to_str().is_none())
        .map(|arg| arg.to_string_lossy().into_owned())
}

/// clap's rendered error, in the parts proc uses. clap writes the message,
/// then, each after a blank line, its tips (`  tip: ...`), the `Usage:` block
/// and the `--help` pointer. A part starts only after a blank line, so a value
/// the user typed that holds `\nUsage:` or `\n  tip:` stays in the message.
struct ClapMessage<'a> {
    /// The message, as clap wrote it. The detail (the missing `<TARGET>...`,
    /// the `[possible values: ...]`) is on the lines after the first.
    message: Vec<&'a str>,
    /// What clap's "a similar ... exists" tips suggest
    similar: Vec<String>,
    /// The `Usage:` block and the `--help` pointer, as clap wrote them
    rest: Vec<&'a str>,
}

fn split_clap(msg: &str) -> ClapMessage<'_> {
    let mut parts = ClapMessage {
        message: Vec::new(),
        similar: Vec::new(),
        rest: Vec::new(),
    };
    let (mut after_blank, mut in_tips) = (false, false);
    for line in msg.lines() {
        let starts_part = after_blank || in_tips;
        after_blank = line.is_empty();
        if !parts.rest.is_empty()
            || starts_part
                && (line.starts_with("Usage:") || line.starts_with("For more information"))
        {
            parts.rest.push(line);
        } else if starts_part && line.starts_with("  tip: ") {
            in_tips = true;
            parts.similar.extend(similar_in(line));
        } else if !line.is_empty() && !in_tips {
            parts.message.push(line);
        }
    }
    parts
}

/// What clap's "a similar subcommand exists: 'kill'" or "some similar values
/// exist: 'a', 'b'" tip suggests; nothing for its other tips, or for one whose
/// list is not plain quoted names (text the user typed to look like a tip)
fn similar_in(tip: &str) -> Vec<String> {
    let list = tip
        .split_once(" similar ")
        .and_then(|(_, rest)| rest.split_once(" exist"))
        .and_then(|(_, rest)| rest.split_once(": "));
    let names = list.and_then(|(_, names)| {
        names
            .split(", ")
            .map(|name| {
                let name = name.strip_prefix('\'')?.strip_suffix('\'')?;
                let plain = !name.is_empty() && !name.contains(['\'', ' ']);
                plain.then(|| name.to_string())
            })
            .collect::<Option<Vec<_>>>()
    });
    names.unwrap_or_default()
}

/// clap's suggestions, closest first and at most three. clap lists the closest
/// last, and puts a subcommand's alias (`k`) beside it, so a subcommand is
/// named once, by its name.
fn best_first(similar: &[String], subcommands: bool) -> Vec<String> {
    let cli = Cli::command();
    let mut names: Vec<String> = Vec::new();
    for name in similar.iter().rev() {
        let name = match cli.find_subcommand(name) {
            Some(sub) if subcommands => sub.get_name().to_string(),
            _ => name.clone(),
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names.truncate(3);
    names
}

impl ClapMessage<'_> {
    /// The message as one line for the JSON `error`: its lines joined with a
    /// space, without the `error: ` prefix
    fn one_line(&self) -> String {
        let lines: Vec<&str> = self
            .message
            .iter()
            .map(|line| line.trim())
            .map(|line| line.strip_prefix("error: ").unwrap_or(line))
            .collect();
        lines.join(" ")
    }

    /// clap's rendering without its tips
    fn human(&self) -> String {
        let mut text = self.message.join("\n");
        if !self.rest.is_empty() {
            text.push_str("\n\n");
            text.push_str(&self.rest.join("\n"));
        }
        text
    }
}

/// The `--by` or `--in` flag given on the command line, if any (`-b`, `-i`,
/// their `=` forms and short-flag clusters count)
fn filter_flag_given() -> Option<&'static str> {
    for arg in raw_args().iter().take_while(|a| *a != "--") {
        if arg == "--by" || arg.starts_with("--by=") {
            return Some("--by");
        }
        if arg == "--in" || arg.starts_with("--in=") {
            return Some("--in");
        }
        let Some(cluster) = arg.strip_prefix('-').filter(|c| !c.starts_with('-')) else {
            continue;
        };
        for c in cluster.chars() {
            match c {
                'b' => return Some("--by"),
                'i' => return Some("--in"),
                'd' | 'l' | 'n' | 'p' | 'r' | 's' | 'S' | 't' => break,
                _ => {}
            }
        }
    }
    None
}

/// What to add when a command that needs targets got a filter but no target:
/// the filter narrows the targets, it does not pick them.
fn missing_target_hint(e: &clap::Error, msg: &str) -> Option<String> {
    if e.kind() != clap::error::ErrorKind::MissingRequiredArgument || !msg.contains("<TARGET>") {
        return None;
    }
    let flag = filter_flag_given()?;
    Some(format!(
        "{} needs at least one target; {flag} filters the targets, it doesn't select them",
        action_name()
    ))
}

/// Matches `--json`, `-j`, and short-flag clusters such as `-yj`.
///
/// Scanning a cluster stops at the first short flag that takes a value,
/// because everything after it is that flag's value (`-bj` is `--by j`).
fn is_json_flag(arg: &str) -> bool {
    if arg == "--json" {
        return true;
    }
    let Some(cluster) = arg.strip_prefix('-') else {
        return false;
    };
    if cluster.starts_with('-') {
        return false;
    }
    for c in cluster.chars() {
        match c {
            'j' => return true,
            'b' | 'd' | 'i' | 'l' | 'n' | 'p' | 'r' | 's' | 'S' | 't' => return false,
            _ => {}
        }
    }
    false
}
