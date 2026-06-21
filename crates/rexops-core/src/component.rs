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

// Learning Notes
// - `Component` describes; it never probes. Keeping I/O out of core preserves
//   the dependency direction (adapters → core) and lets the whole type system be
//   unit-tested in isolation.
// - `Planned` is a first-class health source so an unwired tool is *honest*
//   (a dim card), never a fake-green instrument or a special-case `Option`.
// - The component registry table (COMPONENTS) and lookup function (component_by_id)
//   live in component_table.rs to keep both files under 300 lines.
