//! Registry-backed launch view: the launchable tools and their run-mode facts,
//! read from the `rexops-core` COMPONENTS registry — the single source of truth.
//!
//! This module used to own a hand-maintained `CATALOG`/`ToolEntry`. Phase D made
//! the registry the one place launch data lives, so this is now a thin view over
//! it: `launchable()` is the ordered list the Launcher screen + palette iterate,
//! and `is_streamable`/`refreshes_after` read each component's `LaunchSpec`.

use rexops_core::{component_by_id, launchable_components, Component, RunMode};

/// The launchable components, in registry (display) order. The Launcher screen
/// indexes this by position and the palette iterates it.
pub fn launchable() -> Vec<&'static Component> {
    launchable_components()
}

/// Look up a component by id (the registry lookup, re-exposed under the name the
/// tui launch code reads naturally).
pub fn by_id(id: &str) -> Option<&'static Component> {
    component_by_id(id)
}

/// True when the tool runs as a background job whose output streams into the Jobs
/// screen (vs. taking over the terminal). Reads the registry `LaunchSpec`.
pub fn is_streamable(tool_id: &str) -> bool {
    matches!(
        by_id(tool_id).and_then(|c| c.launch).map(|l| l.run_mode),
        Some(RunMode::Background)
    )
}

/// Whether finishing this tool should kick off a background snapshot refresh.
/// Unknown ids (or non-launchable components) default to `false` — no surprise
/// re-probe. Reads the registry `LaunchSpec.refresh_after`.
pub fn refreshes_after(tool_id: &str) -> bool {
    by_id(tool_id)
        .and_then(|c| c.launch)
        .is_some_and(|l| l.refresh_after)
}

// Learning Notes
// - The launchable set is now a *view* over COMPONENTS (one source of truth), not
//   a second hand-maintained list. Adding a launchable tool is exactly: give its
//   registry row a `launch`. The old CATALOG/ToolEntry could drift from the
//   registry's `launchable` flag; this view cannot.
// - is_streamable/refreshes_after read the component's LaunchSpec, so a tool's run
//   mode + refresh behaviour live in one place (the registry row) instead of being
//   duplicated between the registry and a catalog entry.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launchable_lists_the_registry_launch_rows_in_order() {
        let ids: Vec<&str> = launchable().iter().map(|c| c.id).collect();
        // tripwire and rex-check (both Probe+launch) join in registry display order:
        // after bulwark/proto/scriptvault/toolfoundry/pulse, tripwire (table row 8)
        // then rex-check.
        assert_eq!(
            ids,
            vec![
                "bulwark",
                "proto",
                "scriptvault",
                "toolfoundry",
                "pulse",
                "tripwire",
                "rex-check"
            ]
        );
    }

    #[test]
    fn run_mode_helpers_read_the_registry() {
        // All current launchables are Foreground → not streamable.
        for id in [
            "bulwark",
            "proto",
            "scriptvault",
            "toolfoundry",
            "pulse",
            "tripwire",
            "rex-check",
        ] {
            assert!(!is_streamable(id), "{id} is Foreground, not streamable");
        }
        // Unknown ids are inert, never panic.
        assert!(!is_streamable("nope"));
        assert!(!refreshes_after("nope"));
        assert!(
            !refreshes_after("bulwark"),
            "bulwark does not refresh-after"
        );
    }

    #[test]
    fn by_id_finds_a_registry_component() {
        assert_eq!(by_id("scriptvault").map(|c| c.name), Some("ScriptVault"));
        assert!(by_id("not-a-tool").is_none());
    }
}
