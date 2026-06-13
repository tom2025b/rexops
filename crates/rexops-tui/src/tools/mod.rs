//! Tool catalog, run mode, and launch orchestration.

pub mod catalog;
pub mod launcher;

pub use catalog::{is_streamable, ToolEntry, CATALOG};
// `resolve_launch_command` is the single public entry point for "what runs when
// this tool launches" — program plus catalog args. Both run surfaces (the
// foreground launcher and the background job manager) and the confirm-gate
// preview go through it, so they can never disagree about the invocation.
// `resolve_command` (program only) stays an internal helper of `launcher`.
pub use launcher::{
    launch_tool, resolve_launch_command, ChildExit, ForegroundRunner, LaunchCommand,
};
