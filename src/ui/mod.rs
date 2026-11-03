//! User interface components for proc CLI
//!
//! Handles output formatting, colors, and interactive prompts.

pub mod card;
pub mod format;
pub mod interrupt;
pub mod out;
pub mod output;
pub mod prompt;
pub mod table;

pub use format::{
    colorize_status, format_duration, format_memory, plural, truncate_path, truncate_string,
    uptime_label,
};
pub use output::{must_confirm, print_note, OutputFormat, Printer};
