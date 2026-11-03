//! CLI command implementations
//!
//! Each command is implemented in its own module and follows a consistent pattern:
//! - Parse arguments from clap
//! - Execute the operation
//! - Format and display results

pub mod by;
pub mod find_in;
pub mod for_file;
pub mod free;
pub mod freeze;
pub(crate) mod held;
pub mod info;
pub mod kill;
pub mod list;
pub mod on;
pub mod orphans;
pub mod ports;
pub mod stop;
pub mod stuck;
pub mod thaw;
pub mod tree;
pub mod unstick;
pub mod wait;
pub mod watch;
pub mod why;

pub use by::ByCommand;
pub use find_in::InCommand;
pub use for_file::ForCommand;
pub use free::FreeCommand;
pub use freeze::FreezeCommand;
pub use info::InfoCommand;
pub use kill::KillCommand;
pub use list::ListCommand;
pub use on::OnCommand;
pub use orphans::OrphansCommand;
pub use ports::PortsCommand;
pub use stop::StopCommand;
pub use stuck::StuckCommand;
pub use thaw::ThawCommand;
pub use tree::TreeCommand;
pub use unstick::UnstickCommand;
pub use wait::WaitCommand;
pub use watch::WatchCommand;
pub use why::WhyCommand;
