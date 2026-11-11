//! Output formatting for proc CLI
//!
//! Provides colored terminal output and JSON formatting.

use crate::core::Process;
use crate::error::{ExitCode, ProcError, Result};
use crate::ui::format::plural;
use crate::ui::table::{process_table, Screen, TableLayout};
use crate::ui::{out, prompt};
use colored::*;
use serde::Serialize;
use std::io::Write;

/// Run a writer-based print against stdout. A failed write is recorded by
/// [`out`], which decides what it means for the exit code; it never panics.
fn to_stdout(write: impl FnOnce(&mut out::Out) -> std::io::Result<()>) {
    out::recorded(write(&mut out::stdout()));
}

/// Whether a command that changes processes should ask before acting.
///
/// The convention is the same in every output format: changing a process
/// needs a yes. `--yes` gives it up front (`Ok(false)`: go ahead without
/// asking); otherwise a person is asked (`Ok(true)`). `--json` only changes
/// how results are printed, so it never stands in for `--yes`, and without a
/// terminal there's nobody to ask: both are refused with exit 4 before
/// anything is touched. `--dry-run` never gets here; it previews without one.
///
/// Both refusals carry `processes`, the ones the command would have changed,
/// so a caller reading the JSON error can see what a retry with `--yes` hits.
/// A command that takes targets also passes `not_found`, the ones that matched
/// nothing, which the refusal carries too.
pub fn must_confirm(
    action: &str,
    yes: bool,
    json: bool,
    processes: &[Process],
    not_found: Option<&[String]>,
) -> Result<bool> {
    use std::io::IsTerminal;
    if yes {
        return Ok(false);
    }
    if json {
        return Err(ProcError::ConfirmationRequired {
            message: format!(
                "{action} changes processes, so it needs --yes, with --json too \
                 (or --dry-run to preview)"
            ),
            processes: processes.to_vec(),
            not_found: not_found.map(<[String]>::to_vec),
        });
    }
    if !std::io::stdin().is_terminal() || !out::stderr_is_terminal() {
        return Err(ProcError::ConfirmationRequired {
            message: format!(
                "{action} asks before changing processes, and there's no terminal \
                 to ask in; pass --yes (or --dry-run to preview)"
            ),
            processes: processes.to_vec(),
            not_found: not_found.map(<[String]>::to_vec),
        });
    }
    Ok(true)
}

/// Whether a note on stderr is coloured: only on a terminal, and never when
/// `NO_COLOR` is set to something (https://no-color.org).
fn note_in_colour(stderr_is_terminal: bool, no_color: Option<std::ffi::OsString>) -> bool {
    stderr_is_terminal && no_color.is_none_or(|value| value.is_empty())
}

fn write_note(out: &mut impl Write, message: &str, colour: bool) -> std::io::Result<()> {
    if colour {
        writeln!(out, "\x1b[1;36mnote:\x1b[0m {message}")
    } else {
        writeln!(out, "note: {message}")
    }
}

/// Print a `note:` line on stderr, for something the user should change that
/// does not stop the command. It is for human output only: stdout and `--json`
/// stay as they are. Coloured only when stderr is a terminal and `NO_COLOR` is
/// not set.
pub fn print_note(message: &str) {
    let colour = note_in_colour(out::stderr_is_terminal(), std::env::var_os("NO_COLOR"));
    // A closed stderr has nobody to tell, and the command goes on
    out::recorded(write_note(&mut out::stderr(), message, colour));
}

/// Output format selection
#[derive(Debug, Clone, Copy, Default)]
pub enum OutputFormat {
    /// Colored, human-readable terminal output
    #[default]
    Human,
    /// Machine-readable JSON output for scripting
    Json,
}

/// Main printer for CLI output
pub struct Printer {
    format: OutputFormat,
    verbose: bool,
    /// Print full values instead of truncating them to the terminal width.
    wide: bool,
    /// The command is scoped to a directory (`--in`): DIR is the table's
    /// second column.
    scoped_to_dir: bool,
    /// The targets that matched nothing, for a command that takes targets. When
    /// set, every JSON result the printer writes has them as `not_found`.
    not_found: Option<Vec<String>>,
    /// The processes proc runs under that a target matched and that were left
    /// out (see [`Printer::with_left_out`])
    left_out: Vec<crate::core::Ancestor>,
    /// The processes the preview listed, in its order, for the receipt of an
    /// action (see [`Printer::with_processes`])
    processes: Option<Vec<Process>>,
}

/// Detect terminal width, falling back to 120 when stdout is not a TTY.
pub(crate) fn terminal_width() -> u16 {
    crossterm::terminal::size().map(|(w, _)| w).unwrap_or(120)
}

/// A screen `width` columns wide, at the time now
pub(crate) fn screen_of(width: u16) -> Screen {
    Screen {
        width,
        now_secs: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs()),
    }
}

/// The terminal as it is now: its width and the clock
pub(crate) fn screen_now() -> Screen {
    screen_of(terminal_width())
}

impl Printer {
    /// Creates a new printer with the specified format and verbosity.
    pub fn new(format: OutputFormat, verbose: bool) -> Self {
        Self {
            format,
            verbose,
            wide: false,
            scoped_to_dir: false,
            not_found: None,
            left_out: Vec::new(),
            processes: None,
        }
    }

    /// Say which processes proc runs under a target matched and left out: the
    /// JSON results (dry run, action and the empty result) then list them as
    /// `left_out`, which is absent when there are none
    pub fn with_left_out(mut self, left_out: &[crate::core::Ancestor]) -> Self {
        self.left_out = left_out.to_vec();
        self
    }

    /// Say which targets matched nothing: the JSON results (dry run, action,
    /// the empty result and the refusal that asks for `--yes`) then list them
    /// as `not_found`, an empty array when every target was found.
    pub fn with_not_found(mut self, not_found: &[String]) -> Self {
        self.not_found = Some(not_found.to_vec());
        self
    }

    /// Say which processes the action was tried on, in the order the preview
    /// listed them: the JSON receipt of [`Printer::print_action_result`] then
    /// lists them as `processes` in that order. Without it the receipt lists
    /// the succeeded processes, then the failed ones.
    pub fn with_processes(mut self, processes: &[Process]) -> Self {
        self.processes = Some(processes.to_vec());
        self
    }

