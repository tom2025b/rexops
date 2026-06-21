//! component_table.rs — the suite Component Registry as a const table.
//!
//! This module owns the COMPONENTS registry (the 11-row table of all suite
//! components in display order) and the component_by_id lookup function. These
//! were split out from component.rs to keep both files under 300 lines while
//! preserving all public API paths (downstream imports remain unchanged).

use super::component::{
    Component, ComponentGroup, FeedSpec, HealthSource, LaunchSpec, Maturity, RunMode,
};

/// The suite map in one table. Order = display order within groups.
///
/// suite-ui is intentionally absent: it is the common face the cockpit is
/// painted with, not an instrument with a status row.
pub const COMPONENTS: &[Component] = &[
    Component {
        id: "workstate",
        name: "Workstate",
        role: "brain",
        blurb: "Suite brain — registry + state hub",
        group: ComponentGroup::Brain,
        health: HealthSource::Feed {
            contract: "workstate",
        },
        launch: None,
        feed: Some(FeedSpec {
            contract: "workstate",
        }),
        maturity: Maturity::Live,
    },
    Component {
        id: "system",
        name: "System",
        role: "host",
        blurb: "Host facts (kernel, uptime, load)",
        group: ComponentGroup::Mechanic,
        health: HealthSource::Host,
        launch: None,
        feed: None,
        maturity: Maturity::Live,
    },
    Component {
        id: "bulwark",
        name: "Bulwark",
        role: "security",
        blurb: "Content/security inspection (live scan)",
        group: ComponentGroup::FieldTool,
        health: HealthSource::Probe {
            binary: "bulwark",
            version_args: &["--help"],
        },
        launch: Some(LaunchSpec {
            run_mode: RunMode::Foreground,
            args: &["tui"],
            refresh_after: false,
        }),
        feed: Some(FeedSpec {
            contract: "bulwark",
        }),
        maturity: Maturity::Live,
    },
    Component {
        id: "proto",
        name: "Proto",
        role: "checklists",
        blurb: "Protocol / checklist runner (interactive picker)",
        group: ComponentGroup::FieldTool,
        health: HealthSource::Probe {
            binary: "proto",
            version_args: &["--help"],
        },
        launch: Some(LaunchSpec {
            run_mode: RunMode::Foreground,
            args: &[],
            refresh_after: false,
        }),
        feed: Some(FeedSpec { contract: "proto" }),
        maturity: Maturity::FeedReady,
    },
    Component {
        id: "scriptvault",
        name: "ScriptVault",
        role: "scripts",
        blurb: "Script library + runner",
        group: ComponentGroup::FieldTool,
        health: HealthSource::Feed {
            contract: "scriptvault",
        },
        launch: Some(LaunchSpec {
            run_mode: RunMode::Foreground,
            args: &[],
            refresh_after: false,
        }),
        feed: Some(FeedSpec {
            contract: "scriptvault",
        }),
        maturity: Maturity::Live,
    },
    Component {
        id: "toolfoundry",
        name: "ToolFoundry",
        role: "tool lifecycle",
        blurb: "Tool build/lifecycle manager",
        group: ComponentGroup::FieldTool,
        health: HealthSource::Feed {
            contract: "toolfoundry",
        },
        launch: Some(LaunchSpec {
            run_mode: RunMode::Foreground,
            args: &[],
            refresh_after: false,
        }),
        feed: Some(FeedSpec {
            contract: "toolfoundry",
        }),
        maturity: Maturity::Live,
    },
    Component {
        id: "pulse",
        name: "Pulse",
        role: "heartbeat",
        blurb: "Heartbeat / liveness monitor",
        group: ComponentGroup::Monitor,
        health: HealthSource::StatusCommand {
            binary: "pulse",
            args: &["status"],
        },
        launch: Some(LaunchSpec {
            run_mode: RunMode::Foreground,
            args: &[],
            refresh_after: false,
        }),
        feed: None,
        maturity: Maturity::Live,
    },
    Component {
        id: "tripwire",
        name: "Tripwire",
        role: "alarm",
        blurb: "Change/intrusion alarm",
        group: ComponentGroup::BlackBox,
        health: HealthSource::Planned,
        launch: None,
        feed: None,
        maturity: Maturity::Planned,
    },
    Component {
        id: "rewind",
        name: "Rewind",
        role: "black box",
        blurb: "Black-box event recorder",
        group: ComponentGroup::BlackBox,
        health: HealthSource::Planned,
        launch: None,
        feed: None,
        maturity: Maturity::Planned,
    },
    Component {
        id: "rex-check",
        name: "rex-check / RexDoctor",
        role: "mechanic",
        blurb: "Suite health checks / doctor",
        group: ComponentGroup::Mechanic,
        // Probe + launch (the bulwark/proto pattern): rex-check has no JSON
        // `status` contract yet, so health is binary-presence (`--help` exits 0),
        // and it launches its doctor run in the foreground. It becomes Live and
        // launchable without a live-status feed; a StatusCommand flip can follow
        // if/when the tool grows the one-line contract.
        health: HealthSource::Probe {
            binary: "rex-check",
            version_args: &["--help"],
        },
        launch: Some(LaunchSpec {
            run_mode: RunMode::Foreground,
            args: &[],
            refresh_after: false,
        }),
        feed: None,
        maturity: Maturity::Live,
    },
    Component {
        id: "rex-forge",
        name: "rex-forge",
        role: "tool factory",
        blurb: "Scaffolder — new tools from templates",
        group: ComponentGroup::Factory,
        health: HealthSource::Planned,
        launch: None,
        feed: None,
        maturity: Maturity::Planned,
    },
];

