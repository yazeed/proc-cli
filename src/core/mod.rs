//! Core abstractions for process and port management
//!
//! This module provides cross-platform abstractions for working with
//! system processes and network ports.

pub mod apply;
pub mod connections;
pub mod filters;
pub mod name_match;
pub mod port;
pub mod process;
pub mod target;

pub use apply::apply_to_each;
pub use connections::{connections_to, Connections, Peer, PeerProcess};
pub use filters::{
    apply_filters, guard_targets, matches_by_filter, matches_in_filter, require_in_dir,
    require_in_dir_live, resolve_in_dir, ruled_out_by, sort_ports, sort_processes, Guards,
    PortSortKey, SortKey, StatusFilter,
};
pub use name_match::{classify, matches_name, Hit, NameMatch, Names};
pub use port::{parse_port, Exposure, PortInfo, Protocol};
#[cfg(unix)]
pub use process::parse_signal_name;
pub use process::{escape_control, is_thread, Process, ProcessStatus, RawText};
pub use target::{
    comma_notice, find_ports_for_pid, flatten_targets, parse_target, parse_targets, resolve_target,
    resolve_targets, resolve_targets_excluding_self, validate_target_arg, with_ports, Resolved,
    TargetType,
};
