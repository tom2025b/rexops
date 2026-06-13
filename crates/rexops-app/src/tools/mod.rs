//! Tool catalog, run-mode metadata, and launch orchestration.
//!
//! One source of truth for known tools, launch commands, and launch reports.
//! Terminal handoff stays behind a front-end supplied runner.

pub mod availability;
pub mod catalog;
pub mod launcher;

pub use availability::{Availability, AvailabilityTag};
pub use catalog::{by_id, is_streamable, RunMode, ToolEntry, CATALOG};
pub use launcher::{
    launch_tool, resolve_command, resolve_launch_command, ChildExit, ForegroundRunner,
    LaunchCommand, LaunchReport,
};
