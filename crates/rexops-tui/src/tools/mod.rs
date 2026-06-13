//! Tool catalog, run mode, and launch orchestration.
//!
//! The catalog now lives in `rexops_app` (shared business logic). This module
//! re-exports it so TUI call sites (`crate::tools::CATALOG`, etc.) are
//! unchanged. Launch orchestration still lives here in `launcher`.

pub mod launcher;

pub use rexops_app::{is_streamable, ToolEntry, CATALOG};
// `resolve_launch_command` is the single public entry point for "what runs when
// this tool launches" — program plus catalog args. Both run surfaces (the
// foreground launcher and the background job manager) and the confirm-gate
// preview go through it, so they can never disagree about the invocation.
// `resolve_command` (program only) stays an internal helper of `launcher`.
pub use launcher::{
    launch_tool, resolve_launch_command, ChildExit, ForegroundRunner, LaunchCommand,
};