/// Look up a component by its stable id. `None` for an unknown id.
pub fn component_by_id(id: &str) -> Option<&'static Component> {
    COMPONENTS.iter().find(|c| c.id == id)
}

/// The launchable components — those with a `LaunchSpec` — in registry (display)
/// order. This is the single list the Launcher screen and command palette
/// iterate, replacing the old hand-maintained `CATALOG`. Adding a launchable tool
/// is now exactly: give its registry row a `launch`.
pub fn launchable_components() -> Vec<&'static Component> {
    COMPONENTS.iter().filter(|c| c.launch.is_some()).collect()
}

// Learning Notes
// - The registry is a `const` table for the same reason the launch CATALOG is:
//   the suite is a known, fixed set. Static data needs no allocation, no plugin
//   machinery, and is trivially testable (YAGNI over a dynamic registry).
// - `COMPONENTS` and `component_by_id` were extracted here from component.rs
//   to split the 343-line god-file into two focused modules, both under 300 lines,
//   without breaking any public API (re-exported identically from lib.rs).
// - `Planned` is a first-class health source so an unwired tool is *honest*
//   (a dim card), never a fake-green instrument or a special-case `Option`.

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::ComponentId;

    #[test]
    fn every_component_id_is_unique_and_valid() {
        let mut seen = std::collections::HashSet::new();
        for c in COMPONENTS {
            assert!(
                ComponentId::new(c.id).is_ok(),
                "component id '{}' must be a valid kebab id",
                c.id
            );
            assert!(seen.insert(c.id), "duplicate component id: {}", c.id);
        }
    }

    #[test]
    fn live_components_have_a_non_planned_health_source() {
        // A Live/FeedReady component must declare a real source; only Planned
        // rows may carry HealthSource::Planned. Guards against a fake-green card.
        for c in COMPONENTS {
            let is_planned_source = matches!(c.health, HealthSource::Planned);
            match c.maturity {
                Maturity::Planned => {}
                Maturity::Live | Maturity::FeedReady => assert!(
                    !is_planned_source,
                    "{} is {:?} but has a Planned health source",
                    c.id, c.maturity
                ),
            }
        }
    }

    #[test]
    fn lookup_finds_a_known_component_and_rejects_unknown() {
        assert!(component_by_id("bulwark").is_some());
        assert!(component_by_id("workstate").is_some());
        assert!(component_by_id("no-such-component").is_none());
    }

    #[test]
    fn suite_ui_is_not_a_component_row() {
        // suite-ui is the common face (the medium), never an instrument card.
        assert!(component_by_id("suite-ui").is_none());
    }

    #[test]
    fn every_component_has_a_nonempty_blurb() {
        for c in COMPONENTS {
            assert!(!c.blurb.is_empty(), "{} must have a blurb", c.id);
        }
    }

    #[test]
    fn launchable_view_is_exactly_the_rows_with_a_launch_spec() {
        let ids: Vec<&str> = launchable_components().iter().map(|c| c.id).collect();
        // After Phase E, five rows carry a LaunchSpec; rex-check (Probe+launch)
        // joins as the sixth, in table order (it sits after pulse/tripwire/rewind).
        assert_eq!(
            ids,
            vec![
                "bulwark",
                "proto",
                "scriptvault",
                "toolfoundry",
                "pulse",
                "rex-check"
            ]
        );
        // And the view must agree with the predicate it claims to implement.
        for c in launchable_components() {
            assert!(
                c.launch.is_some(),
                "{} in the view must be launchable",
                c.id
            );
        }
    }

    #[test]
    fn scriptvault_and_toolfoundry_are_launchable_live() {
        for id in ["scriptvault", "toolfoundry"] {
            let c = component_by_id(id).expect("present");
            assert!(c.launch.is_some(), "{id} must be launchable in Phase D");
            assert_eq!(c.maturity, Maturity::Live, "{id} must be Live in Phase D");
        }
    }
}
