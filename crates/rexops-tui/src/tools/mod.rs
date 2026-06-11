//! Tool catalog, run mode, and launch orchestration.

pub mod catalog;
pub mod launcher;

pub use catalog::{is_streamable, RunMode, ToolEntry, CATALOG};
pub use launcher::{launch_tool, resolve_command, ChildExit, ForegroundRunner};