    /// Opt into untruncated output (`--wide`). Values print in full and rows
    /// may exceed the terminal width rather than being elided.
    pub fn wide(mut self, wide: bool) -> Self {
        self.wide = wide;
        self
    }

    /// Say whether the command is scoped to a directory (`--in`): the process
    /// table then has DIR as its second column, with or without `-v`.
    pub fn scoped_to_dir(mut self, scoped: bool) -> Self {
        self.scoped_to_dir = scoped;
        self
    }

    /// Create a printer from common CLI flags.
    pub fn from_flags(json: bool, verbose: bool) -> Self {
        Self::new(
            if json {
                OutputFormat::Json
            } else {
                OutputFormat::Human
            },
            verbose,
        )
    }

    /// Print a success message (human only — use `print_empty_result` for JSON-safe success)
    pub fn success(&self, message: &str) {
        to_stdout(|out| self.write_success(out, message));
    }

    fn write_success(&self, out: &mut impl Write, message: &str) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Human => writeln!(out, "{} {}", "✓".green().bold(), message.green()),
            // JSON output handled separately
            OutputFormat::Json => Ok(()),
        }
    }

    /// Print a "nothing found" success result with JSON support.
    ///
    /// In human mode, prints a green success message. In JSON mode, outputs
    /// structured JSON so scripts/LLMs always get parseable output.
    pub fn print_empty_result(&self, action: &str, message: &str) {
        match self.format {
            OutputFormat::Human => self.success(message),
            OutputFormat::Json => self.print_json(&EmptyResult {
                action,
                success: true,
                count: 0,
                message,
                not_found: self.not_found.as_deref(),
                left_out: Some(&self.left_out),
            }),
        }
    }

    /// Print a note, for people only: `ℹ <message>`
    pub fn note(&self, message: &str) {
        to_stdout(|out| self.write_note(out, message));
    }

    fn write_note(&self, out: &mut impl Write, message: &str) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Human => writeln!(out, "{} {}", "ℹ".cyan().bold(), message.cyan()),
            OutputFormat::Json => Ok(()),
        }
    }

    /// Print a warning message
    pub fn warning(&self, message: &str) {
        to_stdout(|out| self.write_warning(out, message));
    }

    fn write_warning(&self, out: &mut impl Write, message: &str) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Human => writeln!(out, "{} {}", "⚠".yellow().bold(), message.yellow()),
            // JSON output handled separately
            OutputFormat::Json => Ok(()),
        }
    }

    /// Print a list of processes with action name and optional context.
    ///
    /// The `action` parameter sets the JSON `action` field (e.g. "list", "by", "stuck").
    pub fn print_processes_as(&self, action: &str, processes: &[Process], context: Option<&str>) {
        match self.format {
            OutputFormat::Human => self.print_processes_human(processes, context, false),
            OutputFormat::Json => self.print_json(&ProcessListOutput {
                action,
                success: true,
                count: processes.len(),
                processes,
                left_out: Some(&self.left_out),
            }),
        }
    }

    /// Print a list of processes with optional context (e.g., "in /path/to/dir").
    /// Uses "list" as the JSON action name.
    pub fn print_processes_with_context(&self, processes: &[Process], context: Option<&str>) {
        self.print_processes_as("list", processes, context)
    }

    /// Print a list of processes. Uses "list" as the JSON action name.
    pub fn print_processes(&self, processes: &[Process]) {
        self.print_processes_with_context(processes, None)
    }

    fn print_processes_human(
        &self,
        processes: &[Process],
        context: Option<&str>,
        whole_command: bool,
    ) {
        to_stdout(|out| {
            self.write_processes_human(out, screen_now(), processes, context, whole_command)
        });
    }

    /// `screen` is what the table is fitted to. `whole_command` prints the
    /// command lines whole, for a list a person is about to approve.
    fn write_processes_human(
        &self,
        out: &mut impl Write,
        screen: Screen,
        processes: &[Process],
        context: Option<&str>,
        whole_command: bool,
    ) -> std::io::Result<()> {
        if processes.is_empty() {
            let msg = match context {
                Some(ctx) => format!("No processes found {}", ctx),
                None => "No processes found".to_string(),
            };
            return self.write_warning(out, &msg);
        }

        let context_str = context.map(|c| format!(" {}", c)).unwrap_or_default();
        writeln!(
            out,
            "{} Found {} process{}{}",
            "✓".green().bold(),
            processes.len().to_string().cyan().bold(),
            if processes.len() == 1 { "" } else { "es" },
            context_str.bright_black()
        )?;
        writeln!(out)?;

        self.write_process_rows(out, screen, processes, whole_command)
    }

    /// Like `print_processes_as`, for a caller that has printed its own headline
    /// (`⚠ Found 2 orphaned processes`): the generic `✓ Found 2 processes` line
    /// would repeat it, so only the table is printed.
    pub fn print_processes_under_headline(&self, action: &str, processes: &[Process]) {
        match self.format {
            OutputFormat::Human => to_stdout(|out| {
                writeln!(out)?;
                self.write_process_rows(out, screen_now(), processes, false)
            }),
            OutputFormat::Json => self.print_processes_as(action, processes, None),
        }
    }

    /// The table under a headline, for the list in front of a question: the
    /// command lines are printed whole, since the person answering approves
    /// exactly those processes. Only a person is asked, so this is human output.
    pub fn print_processes_to_approve(&self, processes: &[Process]) {
        to_stdout(|out| {
            writeln!(out)?;
            self.write_process_rows(out, screen_now(), processes, true)
        });
    }

    /// How the table is laid out for this printer: `-v` adds columns and cuts
    /// nothing, `--wide` cuts nothing, and `whole_command` keeps the command
    /// lines whole whatever the flags say.
    fn layout(&self, screen: Screen, whole_command: bool) -> TableLayout {
        let layout = TableLayout::from_flags(screen, self.verbose, self.wide, self.scoped_to_dir);
        if whole_command {
            layout.with_whole_command()
        } else {
            layout
        }
    }

    /// The process table, without a headline, fitted to `screen`
    fn write_process_rows(
        &self,
        out: &mut impl Write,
        screen: Screen,
        processes: &[Process],
        whole_command: bool,
    ) -> std::io::Result<()> {
        let layout = self.layout(screen, whole_command);
        writeln!(out, "{}", process_table(processes, &layout))?;
        writeln!(out)
    }

    /// Print JSON output for any serializable type
    pub fn print_json<T: Serialize>(&self, data: &T) {
        to_stdout(|out| self.write_json(out, &mut out::stderr(), data));
    }

    /// The JSON goes to `out`; a value that can't be serialized is reported on `err`.
    fn write_json<T: Serialize>(
        &self,
        out: &mut impl Write,
        err: &mut impl Write,
        data: &T,
    ) -> std::io::Result<()> {
        match serde_json::to_string_pretty(data) {
            Ok(json) => writeln!(out, "{}", json),
            Err(e) => writeln!(err, "Failed to serialize JSON: {}", e),
        }
    }

    /// Print action result (generalized for kill/stop/freeze/thaw).
    ///
    /// The `action` parameter is the command name (e.g. "kill", "stop", "freeze").
    /// It is used as-is in JSON output and as the verb for human display.
    ///
    /// Returns `Err(ProcError::Reported)` when any process failed, so the command
    /// exits non-zero without printing a second result.
    pub fn print_action_result(
        &self,
        action: &str,
        succeeded: &[Process],
        failed: &[(Process, String)],
    ) -> Result<()> {
        self.print_action_result_as(action, action, succeeded, failed)
    }

    /// Print action result where the JSON `action` differs from the verb shown
    /// to humans (e.g. `proc orphans --kill` reports action "orphans", verb "kill").
    pub fn print_action_result_as(
        &self,
        action: &str,
        verb: &str,
        succeeded: &[Process],
        failed: &[(Process, String)],
    ) -> Result<()> {
        self.write_action_result_as(&mut out::stdout(), action, verb, succeeded, failed)
    }

    fn write_action_result_as(
        &self,
        out: &mut impl Write,
        action: &str,
        verb: &str,
        succeeded: &[Process],
        failed: &[(Process, String)],
    ) -> Result<()> {
        // Past tense for human display: "kill" → "Killed", "stop" → "Stopped"
        let past_tense = match verb {
            "kill" => "Killed".to_string(),
            "stop" => "Stopped".to_string(),
            "freeze" => "Frozen".to_string(),
            "thaw" => "Thawed".to_string(),
            _ => format!("{}{}ed", verb[..1].to_uppercase(), &verb[1..]),
        };

        // the preview's list when the command gave it, else succeeded then failed
        let listed: Vec<&Process> = match &self.processes {
            Some(all) => all.iter().collect(),
            None => succeeded
                .iter()
                .chain(failed.iter().map(|(p, _)| p))
                .collect(),
        };
        let printed = match self.format {
            OutputFormat::Human => {
                Self::write_action_lines(out, &past_tense, verb, succeeded, failed)
            }
            OutputFormat::Json => self.write_json(
                out,
                &mut out::stderr(),
                &ActionOutput {
                    action,
                    success: failed.is_empty(),
                    succeeded_count: succeeded.len(),
                    failed_count: failed.len(),
                    not_found: self.not_found.as_deref(),
                    left_out: Some(&self.left_out),
                    succeeded,
                    failed: &failed
                        .iter()
                        .map(|(p, e)| FailedAction {
                            process: p,
                            error: e,
                        })
                        .collect::<Vec<_>>(),
                    processes: &listed
                        .iter()
                        .map(|p| ReceiptProcess::of(p, failed))
                        .collect::<Vec<_>>(),
                    count: listed.len(),
                },
            ),
        };
        // The action has been done; a result nobody can read (a closed pipe, a
        // full disk) is recorded by `out` and does not change what it was
        out::recorded(printed);

        if failed.is_empty() {
            Ok(())
        } else {
            Err(ProcError::reported(
                ExitCode::GeneralError,
                format!(
                    "Failed to {} {} process{}",
                    verb,
                    failed.len(),
                    plural(failed.len())
                ),
            ))
        }
    }

    /// The human lines of an action result: what worked, then what failed
    fn write_action_lines(
        out: &mut impl Write,
        past_tense: &str,
        verb: &str,
        succeeded: &[Process],
        failed: &[(Process, String)],
    ) -> std::io::Result<()> {
        if !succeeded.is_empty() {
            writeln!(
                out,
                "{} {} {} process{}",
                "✓".green().bold(),
                past_tense,
                succeeded.len().to_string().cyan().bold(),
                plural(succeeded.len())
            )?;
            for proc in succeeded {
                writeln!(
                    out,
                    "  {} {} [PID {}]",
                    "→".bright_black(),
                    proc.name.white(),
                    proc.pid.to_string().cyan()
                )?;
            }
        }
        if !failed.is_empty() {
            writeln!(
                out,
                "{} Failed to {} {} process{}",
                "✗".red().bold(),
                verb,
                failed.len(),
                plural(failed.len())
            )?;
            for (proc, err) in failed {
                writeln!(
                    out,
                    "  {} {} [PID {}]: {}",
                    "→".bright_black(),
                    proc.name.white(),
                    proc.pid.to_string().cyan(),
                    err.red()
                )?;
            }
        }
        Ok(())
    }

    /// Print dry-run summary for a destructive command.
    ///
    /// The `action` is the command name (e.g. "kill", "stop", "freeze", "thaw"),
    /// used both as the JSON `action` and as the verb in the human summary.
    pub fn print_dry_run(&self, action: &str, processes: &[Process]) {
        self.print_dry_run_with(
            action,
            processes,
            &format!(
                "Dry run: would {} {} process{}",
                action,
                processes.len(),
                plural(processes.len())
            ),
        );
    }

    /// Print dry-run summary with a custom human message.
    ///
    /// JSON output is `{"action", "success", "dry_run": true, "count", "not_found" (when the
    /// printer was given targets), "processes"}`.
    pub fn print_dry_run_with(&self, action: &str, processes: &[Process], message: &str) {
        match self.format {
            OutputFormat::Human => {
                // The thing a person approves: the whole command line
                self.print_processes_human(processes, None, true);
                self.warning(message);
            }
            OutputFormat::Json => self.print_json(&DryRunOutput {
                action,
                success: true,
                dry_run: true,
                count: processes.len(),
                not_found: self.not_found.as_deref(),
                left_out: Some(&self.left_out),
                processes,
            }),
        }
    }

    /// Show confirmation prompt and return whether the user confirmed.
    ///
    /// Returns `Ok(true)` if confirmed or skipped (--yes), `Ok(false)` if the
    /// user declined (prints "Cancelled"), and an error when there's nobody to
    /// ask (see [`must_confirm`]) or the list of what would change can't be
    /// shown to them (see [`prompt`]).
    pub fn ask_confirm(&self, action: &str, processes: &[Process], yes: bool) -> Result<bool> {
        let json = matches!(self.format, OutputFormat::Json);
        if !must_confirm(action, yes, json, processes, self.not_found.as_deref())? {
            return Ok(true);
        }
        // A person is being asked: `must_confirm` never gets here with --json
        let question = format!(
            "{}{} {} process{}?",
            action[..1].to_uppercase(),
            &action[1..],
            processes.len(),
            plural(processes.len())
        );
        let list = || self.print_confirmation(action, processes);
        let not_found = self.not_found.as_deref();
        let confirmed = prompt::ask(action, processes, not_found, &question, list)?;
        if !confirmed {
            self.warning("Cancelled");
        }
        Ok(confirmed)
    }

    /// Print a confirmation prompt showing processes about to be acted on:
    /// the process table, with the whole command lines
    pub fn print_confirmation(&self, action: &str, processes: &[Process]) {
        to_stdout(|out| self.write_confirmation(out, screen_now(), action, processes));
    }

    fn write_confirmation(
        &self,
        out: &mut impl Write,
        screen: Screen,
        action: &str,
        processes: &[Process],
    ) -> std::io::Result<()> {
        writeln!(
            out,
            "\n{} Found {} process{} to {}:\n",
            "⚠".yellow().bold(),
            processes.len().to_string().cyan().bold(),
            if processes.len() == 1 { "" } else { "es" },
            action
        )?;
        self.write_process_rows(out, screen, processes, true)
    }
}

