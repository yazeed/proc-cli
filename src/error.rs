//! Error types for proc CLI
//!
//! Provides structured error handling with helpful suggestions for users.

use crate::core::Process;
use thiserror::Error;

/// Main error type for proc operations
#[derive(Error, Debug)]
pub enum ProcError {
    /// No process found matching the given target
    #[error("No process found matching '{0}'\n  Try: proc list to list all processes")]
    ProcessNotFound(String),

    /// No process has this name, but some have it in their command line, which
    /// `--cmd` would match
    #[error("No process found matching '{name}'\n  {}\n  Try: proc list to list all processes", command_line_hint(.name, *.in_command_line))]
    NameNotFound {
        /// The name that was looked for
        name: String,
        /// How many processes have it in their command line (at least one)
        in_command_line: usize,
    },

    /// A target exists but its guards (`--by`, `--in`, `--start-time`) ruled
    /// out everything it matched. The target sits in the quotes and the guards
    /// that ruled it out follow them.
    #[error(
        "No process found matching '{target}' (ruled out by {})\n  Try: proc list to list all processes",
        guard_list(.ruled_out_by)
    )]
    FilteredOut {
        /// The target as typed (`node`, `:3000`, `1234`)
        target: String,
        /// Each guard that ruled it out: its flag and the value typed
        /// (`("--by", "node")`)
        ruled_out_by: Vec<(String, String)>,
    },

    /// No process is listening on the specified port
    #[error("No process listening on port {0}\n  Try: proc ports")]
    PortNotFound(u16),

    /// proc found no holder of the port, but one may be there: something
    /// listens on it and belongs to another user, or the system hides other
    /// users' sockets. Root sees them
    #[error("{0}\n  Try: sudo proc <command>")]
    PortUnseen(crate::core::UnseenPort),

    /// Insufficient permissions to operate on the process
    #[error("Permission denied for PID {0}\n  Try: sudo proc <command>")]
    PermissionDenied(u32),

    /// User provided invalid input or arguments
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// A command that changes processes was run without `--yes` where nobody
    /// can be asked (`--json`, or no terminal). Carries the processes it would
    /// have changed so `--json` callers can see them in the error.
    ///
    /// Not shown as invalid input: the call is valid, it only needs a yes, and
    /// the JSON `reason` (`needs_confirmation`) says so.
    #[error("{message}")]
    ConfirmationRequired {
        /// What to pass to proceed
        message: String,
        /// The processes the command would have changed
        processes: Vec<Process>,
        /// The targets that matched nothing, for a command that takes targets
        /// (`None` for one that finds its own processes)
        not_found: Option<Vec<String>>,
    },

    /// An underlying system call failed
    #[error("System error: {0}")]
    SystemError(String),

    /// The operation exceeded the allowed time limit
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Failed to parse input or system output
    #[error("Failed to parse: {0}")]
    ParseError(String),

    /// Feature is not available on the current platform
    #[error("Not supported on this platform: {0}")]
    NotSupported(String),

    /// The target process terminated during the operation
    #[error("Process {0} is no longer running")]
    ProcessGone(u32),

    /// Failed to send a signal to the process
    #[error("Signal failed: {0}")]
    SignalError(String),

    /// The command already printed its result, but the operation did not fully
    /// succeed (some processes failed, a port stayed busy, a wait timed out).
    /// `main` exits with `exit_code` without printing anything further.
    #[error("{message}")]
    Reported {
        /// Human-readable summary of what did not succeed
        message: String,
        /// Exit code to return
        exit_code: ExitCode,
    },
}

/// The `reason` of an exit-1 JSON error: proc itself could not do its part,
/// such as reading the process or port table
pub const REASON_SYSTEM_ERROR: &str = "system_error";

/// The `reason` of an exit-2 JSON error: no process holds that PID, name or
/// port (any more)
pub const REASON_NOT_FOUND: &str = "not_found";

/// The `reason` of an exit-2 JSON error for a name that no process has, though
/// some have it in their command line (`--cmd` would match those)
pub const REASON_COMMAND_LINE_ONLY: &str = "command_line_only";

/// The `reason` of an exit-2 JSON error for a target whose guards (`--by`,
/// `--in`, `--start-time`) ruled out every process it found; `ruled_out_by`
/// names them
pub const REASON_RULED_OUT: &str = "ruled_out";

/// The `reason` of an exit-3 JSON error
pub const REASON_PERMISSION_DENIED: &str = "permission_denied";

/// The `reason` of an exit-4 JSON error caused by bad flags, values or
/// arguments, including clap's own usage errors
pub const REASON_INVALID_INPUT: &str = "invalid_input";

/// The `reason` of an exit-4 JSON error for a command that changes processes
/// and was not given `--yes`
pub const REASON_NEEDS_CONFIRMATION: &str = "needs_confirmation";

