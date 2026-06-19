//! Tool catalog, run mode, and launch orchestration.
//!
//! The catalog and launch orchestration now live in `rexops_app` (shared
//! business logic). This module re-exports them so TUI call sites
//! (`crate::tools::…`) are unchanged. The terminal-touching
//! `ForegroundRunner` impl stays in the TUI.

pub(crate) use rexops_app::{is_streamable, AvailabilityTag, ToolEntry, CATALOG};
// `resolve_launch_command` is the single public entry point for "what runs when
// this tool launches" — program plus catalog args. Both run surfaces (the
// foreground launcher and the background job manager) and the confirm-gate
// preview go through it, so they can never disagree about the invocation.
pub(crate) use rexops_app::{
    launch_tool, resolve_launch_command, ChildExit, ForegroundRunner, LaunchCommand,
};

/// Render-boundary wording for the domain availability verdict. The TUI owns the
/// labels (the launcher rows and the palette both render these); `rexops-app`
/// stays UI-free and only emits the [`AvailabilityTag`] enum. Returned without a
/// leading "· " so each caller frames it to taste.
pub(crate) fn availability_label(tag: AvailabilityTag) -> &'static str {
    match tag {
        AvailabilityTag::Available { streamable: true } => "streams",
        AvailabilityTag::Available { streamable: false } => "interactive",
        AvailabilityTag::Unavailable => "unavailable",
        AvailabilityTag::Disabled => "disabled",
    }
}
