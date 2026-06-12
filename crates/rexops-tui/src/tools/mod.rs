//! Tool catalog, run mode, and launch orchestration.

pub mod catalog;
pub mod launcher;

pub use catalog::{is_streamable, ToolEntry, CATALOG};
pub use launcher::{
    launch_tool, resolve_command, resolve_launch_command, ChildExit, ForegroundRunner,
    LaunchCommand,
};