// JSON output structures
#[derive(Serialize)]
struct EmptyResult<'a> {
    action: &'a str,
    success: bool,
    count: usize,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    not_found: Option<&'a [String]>,
    #[serde(skip_serializing_if = "none_left_out")]
    left_out: Option<&'a [crate::core::Ancestor]>,
}

/// No process was left out, so the document has no `left_out`
fn none_left_out(left_out: &Option<&[crate::core::Ancestor]>) -> bool {
    left_out.is_none_or(|left_out| left_out.is_empty())
}

#[derive(Serialize)]
struct ProcessListOutput<'a> {
    action: &'a str,
    success: bool,
    count: usize,
    processes: &'a [Process],
    #[serde(skip_serializing_if = "none_left_out")]
    left_out: Option<&'a [crate::core::Ancestor]>,
}

#[derive(Serialize)]
struct DryRunOutput<'a> {
    action: &'a str,
    success: bool,
    dry_run: bool,
    count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    not_found: Option<&'a [String]>,
    #[serde(skip_serializing_if = "none_left_out")]
    left_out: Option<&'a [crate::core::Ancestor]>,
    processes: &'a [Process],
}

#[derive(Serialize)]
struct ActionOutput<'a> {
    action: &'a str,
    success: bool,
    succeeded_count: usize,
    failed_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    not_found: Option<&'a [String]>,
    #[serde(skip_serializing_if = "none_left_out")]
    left_out: Option<&'a [crate::core::Ancestor]>,
    succeeded: &'a [Process],
    failed: &'a [FailedAction<'a>],
    /// Every process acted on, in the preview's order, each with its `outcome`
    processes: &'a [ReceiptProcess<'a>],
    /// How many of them
    count: usize,
}