/// The `reason` of an exit-4 JSON error for a command this OS does not support
pub const REASON_NOT_SUPPORTED: &str = "not_supported";

/// Guards as the message names them: `--by node and --start-time 1700000000`
fn guard_list(guards: &[(String, String)]) -> String {
    let named: Vec<String> = guards
        .iter()
        .map(|(flag, value)| format!("{flag} {value}"))
        .collect();
    named.join(" and ")
}

/// The hint for a name that matched nothing by name but is in some command lines
pub fn command_line_hint(name: &str, count: usize) -> String {
    let (who, has, its) = if count == 1 {
        ("process", "has", "its")
    } else {
        ("processes", "have", "their")
    };
    format!("{count} {who} {has} '{name}' in {its} command line (add --cmd to match those)")
}

impl ProcError {
    /// The `--cmd` hint, when this error carries one
    pub fn command_line_hint(&self) -> Option<String> {
        match self {
            ProcError::NameNotFound {
                name,
                in_command_line,
            } => Some(command_line_hint(name, *in_command_line)),
            _ => None,
        }
    }

    /// Why this error happened, as the `reason` of the JSON error: a fixed word
    /// a caller branches on instead of reading the message. Every error has
    /// one except [`ProcError::Reported`], whose command already printed its
    /// result and prints no error.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            ProcError::ProcessNotFound(_)
            | ProcError::PortNotFound(_)
            | ProcError::ProcessGone(_) => Some(REASON_NOT_FOUND),
            ProcError::NameNotFound { .. } => Some(REASON_COMMAND_LINE_ONLY),
            ProcError::FilteredOut { .. } => Some(REASON_RULED_OUT),
            ProcError::PermissionDenied(_) | ProcError::PortUnseen(_) => {
                Some(REASON_PERMISSION_DENIED)
            }
            ProcError::InvalidInput(_) => Some(REASON_INVALID_INPUT),
            ProcError::ConfirmationRequired { .. } => Some(REASON_NEEDS_CONFIRMATION),
            ProcError::NotSupported(_) => Some(REASON_NOT_SUPPORTED),
            ProcError::SystemError(_)
            | ProcError::Timeout(_)
            | ProcError::ParseError(_)
            | ProcError::SignalError(_) => Some(REASON_SYSTEM_ERROR),
            ProcError::Reported { .. } => None,
        }
    }

    /// The flags of the guards that ruled the target out, for the JSON's
    /// `ruled_out_by`; `None` for every other error
    pub fn ruled_out_by(&self) -> Option<Vec<&str>> {
        match self {
            ProcError::FilteredOut { ruled_out_by, .. } => {
                Some(ruled_out_by.iter().map(|(flag, _)| flag.as_str()).collect())
            }
            _ => None,
        }
    }

    /// The error for a name that matched no process: [`ProcError::NameNotFound`]
    /// when `in_command_line` processes have it in their command line (which
    /// `--cmd` would match), [`ProcError::ProcessNotFound`] when none do.
    pub fn name_not_found(name: &str, in_command_line: usize) -> Self {
        if in_command_line > 0 {
            ProcError::NameNotFound {
                name: name.to_string(),
                in_command_line,
            }
        } else {
            ProcError::ProcessNotFound(name.to_string())
        }
    }

    /// Creates a [`ProcError::Reported`] for a result that has already been printed.
    pub fn reported(exit_code: ExitCode, message: impl Into<String>) -> Self {
        ProcError::Reported {
            message: message.into(),
            exit_code,
        }
    }
}

impl From<std::io::Error> for ProcError {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::PermissionDenied => ProcError::PermissionDenied(0),
            std::io::ErrorKind::NotFound => ProcError::ProcessNotFound("unknown".to_string()),
            _ => ProcError::SystemError(err.to_string()),
        }
    }
}

impl From<serde_json::Error> for ProcError {
    fn from(err: serde_json::Error) -> Self {
        ProcError::ParseError(err.to_string())
    }
}

impl From<regex::Error> for ProcError {
    fn from(err: regex::Error) -> Self {
        ProcError::InvalidInput(format!("Invalid pattern: {}", err))
    }
}

impl From<dialoguer::Error> for ProcError {
    fn from(err: dialoguer::Error) -> Self {
        ProcError::SystemError(format!("Dialog error: {}", err))
    }
}

/// Result type alias for proc operations
pub type Result<T> = std::result::Result<T, ProcError>;

/// Exit codes for CLI
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    /// Operation completed successfully
    Success = 0,
    /// A general error occurred
    GeneralError = 1,
    /// The requested process or port was not found
    NotFound = 2,
    /// Operation requires elevated privileges
    PermissionDenied = 3,
    /// Invalid arguments or input provided
    InvalidInput = 4,
}

