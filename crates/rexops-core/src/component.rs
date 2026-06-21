//! component.rs — the suite Component Registry as pure data.
//!
//! One box in the suite metaphor = one `Component`. This module DESCRIBES each
//! component and HOW the cockpit should learn its health/launch it; it performs
//! no I/O (the app/adapters layer does the work). It is the single source of
//! truth for "what components exist," unifying the health roster and the launch
//! catalog so every cockpit surface reads the same list.

use crate::error::CoreError;

/// A validated, stable kebab id for a component (e.g. "pulse"). Mirrors
/// `AdapterId`: non-empty, lowercase letters/digits/`-` only.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComponentId(String);

impl ComponentId {
    pub fn new(s: &str) -> Result<Self, CoreError> {
        let ok = !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if ok {
            Ok(Self(s.to_owned()))
        } else {
            Err(CoreError::InvalidId(format!(
                "component id must be non-empty kebab (lowercase/digits/-): got {s:?}"
            )))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which box in the metaphor a component belongs to. Drives cockpit grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentGroup {
    Brain,
    Monitor,
    BlackBox,
    FieldTool,
    Mechanic,
    Factory,
    Face,
}

impl ComponentGroup {
    pub fn label(&self) -> &'static str {
        match self {
            ComponentGroup::Brain => "brain",
            ComponentGroup::Monitor => "monitor",
            ComponentGroup::BlackBox => "black box",
            ComponentGroup::FieldTool => "field tool",
            ComponentGroup::Mechanic => "mechanic",
            ComponentGroup::Factory => "factory",
            ComponentGroup::Face => "face",
        }
    }
}

/// How wired-up a component is today. `Planned` rows render as dim, never green.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Maturity {
    Live,
    FeedReady,
    Planned,
}

impl Maturity {
    pub fn label(&self) -> &'static str {
        match self {
            Maturity::Live => "live",
            Maturity::FeedReady => "feed-ready",
            Maturity::Planned => "planned",
        }
    }
}

/// How a tool runs when launched (core-owned copy; the tui catalog keeps its
/// own until Phase C folds them into one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Foreground,
    Background,
}

/// How to launch a component, if it is runnable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchSpec {
    pub run_mode: RunMode,
    pub args: &'static [&'static str],
    pub refresh_after: bool,
}

/// The contract-feed file a component publishes, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedSpec {
    pub contract: &'static str,
}

/// How the cockpit learns a component's health. The unification of the three
/// patterns already in the codebase, plus the honest "not yet" (`Planned`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthSource {
    /// Binary presence + version, like today's Bulwark probe.
    Probe {
        binary: &'static str,
        version_args: &'static [&'static str],
    },
    /// A live `status` subcommand printing health (Pulse-style liveness).
    StatusCommand {
        binary: &'static str,
        args: &'static [&'static str],
    },
    /// A contract-feed file the component publishes (Workstate-style).
    Feed { contract: &'static str },
    /// Derived from the host itself (the existing `system` adapter).
    Host,
    /// Designed but not wired yet. Resolves with zero I/O to a neutral card.
    Planned,
}

/// One box in the suite metaphor, as pure data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Component {
    pub id: &'static str,
    pub name: &'static str,
    pub role: &'static str,
    pub group: ComponentGroup,
    pub health: HealthSource,
    pub launch: Option<LaunchSpec>,
    pub feed: Option<FeedSpec>,
    pub maturity: Maturity,
}

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
// - `Component` describes; it never probes. Keeping I/O out of core preserves
//   the dependency direction (adapters → core) and lets the whole table be
//   unit-tested in isolation.
// - `Planned` is a first-class health source so an unwired tool is *honest*
//   (a dim card), never a fake-green instrument or a special-case `Option`.

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

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