/// A process of a receipt: the reading the receipt has always listed, then
/// what became of it
#[derive(Serialize)]
pub(crate) struct ReceiptProcess<'a> {
    #[serde(flatten)]
    process: &'a Process,
    /// `succeeded` or `failed`
    outcome: &'static str,
    /// Why it failed; absent when it succeeded
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
}

impl<'a> ReceiptProcess<'a> {
    /// `process` as the receipt lists it: failed when `failed` has it (by PID,
    /// which is unique in the list), succeeded otherwise
    pub(crate) fn of(process: &'a Process, failed: &'a [(Process, String)]) -> Self {
        let error = failed
            .iter()
            .find(|(p, _)| p.pid == process.pid)
            .map(|(_, error)| error.as_str());
        Self {
            process,
            outcome: if error.is_some() {
                "failed"
            } else {
                "succeeded"
            },
            error,
        }
    }
}

#[derive(Serialize)]
struct FailedAction<'a> {
    process: &'a Process,
    error: &'a str,
}

impl Default for Printer {
    fn default() -> Self {
        Self::new(OutputFormat::Human, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::assert_stops_at_a_failed_write;

    #[test]
    fn a_document_with_left_out_names_them() {
        let zsh = [crate::core::Ancestor {
            pid: 81340,
            name: "zsh".to_string(),
        }];
        let dry = serde_json::to_value(DryRunOutput {
            action: "kill",
            success: true,
            dry_run: true,
            count: 0,
            not_found: Some(&[]),
            left_out: Some(&zsh),
            processes: &[],
        })
        .unwrap();
        assert_eq!(
            dry["left_out"],
            serde_json::json!([{"pid": 81340, "name": "zsh"}])
        );
        let empty = serde_json::to_value(EmptyResult {
            action: "free",
            success: true,
            count: 0,
            message: "m",
            not_found: None,
            left_out: Some(&[]),
        })
        .unwrap();
        assert!(empty.get("left_out").is_none(), "{empty}");
        let receipt = serde_json::to_value(ActionOutput {
            action: "kill",
            success: true,
            succeeded_count: 0,
            failed_count: 0,
            not_found: None,
            left_out: None,
            succeeded: &[],
            failed: &[],
            processes: &[],
            count: 0,
        })
        .unwrap();
        assert!(receipt.get("left_out").is_none(), "{receipt}");
    }

    #[test]
    fn a_note_is_for_people_only() {
        let mut out = Vec::new();
        Printer::new(OutputFormat::Human, false)
            .write_note(&mut out, "Includes zsh (PID 1), which runs this proc")
            .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.ends_with("Includes zsh (PID 1), which runs this proc\n"),
            "{text:?}"
        );
        let mut out = Vec::new();
        Printer::new(OutputFormat::Json, false)
            .write_note(&mut out, "x")
            .unwrap();
        assert!(out.is_empty());
        assert_stops_at_a_failed_write(|w| {
            Printer::new(OutputFormat::Human, false).write_note(w, "x")
        });
    }

    #[test]
    fn a_note_is_plain_unless_stderr_is_a_terminal_and_no_color_is_unset() {
        let note = |colour| {
            let mut out = Vec::new();
            write_note(&mut out, "pass them apart", colour).unwrap();
            String::from_utf8(out).unwrap()
        };
        assert_eq!(note(false), "note: pass them apart\n");
        assert_eq!(note(true), "\x1b[1;36mnote:\x1b[0m pass them apart\n");

        let set = |v: &str| Some(std::ffi::OsString::from(v));
        assert!(note_in_colour(true, None));
        assert!(note_in_colour(true, set("")));
        assert!(!note_in_colour(true, set("1")));
        assert!(!note_in_colour(false, None));
    }

    #[test]
    fn json_never_stands_in_for_yes() {
        // --yes: go ahead without asking, in either output format
        assert!(!must_confirm("kill", true, false, &[], None).unwrap());
        assert!(!must_confirm("kill", true, true, &[], None).unwrap());
        // --json without --yes is refused (exit 4), never treated as a yes
        let refused = must_confirm("kill", false, true, &[], None).unwrap_err();
        assert!(
            matches!(refused, ProcError::ConfirmationRequired { .. }),
            "{refused}"
        );
        assert!(refused.to_string().contains("--yes"));
    }

    #[test]
    fn the_refusal_carries_the_targets_that_were_not_found() {
        let missing = ["99999999".to_string()];
        let refused = must_confirm("kill", false, true, &[], Some(&missing)).unwrap_err();
        let shown = format!("{refused:?}");
        assert!(shown.contains(r#"not_found: Some(["99999999"])"#));
        // a command without targets has nothing to say about them
        let refused = must_confirm("stuck", false, true, &[], None).unwrap_err();
        assert!(format!("{refused:?}").contains("not_found: None"));
    }

    use crate::core::ProcessStatus;

    /// What a writer-based print writes, with colours off
    fn written(print: impl FnOnce(&mut Vec<u8>) -> std::io::Result<()>) -> String {
        colored::control::set_override(false);
        let mut out = Vec::new();
        print(&mut out).unwrap();
        plain(&String::from_utf8(out).unwrap())
    }

    /// `text` without the escape sequences a table adds when run in a terminal
    fn plain(text: &str) -> String {
        regex::Regex::new("\x1b\\[[0-9;]*m")
            .unwrap()
            .replace_all(text, "")
            .into_owned()
    }

    fn process(pid: u32, name: &str) -> Process {
        Process {
            pid,
            name: name.to_string(),
            exe_path: None,
            cwd: None,
            command: None,
            argv0: None,
            raw: None,
            cpu_percent: 0.0,
            memory_mb: 0.0,
            status: ProcessStatus::Running,
            user: None,
            parent_pid: None,
            start_time: None,
            uptime_secs: None,
            parent_name: None,
            cpu_time_secs: Some(0),
            open_files: None,
            child_count: None,
            runs_proc: false,
        }
    }

    /// Output with every run of spaces collapsed, one entry per non-empty line,
    /// so a table row can be compared without its column padding
    fn rows(text: &str) -> Vec<String> {
        text.lines()
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|l| !l.is_empty())
            .collect()
    }

    fn human() -> Printer {
        Printer::new(OutputFormat::Human, false)
    }

    fn json() -> Printer {
        Printer::new(OutputFormat::Json, false)
    }

    #[test]
    fn success_and_warning_print_a_line_for_people_and_nothing_for_json() {
        assert_eq!(
            written(|w| human().write_success(w, "all done")),
            "✓ all done\n"
        );
        assert_eq!(
            written(|w| human().write_warning(w, "careful")),
            "⚠ careful\n"
        );
        assert_eq!(written(|w| json().write_success(w, "all done")), "");
        assert_eq!(written(|w| json().write_warning(w, "careful")), "");
    }

    /// The time the tests draw tables at
    const NOW: u64 = 1_000_000;

    /// A screen `width` columns wide, at [`NOW`]
    fn screen(width: u16) -> Screen {
        Screen {
            width,
            now_secs: NOW,
        }
    }

    /// A process that has run for `secs`
    fn up_for(mut p: Process, secs: u64) -> Process {
        p.start_time = Some(NOW - secs);
        p
    }

    #[test]
    fn no_processes_says_so_with_or_without_a_context() {
        let out = written(|w| human().write_processes_human(w, screen(120), &[], None, false));
        assert_eq!(out, "⚠ No processes found\n");
        let out =
            written(|w| human().write_processes_human(w, screen(120), &[], Some("in /srv"), false));
        assert_eq!(out, "⚠ No processes found in /srv\n");
    }

    #[test]
    fn the_headline_counts_processes_and_names_the_context() {
        let one = [process(42, "node")];
        let out =
            written(|w| human().write_processes_human(w, screen(120), &one, Some("in /a"), false));
        assert_eq!(out.lines().next(), Some("✓ Found 1 process in /a"));
        assert_eq!(out.lines().nth(1), Some(""));

        let two = [process(42, "node"), process(43, "deno")];
        let out = written(|w| human().write_processes_human(w, screen(120), &two, None, false));
        assert_eq!(out.lines().next(), Some("✓ Found 2 processes"));
    }

    /// A process with every column known
    fn known_process() -> Process {
        let mut p = up_for(process(42, "node"), 65);
        p.user = Some("zee".into());
        p.cwd = Some("/a/b".into());
        p.command = Some("node /x/y/server.js --port 3000".into());
        p.parent_pid = Some(7);
        p.cpu_percent = 1.25;
        p.memory_mb = 12.5;
        p
    }

    #[test]
    fn the_table_shows_pid_name_user_cpu_mem_uptime_command() {
        let out =
            written(|w| human().write_process_rows(w, screen(120), &[known_process()], false));
        assert_eq!(
            rows(&out),
            [
                "PID NAME USER CPU% MEM UPTIME COMMAND",
                "42 node zee 1.2 12.5MB 1m 5s node /x/y/server.js --port 3000"
            ]
        );
        assert!(out.ends_with("\n\n"), "{out:?}");
    }

    #[test]
    fn uptime_shows_at_any_width() {
        for width in [1, 20, 40, 80, 99, 100, 200] {
            let out = written(|w| {
                human().write_process_rows(w, screen(width), &[known_process()], false)
            });
            let rows = rows(&out);
            assert!(
                rows[0].starts_with("PID NAME USER CPU% MEM UPTIME COMMAND"),
                "{width} columns: {out}"
            );
            assert!(
                rows[1].starts_with("42 node zee 1.2 12.5MB 1m 5s "),
                "{width}: {out}"
            );
        }
    }

    #[test]
    fn a_narrow_terminal_cuts_the_command_and_nothing_else() {
        let out = written(|w| human().write_process_rows(w, screen(60), &[known_process()], false));
        // 60 columns less the other columns (39 with their padding), the
        // command's own padding and the slack leaves 18 for the command
        assert_eq!(
            rows(&out),
            [
                "PID NAME USER CPU% MEM UPTIME COMMAND",
                "42 node zee 1.2 12.5MB 1m 5s node /x/y/server.…"
            ]
        );
        for line in out.lines().filter(|l| !l.is_empty()) {
            assert!(line.chars().count() <= 60, "{line:?} overflows 60 columns");
        }
        // The command keeps a few columns however narrow the terminal is
        let out = written(|w| human().write_process_rows(w, screen(10), &[known_process()], false));
        let command = rows(&out)[1].split_once("1m 5s ").unwrap().1.to_string();
        assert_eq!(command, "node /x/y/s…");
    }

    #[test]
    fn wide_and_verbose_cut_nothing_and_a_whole_command_keeps_only_the_command() {
        let mut p = known_process();
        p.name = "a-process-with-a-very-long-name".into();
        p.cwd = Some("/srv/some/deeply/nested/working/directory/of/the/app".into());
        p.command = Some("a-process /x/y/server.js --listen-on-all-interfaces-please 3000".into());
        let command = "a-process /x/y/server.js --listen-on-all-interfaces-please 3000";

        let cut = written(|w| human().write_process_rows(w, screen(80), &[p.clone()], false));
        assert!(cut.contains('…'), "{cut}");
        assert!(!cut.contains(command), "{cut}");

        let wide = written(|w| {
            human()
                .wide(true)
                .write_process_rows(w, screen(80), &[p.clone()], false)
        });
        assert!(wide.contains(command) && !wide.contains('…'), "{wide}");

        let verbose = written(|w| {
            Printer::new(OutputFormat::Human, true).write_process_rows(
                w,
                screen(80),
                &[p.clone()],
                false,
            )
        });
        assert!(
            verbose.contains(command) && !verbose.contains('…'),
            "{verbose}"
        );
        assert!(verbose.contains("/srv/some/deeply/nested/working/directory/of/the/app"));

        // A list to approve keeps the command whole and still cuts the rest
        let approve = written(|w| {
            human()
                .scoped_to_dir(true)
                .write_process_rows(w, screen(80), &[p.clone()], true)
        });
        assert!(approve.contains(command), "{approve}");
        assert!(approve.contains('…'), "the directory is cut: {approve}");
    }

    #[test]
    fn verbose_adds_ppid_status_dir_uncut() {
        let verbose = Printer::new(OutputFormat::Human, true);
        let bare = process(43, "deno");
        let out =
            written(|w| verbose.write_process_rows(w, screen(20), &[known_process(), bare], false));
        assert_eq!(
            rows(&out),
            [
                "PID NAME USER CPU% MEM UPTIME PPID STATUS DIR COMMAND",
                "42 node zee 1.2 12.5MB 1m 5s 7 Running /a/b node /x/y/server.js --port 3000",
                "43 deno - 0.0 0KB - - Running - -"
            ]
        );
    }

    #[test]
    fn in_puts_dir_second() {
        let scoped = human().scoped_to_dir(true);
        let out = written(|w| scoped.write_process_rows(w, screen(120), &[known_process()], false));
        assert_eq!(
            rows(&out),
            [
                "PID DIR NAME USER CPU% MEM UPTIME COMMAND",
                "42 /a/b node zee 1.2 12.5MB 1m 5s node /x/y/server.js --port 3000"
            ]
        );
        // with -v too: the second column, not the one before COMMAND
        let verbose = Printer::new(OutputFormat::Human, true).scoped_to_dir(true);
        let out =
            written(|w| verbose.write_process_rows(w, screen(120), &[known_process()], false));
        assert_eq!(
            rows(&out),
            [
                "PID DIR NAME USER CPU% MEM UPTIME PPID STATUS COMMAND",
                "42 /a/b node zee 1.2 12.5MB 1m 5s 7 Running node /x/y/server.js --port 3000"
            ]
        );
    }

    #[test]
    fn the_directory_gives_way_to_the_command() {
        let dir = "/srv/some/deeply/nested/working/directory/of/the/app";
        let mut p = process(42, "node");
        p.cwd = Some(dir.into());
        p.command = Some("node ".to_string() + &"argument ".repeat(10));
        let scoped = human().scoped_to_dir(true);

        // Room for both: neither is cut
        let out = written(|w| scoped.write_process_rows(w, screen(200), &[p.clone()], false));
        assert!(rows(&out)[1].contains(dir), "{out}");
        assert!(!out.contains('…'), "{out}");

        // 80 columns: the other columns take 36, the directory gets 15 and the
        // command the 24 that are left
        let out = written(|w| scoped.write_process_rows(w, screen(80), &[p.clone()], false));
        let row = rows(&out)[1].clone();
        let cells: Vec<&str> = row.split(' ').collect();
        assert_eq!(cells[1], crate::ui::truncate_path(dir, 15), "{out}");
        let command = row.split_once(" - - 0.0 0KB - ").map(|(_, c)| c);
        assert!(command.is_none(), "the order is DIR NAME USER ...: {row}");
        let command = row.split_once("0KB - ").unwrap().1;
        assert_eq!(command.chars().count(), 24, "{out}");
        assert!(command.ends_with('…'), "{out}");

        // However wide the terminal, the directory gets a third of the line at most
        let out = written(|w| scoped.write_process_rows(w, screen(140), &[p.clone()], false));
        let cell = rows(&out)[1].split(' ').nth(1).unwrap().to_string();
        assert_eq!(cell, crate::ui::truncate_path(dir, 46), "{out}");

        // The directory is never cut below 12 columns
        let out = written(|w| scoped.write_process_rows(w, screen(30), &[p.clone()], false));
        assert_eq!(
            rows(&out)[1].split(' ').nth(1).unwrap().chars().count(),
            12,
            "{out}"
        );
    }

    #[test]
    fn wide_characters_are_measured_in_columns_not_chars() {
        use unicode_width::UnicodeWidthStr;
        let mut p = known_process();
        p.name = "日本語サーバー🚀".into();
        p.command =
            Some("日本語サーバー 🚀🚀🚀 ".to_string() + &"--listen-on-all-interfaces ".repeat(8));
        for width in [80u16, 100, 120] {
            let out =
                written(|w| human().write_process_rows(w, screen(width), &[p.clone()], false));
            for line in out.lines() {
                assert!(line.width() <= width as usize, "{line:?} overflows {width}");
            }
            assert!(out.contains('…'), "{out}");
        }
    }

    #[test]
    fn what_the_os_refused_to_read_shows_a_dash_in_cpu_and_mem() {
        // no CPU time and no memory: not read where the OS refuses (macOS, Windows)
        let mut p = process(42, "node");
        p.cpu_time_secs = None;
        let out = written(|w| human().write_process_rows(w, screen(120), &[p], false));
        let expected = if cfg!(any(target_os = "macos", windows)) {
            "42 node - - - - -"
        } else {
            "42 node - 0.0 0KB - -"
        };
        assert_eq!(rows(&out)[1], expected);
    }

    #[test]
    fn an_unknown_value_shows_a_dash() {
        let mut p = process(42, "node");
        p.user = Some(String::new());
        p.command = Some(String::new());
        let verbose = Printer::new(OutputFormat::Human, true);
        let out = written(|w| verbose.write_process_rows(w, screen(120), &[p.clone()], false));
        assert_eq!(rows(&out)[1], "42 node - 0.0 0KB - - Running - -");
        // Nothing at all known but the PID and name
        p.user = None;
        p.command = None;
        let out = written(|w| human().write_process_rows(w, screen(120), &[p], false));
        assert_eq!(rows(&out)[1], "42 node - 0.0 0KB - -");
    }

    #[test]
    fn the_verbose_table_names_every_status() {
        let statuses = [
            ProcessStatus::Running,
            ProcessStatus::Sleeping,
            ProcessStatus::Stopped,
            ProcessStatus::Zombie,
            ProcessStatus::Dead,
            ProcessStatus::Idle,
            ProcessStatus::Starting,
            ProcessStatus::DiskWait,
            ProcessStatus::Unknown,
        ];
        let list: Vec<Process> = statuses
            .iter()
            .enumerate()
            .map(|(i, status)| {
                let mut p = process(100 + i as u32, "node");
                p.status = *status;
                p
            })
            .collect();
        let verbose = Printer::new(OutputFormat::Human, true);
        let table = written(|w| verbose.write_process_rows(w, screen(120), &list, false));
        for (i, status) in statuses.iter().enumerate() {
            let name = match status {
                ProcessStatus::DiskWait => "Disk wait".to_string(),
                other => format!("{other:?}"),
            };
            let row = &rows(&table)[i + 1];
            assert!(row.contains(&format!(" {name} ")), "{table}");
        }
    }

    #[test]
    fn a_preview_shows_the_whole_command_line() {
        let mut p = known_process();
        p.command = Some("node ".to_string() + &"argument ".repeat(30));
        let command = p.command.clone().unwrap();

        // The dry run's table and the question's list
        let preview =
            written(|w| human().write_processes_human(w, screen(60), &[p.clone()], None, true));
        assert!(preview.contains(&command), "{preview}");
        assert!(!preview.contains('…'), "{preview}");
        let question = written(|w| human().write_confirmation(w, screen(60), "kill", &[p.clone()]));
        assert!(question.contains(&command), "{question}");
        assert_eq!(
            question.lines().next(),
            Some(""),
            "a blank line comes before the headline: {question:?}"
        );
        assert_eq!(
            question.lines().nth(1),
            Some("⚠ Found 1 process to kill:"),
            "{question:?}"
        );
        assert_eq!(rows(&question)[1], "PID NAME USER CPU% MEM UPTIME COMMAND");

        // ... where the same terminal cuts it in a list
        let list = written(|w| human().write_processes_human(w, screen(60), &[p], None, false));
        assert!(!list.contains(&command) && list.contains('…'), "{list}");
    }

    #[test]
    fn an_action_result_lists_what_worked_and_what_failed() {
        let ok = [process(1, "a"), process(2, "b")];
        let out = written(|w| {
            human()
                .write_action_result_as(w, "kill", "kill", &ok, &[])
                .map_err(std::io::Error::other)
        });
        assert_eq!(out, "✓ Killed 2 processes\n  → a [PID 1]\n  → b [PID 2]\n");

        let one = [process(1, "a")];
        let out = written(|w| {
            human()
                .write_action_result_as(w, "stop", "stop", &one, &[])
                .map_err(std::io::Error::other)
        });
        assert_eq!(out, "✓ Stopped 1 process\n  → a [PID 1]\n");

        // Mixed: both blocks, then the error a command exits with
        let failed = [(process(3, "c"), "Permission denied".to_string())];
        let mut buf = Vec::new();
        let err = human()
            .write_action_result_as(&mut buf, "freeze", "freeze", &one, &failed)
            .unwrap_err();
        assert_eq!(
            plain(&String::from_utf8(buf).unwrap()),
            "✓ Frozen 1 process\n  → a [PID 1]\n\
             ✗ Failed to freeze 1 process\n  → c [PID 3]: Permission denied\n"
        );
        assert_eq!(err.to_string(), "Failed to freeze 1 process");
        assert_eq!(ExitCode::from(&err) as i32, 1);
    }

    #[test]
    fn only_failures_print_no_success_block_and_pluralize() {
        let failed = [
            (process(3, "c"), "gone".to_string()),
            (process(4, "d"), "busy".to_string()),
        ];
        let mut buf = Vec::new();
        let err = human()
            .write_action_result_as(&mut buf, "thaw", "thaw", &[], &failed)
            .unwrap_err();
        assert_eq!(
            plain(&String::from_utf8(buf).unwrap()),
            "✗ Failed to thaw 2 processes\n  → c [PID 3]: gone\n  → d [PID 4]: busy\n"
        );
        assert_eq!(err.to_string(), "Failed to thaw 2 processes");
    }

    #[test]
    fn a_verb_without_its_own_past_tense_gets_ed() {
        let one = [process(1, "a")];
        let out = written(|w| {
            human()
                .write_action_result_as(w, "restart", "restart", &one, &[])
                .map_err(std::io::Error::other)
        });
        assert!(out.starts_with("✓ Restarted 1 process\n"), "{out}");
    }

    #[test]
    fn json_action_results_carry_the_failures() {
        let ok = [process(1, "a")];
        let failed = [(process(3, "c"), "Permission denied".to_string())];
        // the preview listed the failed one first
        let all = [failed[0].0.clone(), ok[0].clone()];
        let mut buf = Vec::new();
        let err = json()
            .with_processes(&all)
            .write_action_result_as(&mut buf, "orphans", "kill", &ok, &failed)
            .unwrap_err();
        let doc: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert_eq!(doc["action"], "orphans");
        assert_eq!(doc["success"], false);
        assert_eq!(doc["succeeded_count"], 1);
        assert_eq!(doc["failed_count"], 1);
        assert_eq!(doc["succeeded"][0]["pid"], 1);
        assert_eq!(doc["failed"][0]["process"]["pid"], 3);
        assert_eq!(doc["failed"][0]["error"], "Permission denied");
        // `processes` lists both in the preview's order, each with its outcome
        assert_eq!(doc["count"], 2);
        assert_eq!(doc["processes"][0]["pid"], 3);
        assert_eq!(doc["processes"][0]["name"], "c");
        assert_eq!(doc["processes"][0]["outcome"], "failed");
        assert_eq!(doc["processes"][0]["error"], "Permission denied");
        assert_eq!(doc["processes"][1]["pid"], 1);
        assert_eq!(doc["processes"][1]["outcome"], "succeeded");
        assert!(doc["processes"][1].get("error").is_none());
        // the old keys keep their places; the new ones come after them
        let keys: Vec<&str> = text
            .lines()
            .filter_map(|line| line.strip_prefix("  \""))
            .filter_map(|rest| rest.split('"').next())
            .collect();
        assert_eq!(
            keys,
            [
                "action",
                "success",
                "succeeded_count",
                "failed_count",
                "succeeded",
                "failed",
                "processes",
                "count"
            ]
        );
        assert_eq!(err.to_string(), "Failed to kill 1 process");
    }

    #[test]
    fn without_the_preview_a_receipt_lists_the_succeeded_then_the_failed() {
        let ok = [process(1, "a"), process(2, "b")];
        let failed = [(process(3, "c"), "no".to_string())];
        let mut buf = Vec::new();
        json()
            .write_action_result_as(&mut buf, "kill", "kill", &ok, &failed)
            .unwrap_err();
        let doc: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        let listed: Vec<(u64, &str)> = doc["processes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p["pid"].as_u64().unwrap(), p["outcome"].as_str().unwrap()))
            .collect();
        assert_eq!(listed, [(1, "succeeded"), (2, "succeeded"), (3, "failed")]);
        assert_eq!(doc["count"], 3);
    }

    #[test]
    fn a_value_that_cannot_be_serialized_is_reported_on_stderr() {
        struct Broken;
        impl Serialize for Broken {
            fn serialize<S: serde::Serializer>(
                &self,
                _: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("no can do"))
            }
        }
        let (mut out, mut err) = (Vec::new(), Vec::new());
        json().write_json(&mut out, &mut err, &Broken).unwrap();
        assert!(out.is_empty());
        assert_eq!(
            String::from_utf8(err).unwrap(),
            "Failed to serialize JSON: no can do\n"
        );

        let (mut out, mut err) = (Vec::new(), Vec::new());
        json()
            .write_json(&mut out, &mut err, &serde_json::json!({"a": 1}))
            .unwrap();
        assert!(err.is_empty());
        assert_eq!(String::from_utf8(out).unwrap(), "{\n  \"a\": 1\n}\n");
    }

    #[test]
    fn the_default_printer_is_plain_human_output() {
        let printer = Printer::default();
        assert!(matches!(printer.format, OutputFormat::Human));
        assert!(!printer.verbose);
        assert!(!printer.wide);
    }

    #[test]
    fn a_failed_write_ends_the_table() {
        let p = known_process();
        assert_stops_at_a_failed_write(|w| {
            human().write_processes_human(
                w,
                screen(120),
                std::slice::from_ref(&p),
                Some("in /a"),
                false,
            )
        });
        assert_stops_at_a_failed_write(|w| {
            human().write_processes_human(w, screen(120), &[], None, false)
        });
    }

    #[test]
    fn a_failed_write_ends_the_verbose_table() {
        let p = known_process();
        let verbose = Printer::new(OutputFormat::Human, true);
        assert_stops_at_a_failed_write(|w| {
            verbose.write_process_rows(w, screen(120), std::slice::from_ref(&p), false)
        });
    }

    #[test]
    fn a_failed_write_ends_the_confirmation_list() {
        let p = known_process();
        assert_stops_at_a_failed_write(|w| {
            human().write_confirmation(w, screen(120), "kill", std::slice::from_ref(&p))
        });
    }

    #[test]
    fn a_failed_write_ends_the_action_result() {
        let ok = [process(1, "a")];
        let failed = [(process(3, "c"), "Permission denied".to_string())];
        assert_stops_at_a_failed_write(|w| {
            Printer::write_action_lines(w, "Killed", "kill", &ok, &failed)
        });
        assert_stops_at_a_failed_write(|w| human().write_success(w, "done"));
        assert_stops_at_a_failed_write(|w| human().write_warning(w, "careful"));
    }
}