impl From<&ProcError> for ExitCode {
    fn from(err: &ProcError) -> Self {
        match err {
            ProcError::ProcessNotFound(_)
            | ProcError::NameNotFound { .. }
            | ProcError::FilteredOut { .. }
            | ProcError::PortNotFound(_)
            | ProcError::ProcessGone(_) => ExitCode::NotFound,
            ProcError::PermissionDenied(_) | ProcError::PortUnseen(_) => ExitCode::PermissionDenied,
            ProcError::InvalidInput(_)
            | ProcError::NotSupported(_)
            | ProcError::ConfirmationRequired { .. } => ExitCode::InvalidInput,
            ProcError::Reported { exit_code, .. } => *exit_code,
            _ => ExitCode::GeneralError,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_supported_on_this_os_is_invalid_input() {
        // "Can't do this here" has one exit code, the same as an unusable
        // flag value such as `stop --signal` on Windows
        let err = ProcError::NotSupported("freeze".to_string());
        assert_eq!(ExitCode::from(&err) as i32, 4);
    }

    fn exit_code(err: ProcError) -> i32 {
        ExitCode::from(&err) as i32
    }

    #[test]
    fn every_error_has_its_message_and_exit_code() {
        let confirmation = ProcError::ConfirmationRequired {
            message: "Pass --yes".to_string(),
            processes: Vec::new(),
            not_found: None,
        };
        let filtered = ProcError::FilteredOut {
            target: "1234".to_string(),
            ruled_out_by: vec![
                ("--by".to_string(), "node".to_string()),
                ("--start-time".to_string(), "1700000000".to_string()),
            ],
        };
        let cases: Vec<(ProcError, &str, i32, Option<&str>)> = vec![
            (
                ProcError::ProcessNotFound("node".to_string()),
                "No process found matching 'node'\n  Try: proc list to list all processes",
                2,
                Some("not_found"),
            ),
            (
                ProcError::NameNotFound {
                    name: "serve".to_string(),
                    in_command_line: 2,
                },
                "No process found matching 'serve'\n  2 processes have 'serve' in their command line (add --cmd to match those)\n  Try: proc list to list all processes",
                2,
                Some("command_line_only"),
            ),
            (
                filtered,
                "No process found matching '1234' (ruled out by --by node and --start-time 1700000000)\n  Try: proc list to list all processes",
                2,
                Some("ruled_out"),
            ),
            (
                ProcError::PortNotFound(3000),
                "No process listening on port 3000\n  Try: proc ports",
                2,
                Some("not_found"),
            ),
            (
                ProcError::PortUnseen(crate::core::UnseenPort {
                    port: 80,
                    why: crate::core::Unseen::OtherUser,
                }),
                "Port 80 is in use, but proc can't see which process holds it: it belongs to another user\n  Try: sudo proc <command>",
                3,
                Some("permission_denied"),
            ),
            (
                ProcError::PortUnseen(crate::core::UnseenPort {
                    port: 22,
                    why: crate::core::Unseen::Hidden("security.bsd.see_other_uids=0"),
                }),
                "proc can't tell whether port 22 is in use: this system hides other users' sockets (security.bsd.see_other_uids=0)\n  Try: sudo proc <command>",
                3,
                Some("permission_denied"),
            ),
            (
                ProcError::PermissionDenied(42),
                "Permission denied for PID 42\n  Try: sudo proc <command>",
                3,
                Some("permission_denied"),
            ),
            (
                ProcError::InvalidInput("bad".to_string()),
                "Invalid input: bad",
                4,
                Some("invalid_input"),
            ),
            (confirmation, "Pass --yes", 4, Some("needs_confirmation")),
            (
                ProcError::SystemError("boom".to_string()),
                "System error: boom",
                1,
                Some("system_error"),
            ),
            (
                ProcError::Timeout("10s".to_string()),
                "Operation timed out: 10s",
                1,
                Some("system_error"),
            ),
            (
                ProcError::ParseError("x".to_string()),
                "Failed to parse: x",
                1,
                Some("system_error"),
            ),
            (
                ProcError::NotSupported("freeze".to_string()),
                "Not supported on this platform: freeze",
                4,
                Some("not_supported"),
            ),
            // every holder of a port ended between listing it and looking it up
            (
                ProcError::ProcessGone(7),
                "Process 7 is no longer running",
                2,
                Some("not_found"),
            ),
            (
                ProcError::SignalError("EPERM".to_string()),
                "Signal failed: EPERM",
                1,
                Some("system_error"),
            ),
            // printed its own result; no error is printed, so no reason
            (
                ProcError::reported(ExitCode::NotFound, "nothing matched"),
                "nothing matched",
                2,
                None,
            ),
        ];
        assert_eq!(ExitCode::Success as i32, 0);
        for (err, message, code, reason) in cases {
            assert_eq!(err.to_string(), message);
            assert_eq!(err.reason(), reason, "{message}");
            assert_eq!(exit_code(err), code, "{message}");
        }
    }

    #[test]
    fn only_a_guard_error_names_the_guards_in_json() {
        let filtered = ProcError::FilteredOut {
            target: "1234".to_string(),
            ruled_out_by: vec![("--in".to_string(), "/app".to_string())],
        };
        assert_eq!(filtered.ruled_out_by(), Some(vec!["--in"]));
        assert_eq!(
            ProcError::ProcessNotFound("1".to_string()).ruled_out_by(),
            None
        );
    }

    #[test]
    fn each_exit_4_error_has_its_own_reason() {
        let confirmation = ProcError::ConfirmationRequired {
            message: "Pass --yes".to_string(),
            processes: Vec::new(),
            not_found: None,
        };
        assert_eq!(
            ProcError::InvalidInput("bad".to_string()).reason(),
            Some("invalid_input")
        );
        assert_eq!(confirmation.reason(), Some("needs_confirmation"));
        assert_eq!(
            ProcError::NotSupported("freeze".to_string()).reason(),
            Some("not_supported")
        );
        // the constants are what clap's usage errors use too
        assert_eq!(REASON_INVALID_INPUT, "invalid_input");
        assert_eq!(REASON_NEEDS_CONFIRMATION, "needs_confirmation");
        assert_eq!(REASON_NOT_SUPPORTED, "not_supported");
        assert_eq!(REASON_SYSTEM_ERROR, "system_error");
        assert_eq!(REASON_NOT_FOUND, "not_found");
        assert_eq!(REASON_COMMAND_LINE_ONLY, "command_line_only");
        assert_eq!(REASON_RULED_OUT, "ruled_out");
        assert_eq!(REASON_PERMISSION_DENIED, "permission_denied");
    }

    #[test]
    fn only_a_name_that_is_in_command_lines_carries_the_cmd_hint() {
        let one = ProcError::NameNotFound {
            name: "serve".to_string(),
            in_command_line: 1,
        };
        assert_eq!(
            one.command_line_hint().unwrap(),
            "1 process has 'serve' in its command line (add --cmd to match those)"
        );
        let many = ProcError::NameNotFound {
            name: "serve".to_string(),
            in_command_line: 3,
        };
        assert_eq!(
            many.command_line_hint().unwrap(),
            "3 processes have 'serve' in their command line (add --cmd to match those)"
        );
        assert_eq!(
            ProcError::ProcessNotFound("serve".to_string()).command_line_hint(),
            None
        );
    }

    #[test]
    fn a_reported_error_keeps_the_exit_code_it_was_given() {
        for code in [
            ExitCode::Success,
            ExitCode::GeneralError,
            ExitCode::NotFound,
            ExitCode::PermissionDenied,
            ExitCode::InvalidInput,
        ] {
            let err = ProcError::reported(code, "done");
            assert_eq!(ExitCode::from(&err), code);
        }
    }

    #[test]
    fn an_io_error_becomes_the_matching_proc_error() {
        use std::io::{Error, ErrorKind};
        let denied = ProcError::from(Error::from(ErrorKind::PermissionDenied));
        assert!(
            matches!(denied, ProcError::PermissionDenied(0)),
            "{denied:?}"
        );
        let missing = ProcError::from(Error::from(ErrorKind::NotFound));
        assert!(
            matches!(&missing, ProcError::ProcessNotFound(t) if t == "unknown"),
            "{missing:?}"
        );
        let other = ProcError::from(Error::new(ErrorKind::BrokenPipe, "pipe closed"));
        assert!(
            matches!(&other, ProcError::SystemError(m) if m == "pipe closed"),
            "{other:?}"
        );
    }

    #[test]
    fn a_json_error_becomes_a_parse_error() {
        let source = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let expected = source.to_string();
        let err = ProcError::from(source);
        assert!(
            matches!(&err, ProcError::ParseError(m) if *m == expected),
            "{err:?}"
        );
    }

    #[test]
    fn a_bad_regex_becomes_invalid_input() {
        // Built at run time: clippy rejects a literal that is not a valid regex
        let pattern = String::from("(");
        let source = regex::Regex::new(&pattern).unwrap_err();
        let err = ProcError::from(source);
        assert!(
            matches!(&err, ProcError::InvalidInput(m) if m.starts_with("Invalid pattern: ")),
            "{err:?}"
        );
        assert_eq!(exit_code(err), 4);
    }

    #[test]
    fn a_dialog_error_becomes_a_system_error() {
        let source = dialoguer::Error::IO(std::io::Error::other("no terminal"));
        let err = ProcError::from(source);
        assert!(
            matches!(&err, ProcError::SystemError(m) if m == "Dialog error: IO error: no terminal"),
            "{err:?}"
        );
    }
}
