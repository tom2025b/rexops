//! Tool catalog, run mode, and launch orchestration.

pub mod catalog;
pub mod launcher;

pub use catalog::{is_streamable, launchable, refreshes_after};
// `resolve_launch_command` is the single public entry point for "what runs when
// this tool launches" — program plus registry args. Both run surfaces (the
// foreground launcher and the background job manager) and the confirm-gate
// preview go through it, so they can never disagree about the invocation.
// It now lives in `rexops_app::launch` (shared with the `rexops launch` CLI) and
// is re-exported through `launcher`, so these call sites are unchanged.
pub use launcher::{
    launch_tool, resolve_launch_command, ChildExit, ForegroundRunner, LaunchCommand,
};
