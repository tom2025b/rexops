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
        maturity: Maturity::Live,
    },
    Component {
        id: "scriptvault",
        name: "ScriptVault",
        role: "scripts",
        group: ComponentGroup::FieldTool,
        health: HealthSource::Feed {
            contract: "scriptvault",
        },
        launch: None,
        feed: Some(FeedSpec {
            contract: "scriptvault",
        }),
        maturity: Maturity::FeedReady,
    },
    Component {
        id: "toolfoundry",
        name: "ToolFoundry",
        role: "tool lifecycle",
        group: ComponentGroup::FieldTool,
        health: HealthSource::Feed {
            contract: "toolfoundry",
        },
        launch: None,
        feed: Some(FeedSpec {
            contract: "toolfoundry",
        }),
        maturity: Maturity::FeedReady,
    },
    Component {
        id: "pulse",
        name: "Pulse",
        role: "heartbeat",
        group: ComponentGroup::Monitor,
        health: HealthSource::Planned,
        launch: None,
        feed: None,
        maturity: Maturity::Planned,
    },
    Component {
        id: "tripwire",
        name: "Tripwire",
        role: "alarm",
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
        group: ComponentGroup::Mechanic,
        health: HealthSource::Planned,
        launch: None,
        feed: None,
        maturity: Maturity::Planned,
    },
    Component {
        id: "rex-forge",
        name: "rex-forge",
        role: "tool factory",
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
}
