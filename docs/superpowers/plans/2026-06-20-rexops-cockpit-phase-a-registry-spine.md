# RexOps Cockpit — Phase A: Registry Spine — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce a declarative Component Registry in `rexops-core` and rewrite `rexops-app`'s hard-coded probe blocks as a single registry walk — unifying the two roster sources (`REAL_ADAPTERS` + the launch `CATALOG`) into one model, with **zero change to observable behavior**.

**Architecture:** A pure-data `Component` table (`COMPONENTS`) lives in `rexops-core` alongside the existing `AdapterHealth`/`Freshness` types. `rexops-app` gains a `registry_walk` that resolves each component's `HealthSource` exactly as today's three `if real_adapter_enabled(...)` blocks do, producing the same `OpsSnapshot`. A new `rexops components` CLI subcommand and a roster-agreement test lock the unification in. This is the foundation Phases B–F (cockpit UI, FeedReady tools, monitors, CLI parity) build on; it ships no UI change.

**Tech Stack:** Rust 2021, workspace crates (`rexops-core`, `rexops-app`, `rexops-cli`), `serde`/`serde_json`, `clap` v4, `cargo test`/`clippy`/`fmt`.

## Global Constraints

- Files stay **under 300 LOC** (ideally < 200); each `.rs` ends with a `// Learning Notes` footer (existing project convention).
- `rexops-core` **denies** `clippy::unwrap_used` / `clippy::expect_used` in non-test code (crate-root attribute); all four cargo gates (`build`, `test`, `clippy -- -D warnings`, `fmt --check`) must be green at the end of **every** task's commit.
- `rexops-core` is **pure data**: zero I/O, and it must **not** depend on `rexops-adapters`. The Component model describes *how* to probe; it never probes.
- **Behavior parity:** after this phase, `rexops status`, `rexops adapters`, and `rexops --json` produce the same output they do today for the same inputs. The roster stays exactly `bulwark`, `system`, `workstate`.
- **stdin is a process singleton** — read once; the registry walk must thread the already-captured `piped: Option<&str>` through, never read stdin itself.
- **Graceful degradation** — a `Planned` component does zero I/O; missing/unknown inputs degrade to a note + neutral health, never a panic.
- Conventional commits: `feat(rexops): … (Phase A)` / `test(rexops): …` / `refactor(rexops): …`.
- Run all `cargo` commands from the worktree root: `/home/tom/projects/rexops/.claude/worktrees/rexops-cockpit-redesign-doc`.

---

## File Structure

- **Create** `crates/rexops-core/src/component.rs` — the pure `Component` model: `ComponentId`, `ComponentGroup`, `Maturity`, `HealthSource`, `LaunchSpec`, `FeedSpec`, the `COMPONENTS` table, and lookup helpers. One responsibility: *describe the suite's components as data.*
- **Modify** `crates/rexops-core/src/lib.rs` — declare `mod component;` and re-export its public types.
- **Modify** `crates/rexops-core/src/models.rs` — add `ComponentStatus` (the resolved, renderable per-component status) and an `OpsSnapshot.components: Vec<ComponentStatus>` field + a setter.
- **Modify** `crates/rexops-app/src/snapshot.rs` — derive `REAL_ADAPTERS` from the registry; add `registry_walk` that populates `snapshot.components` from `COMPONENTS`; keep the existing probe blocks as the resolution bodies (no behavior change). Add the roster-agreement test across `status`/`adapters`/`components`.
- **Modify** `crates/rexops-cli/src/main.rs` — add the `Components` subcommand (human + `--json`).

> The launch `CATALOG` in `rexops-tui/src/tools/catalog.rs` is **left untouched in Phase A** — folding it into the registry happens in Phase C when the cockpit consumes `LaunchSpec`. Phase A only unifies the *health/roster* half, which is self-contained and behavior-preserving.

---

### Task 1: The `Component` data model

**Files:**
- Create: `crates/rexops-core/src/component.rs`
- Modify: `crates/rexops-core/src/lib.rs` (declare + re-export)
- Test: inline `#[cfg(test)]` in `crates/rexops-core/src/component.rs`

**Interfaces:**
- Consumes: nothing (pure data, no other task).
- Produces:
  - `pub struct ComponentId(String)` with `pub fn new(s: &str) -> Result<ComponentId, CoreError>` (non-empty, kebab; mirrors `AdapterId`) and `pub fn as_str(&self) -> &str`.
  - `pub enum ComponentGroup { Brain, Monitor, BlackBox, FieldTool, Mechanic, Factory, Face }` with `pub fn label(&self) -> &'static str`.
  - `pub enum Maturity { Live, FeedReady, Planned }` with `pub fn label(&self) -> &'static str`.
  - `pub enum HealthSource { Probe { binary: &'static str, version_args: &'static [&'static str] }, StatusCommand { binary: &'static str, args: &'static [&'static str] }, Feed { contract: &'static str }, Host, Planned }`.
  - `pub struct LaunchSpec { pub run_mode: RunMode, pub args: &'static [&'static str], pub refresh_after: bool }` and `pub enum RunMode { Foreground, Background }` (a core-owned copy; the tui catalog keeps its own until Phase C folds them).
  - `pub struct FeedSpec { pub contract: &'static str }`.
  - `pub struct Component { pub id: &'static str, pub name: &'static str, pub role: &'static str, pub group: ComponentGroup, pub health: HealthSource, pub launch: Option<LaunchSpec>, pub feed: Option<FeedSpec>, pub maturity: Maturity }`.
  - `pub const COMPONENTS: &[Component]` — the nine rows from the design (Workstate, Bulwark, system, Proto, ScriptVault, ToolFoundry, Pulse, Rewind, Tripwire, rex-check, rex-forge). suite-ui is **not** a row.
  - `pub fn component_by_id(id: &str) -> Option<&'static Component>`.

- [ ] **Step 1: Write the failing test**

Add to a new file `crates/rexops-core/src/component.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-core component:: 2>&1 | tail -20`
Expected: FAIL — compile error, `COMPONENTS`/`ComponentId`/`component_by_id` not found (the module body doesn't exist yet).

- [ ] **Step 3: Write the minimal implementation**

Put this **above** the test module in `crates/rexops-core/src/component.rs`:

```rust
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
        feed: Some(FeedSpec { contract: "bulwark" }),
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
```

`CoreError::InvalidId(String)` is the variant `AdapterId::new` returns (it takes a human message string, e.g. `CoreError::InvalidId("tool id must be non-empty".to_owned())`), so the code above matches the existing contract. Confirm if unsure:

Run: `grep -n "InvalidId" crates/rexops-core/src/error.rs crates/rexops-core/src/ids.rs`
Expected: the `InvalidId(String)` variant in `error.rs` and `AdapterId::new` constructing it with a message string in `ids.rs`.

- [ ] **Step 4: Wire the module into the crate**

In `crates/rexops-core/src/lib.rs`, add `component` to the `mod` block (keep alphabetical with the neighbors):

```rust
mod adapter_models;
mod adapter_types;
mod component;
mod config;
```

And add the re-export line, grouped with the other `pub use` lines:

```rust
pub use component::{
    component_by_id, Component, ComponentGroup, ComponentId, FeedSpec, HealthSource, LaunchSpec,
    Maturity, RunMode, COMPONENTS,
};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rexops-core component:: 2>&1 | tail -20`
Expected: PASS — all four tests in `component::tests` green.

- [ ] **Step 6: Gates green, then commit**

Run: `cargo fmt && cargo clippy -p rexops-core -- -D warnings && cargo test -p rexops-core 2>&1 | tail -5`
Expected: fmt clean, clippy no warnings, all rexops-core tests pass.

```bash
git add crates/rexops-core/src/component.rs crates/rexops-core/src/lib.rs
git commit -m "feat(rexops): add Component registry data model (Phase A)"
```

---

### Task 2: `ComponentStatus` + `OpsSnapshot.components`

**Files:**
- Modify: `crates/rexops-core/src/models.rs` (add `ComponentStatus`, the field, the setter)
- Test: inline `#[cfg(test)]` in `crates/rexops-core/src/models.rs`

**Interfaces:**
- Consumes: `AdapterHealth` (already in core), `Freshness` (already in core).
- Produces:
  - `pub struct ComponentStatus { pub id: String, pub name: String, pub group: String, pub maturity: String, pub health: AdapterHealth, pub freshness: Option<Freshness>, pub vital: Option<String>, pub launchable: bool }` — the **resolved**, renderable status the cockpit/CLI display (plain owned strings so it serializes cleanly and carries no `'static` borrow).
  - `OpsSnapshot.components: Vec<ComponentStatus>` (new public field, `#[serde(default)]`).
  - `pub fn push_component(&mut self, status: ComponentStatus)` on `OpsSnapshot`.

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` in `crates/rexops-core/src/models.rs` (or create one if absent, with the same `#[allow(...)]` header the other test modules use):

```rust
#[test]
fn ops_snapshot_carries_resolved_component_statuses() {
    use crate::{AdapterHealth, ComponentStatus};
    let mut snap = OpsSnapshot::new();
    assert!(snap.components.is_empty(), "new snapshot has no components");

    snap.push_component(ComponentStatus {
        id: "bulwark".to_owned(),
        name: "Bulwark".to_owned(),
        group: "field tool".to_owned(),
        maturity: "live".to_owned(),
        health: AdapterHealth::Healthy,
        freshness: None,
        vital: Some("1 crit 1 high".to_owned()),
        launchable: true,
    });

    assert_eq!(snap.components.len(), 1);
    assert_eq!(snap.components[0].id, "bulwark");
    assert_eq!(snap.components[0].health, AdapterHealth::Healthy);

    // Round-trips through serde so the CLI `--json` view can emit it.
    let json = serde_json::to_string(&snap).expect("serialize");
    let back: OpsSnapshot = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.components.len(), 1);
    assert_eq!(back.components[0].vital.as_deref(), Some("1 crit 1 high"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-core ops_snapshot_carries_resolved 2>&1 | tail -20`
Expected: FAIL — `ComponentStatus` not found / `OpsSnapshot` has no field `components` / no `push_component`.

- [ ] **Step 3: Write the minimal implementation**

In `crates/rexops-core/src/models.rs`, add the struct near the other snapshot types (above `OpsSnapshot`), deriving the same traits the sibling structs use (`Debug, Clone, Serialize, Deserialize`, plus `PartialEq` if the others have it):

```rust
/// A single component's RESOLVED status, ready to render. Produced by the app
/// layer's registry walk from a `Component` + a live probe; carries owned
/// strings (no `'static` borrow) so it serializes cleanly into the snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComponentStatus {
    pub id: String,
    pub name: String,
    pub group: String,
    pub maturity: String,
    pub health: AdapterHealth,
    /// Data freshness when the source is a feed; `None` for probe/host/planned.
    pub freshness: Option<Freshness>,
    /// The one headline number for the card (e.g. "3/3 fresh", "1 crit 1 high").
    pub vital: Option<String>,
    /// Whether this component currently resolves to a launch command.
    pub launchable: bool,
}
```

Add the field to `OpsSnapshot` (with `#[serde(default)]` so older JSON still parses):

```rust
    /// Resolved per-component statuses for the cockpit. Populated by the app
    /// layer's registry walk; empty until the first build.
    #[serde(default)]
    pub components: Vec<ComponentStatus>,
```

Initialize it in `OpsSnapshot::new()` (add `components: Vec::new(),` to the struct literal). Add the setter in `impl OpsSnapshot`:

```rust
    /// Append a resolved component status (the registry walk calls this once per
    /// component, in table order).
    pub fn push_component(&mut self, status: ComponentStatus) {
        self.components.push(status);
    }
```

Confirm `AdapterHealth` and `Freshness` are in scope in `models.rs`; if not, add to its `use` block:

```rust
use crate::{AdapterHealth, Freshness};
```

(If `models.rs` already imports `AdapterHealth` via a different path, reuse that; just ensure both names resolve.)

- [ ] **Step 4: Export `ComponentStatus`**

In `crates/rexops-core/src/lib.rs`, add `ComponentStatus` to the `models` re-export line:

```rust
pub use models::{
    format_unix_millis_utc, ComponentStatus, JobStatus, OpsSnapshot, ReportSummary, RiskSummary,
};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rexops-core ops_snapshot_carries_resolved 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 6: Gates green, then commit**

Run: `cargo fmt && cargo clippy -p rexops-core -- -D warnings && cargo test -p rexops-core 2>&1 | tail -5`
Expected: all green (the new `#[serde(default)]` field keeps every existing snapshot test passing).

```bash
git add crates/rexops-core/src/models.rs crates/rexops-core/src/lib.rs
git commit -m "feat(rexops): add ComponentStatus + OpsSnapshot.components (Phase A)"
```

---

### Task 3: Derive `REAL_ADAPTERS` from the registry

**Files:**
- Modify: `crates/rexops-app/src/snapshot.rs` (replace the hand-written `REAL_ADAPTERS` const with one derived from `COMPONENTS`)
- Test: inline `#[cfg(test)]` in `crates/rexops-app/src/snapshot.rs`

**Interfaces:**
- Consumes: `rexops_core::{COMPONENTS, Component, HealthSource}` (Task 1).
- Produces: `fn real_adapter_ids() -> Vec<&'static str>` — the ids of components whose `HealthSource` is a *probed/host/feed* source (i.e. not `Planned`), filtered to the three the app currently resolves. Keeps `real_adapter_enabled` working unchanged.

> **Why filtered, not "all non-Planned":** Task 1's table marks ScriptVault/ToolFoundry as `Feed` + `FeedReady`. The app does **not** resolve their feeds until Phase D. To preserve Phase A's behavior-parity constraint (roster stays exactly `bulwark`/`system`/`workstate`), this task derives the roster from the registry **and** intersects it with the set the app actually resolves today. The intersection set shrinks to nothing in Phase D when the feeds are wired.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` in `crates/rexops-app/src/snapshot.rs`:

```rust
#[test]
fn real_adapter_roster_is_derived_from_the_registry() {
    // The roster the app probes must be exactly today's three, and every one of
    // them must be a real (non-Planned) component in the core registry — proving
    // the roster is registry-derived, not a hand-maintained duplicate that can
    // drift.
    let mut roster = real_adapter_ids();
    roster.sort_unstable();
    assert_eq!(roster, vec!["bulwark", "system", "workstate"]);

    for id in &roster {
        let c = rexops_core::component_by_id(id)
            .unwrap_or_else(|| panic!("roster id '{id}' missing from COMPONENTS"));
        assert!(
            !matches!(c.health, rexops_core::HealthSource::Planned),
            "roster id '{id}' must have a real health source, not Planned"
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-app real_adapter_roster_is_derived 2>&1 | tail -20`
Expected: FAIL — `real_adapter_ids` not found.

- [ ] **Step 3: Write the minimal implementation**

In `crates/rexops-app/src/snapshot.rs`, **replace** the existing const:

```rust
const REAL_ADAPTERS: &[&str] = &["bulwark", "system", "workstate"];
```

with a registry-derived function. Add near the top of the file (after the `use` lines):

```rust
/// The ids the app currently resolves to live health. Derived from the core
/// registry (every id must be a real, non-`Planned` component) intersected with
/// the set this crate actually probes today. The intersection is what preserves
/// behavior parity while the table already lists not-yet-wired feeds
/// (ScriptVault/ToolFoundry) that Phase D will light up.
fn real_adapter_ids() -> Vec<&'static str> {
    // The sources the app resolves in `build_snapshot_with_piped` today.
    const RESOLVED_TODAY: &[&str] = &["bulwark", "system", "workstate"];
    rexops_core::COMPONENTS
        .iter()
        .filter(|c| !matches!(c.health, rexops_core::HealthSource::Planned))
        .map(|c| c.id)
        .filter(|id| RESOLVED_TODAY.contains(id))
        .collect()
}
```

Then update `real_adapter_enabled` to use it (replace the two `REAL_ADAPTERS` references in that function):

```rust
fn real_adapter_enabled(config: &AppConfig, id: &str) -> bool {
    let roster = real_adapter_ids();
    debug_assert!(
        roster.contains(&id),
        "{id} is not a real adapter; only {roster:?} may be probed"
    );
    roster.contains(&id) && config.adapter_enabled(id)
}
```

Finally, update the three **existing** roster tests that reference `REAL_ADAPTERS` (`adapter_health_roster_only_ever_holds_real_adapters` and `status_and_adapters_views_agree_on_the_roster`) to call `real_adapter_ids()` instead. In each, replace `REAL_ADAPTERS.contains(&id.as_str())` with `real_adapter_ids().contains(&id.as_str())`, and replace the `expected` builder:

```rust
let mut expected: Vec<String> = REAL_ADAPTERS.iter().map(|s| (*s).to_owned()).collect();
```

with:

```rust
let mut expected: Vec<String> = real_adapter_ids().iter().map(|s| (*s).to_owned()).collect();
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rexops-app 2>&1 | tail -20`
Expected: PASS — the new test plus the two updated roster tests are green; no behavior changed (the roster is still exactly the three).

- [ ] **Step 5: Gates green, then commit**

Run: `cargo fmt && cargo clippy -p rexops-app -- -D warnings && cargo test -p rexops-app 2>&1 | tail -5`
Expected: all green.

```bash
git add crates/rexops-app/src/snapshot.rs
git commit -m "refactor(rexops): derive adapter roster from the Component registry (Phase A)"
```

---

### Task 4: `registry_walk` populates `snapshot.components`

**Files:**
- Modify: `crates/rexops-app/src/snapshot.rs` (add `registry_walk`; call it from `build_snapshot_with_piped`)
- Test: inline `#[cfg(test)]` in `crates/rexops-app/src/snapshot.rs`

**Interfaces:**
- Consumes: `rexops_core::{COMPONENTS, Component, HealthSource, Maturity, ComponentGroup, ComponentStatus, AdapterHealth, OpsSnapshot}`; the already-resolved `snapshot.adapter_health` map; the already-folded `snapshot.{scripts,tools,findings,workstate}` (Task runs **after** the existing probe blocks).
- Produces: `fn registry_walk(snap: &mut OpsSnapshot, config: &AppConfig)` — appends one `ComponentStatus` per `COMPONENTS` row, reading health from `snap.adapter_health` (for resolved adapters) or `Unknown`/`Planned` otherwise, and a `vital` derived from the data already in `snap`. No new I/O (it reads what the probe blocks already wrote).

> **Key design point — no double-probing:** the walk does **not** re-probe. The existing blocks in `build_snapshot_with_piped` already populate `adapter_health` and the structured fields; `registry_walk` runs last and simply *projects* that resolved state into the `components` vec. This keeps "one probe per refresh" intact and makes the walk a pure projection (easy to test).

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` in `crates/rexops-app/src/snapshot.rs`:

```rust
#[test]
fn registry_walk_projects_one_status_per_component() {
    // The walk must emit exactly one ComponentStatus per registry row, in table
    // order, projecting the already-resolved health — never re-probing.
    let snap = build_snapshot_with_piped(&workstate_only_config(), Some(WORKSTATE_FEED));
    assert_eq!(
        snap.components.len(),
        rexops_core::COMPONENTS.len(),
        "one status per registry component"
    );
    // Order matches the table.
    for (status, comp) in snap.components.iter().zip(rexops_core::COMPONENTS) {
        assert_eq!(status.id, comp.id, "component statuses follow table order");
    }
}

#[test]
fn planned_components_are_neutral_not_faulty() {
    // A Planned component (e.g. pulse) must surface as Unknown health and a
    // "planned" maturity — never Healthy (fake green) and never Unavailable
    // (a fault). It is honest, dim, and does no I/O.
    let snap = build_snapshot_with_piped(&workstate_only_config(), Some(WORKSTATE_FEED));
    let pulse = snap
        .components
        .iter()
        .find(|c| c.id == "pulse")
        .expect("pulse is a registry row");
    assert_eq!(pulse.maturity, "planned");
    assert_eq!(pulse.health, rexops_core::AdapterHealth::Unknown);
    assert!(!pulse.launchable, "a planned component is not launchable");
}

#[test]
fn live_workstate_component_reflects_resolved_health() {
    // The workstate component's projected health must equal what the probe block
    // already wrote into adapter_health — proving projection, not re-probe.
    let snap = build_snapshot_with_piped(&workstate_only_config(), Some(WORKSTATE_FEED));
    let ws_health = snap
        .adapter_health
        .get("workstate")
        .copied()
        .expect("workstate probed");
    let ws_component = snap
        .components
        .iter()
        .find(|c| c.id == "workstate")
        .expect("workstate is a registry row");
    assert_eq!(ws_component.health, ws_health);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-app registry_walk_projects 2>&1 | tail -20`
Expected: FAIL — `snap.components` is empty (the walk isn't called yet), so the length assertion fails.

- [ ] **Step 3: Write the minimal implementation**

In `crates/rexops-app/src/snapshot.rs`, add the walk function (place it after `build_snapshot_with_piped`):

```rust
/// Project the already-resolved snapshot state into one `ComponentStatus` per
/// registry row. Runs LAST in the build, after the probe blocks have populated
/// `adapter_health` and the structured fields — it re-probes nothing, it only
/// reads what is already there. This is what makes the cockpit, `status`, and
/// `components` all read the same single resolution.
fn registry_walk(snap: &mut OpsSnapshot, config: &AppConfig) {
    use rexops_core::{AdapterHealth, ComponentStatus, HealthSource};

    for comp in rexops_core::COMPONENTS {
        // Health: a Planned source never touches I/O and reads Unknown; every
        // other source's health was already resolved into adapter_health by the
        // probe blocks (or stays Unknown if that source isn't wired this phase).
        let health = match comp.health {
            HealthSource::Planned => AdapterHealth::Unknown,
            _ => snap
                .adapter_health
                .get(comp.id)
                .copied()
                .unwrap_or(AdapterHealth::Unknown),
        };

        let launchable = comp.launch.is_some()
            && config.adapter_enabled(comp.id)
            && health != AdapterHealth::Unavailable;

        snap.push_component(ComponentStatus {
            id: comp.id.to_owned(),
            name: comp.name.to_owned(),
            group: comp.group.label().to_owned(),
            maturity: comp.maturity.label().to_owned(),
            health,
            freshness: component_freshness(snap, comp.id),
            vital: component_vital(snap, comp.id),
            launchable,
        });
    }
}

/// Freshness for feed-backed components, read from the structured data the
/// Workstate fold already produced. `None` for non-feed sources.
fn component_freshness(snap: &OpsSnapshot, id: &str) -> Option<rexops_core::Freshness> {
    use rexops_core::status_to_freshness;
    let ws = snap.workstate.as_ref()?;
    let section = match id {
        "scriptvault" => &ws.scripts,
        "toolfoundry" => &ws.tools,
        "workstate" => return Some(status_to_freshness(&ws.findings.status)),
        _ => return None,
    };
    Some(status_to_freshness(&section.status))
}

/// The one headline number per component, derived from already-folded data.
/// `None` when there is nothing meaningful to show (e.g. a Planned component).
fn component_vital(snap: &OpsSnapshot, id: &str) -> Option<String> {
    match id {
        "workstate" => snap
            .workstate
            .as_ref()
            .map(|ws| format!("{}/3 fresh", ws.populated_section_count())),
        "bulwark" => snap.findings.as_ref().map(|f| {
            let t = f.risk_tally();
            format!("{} crit {} high", t.critical, t.high)
        }),
        "scriptvault" => snap
            .scripts
            .as_ref()
            .map(|s| format!("{} scripts", s.total())),
        "toolfoundry" => snap
            .tools
            .as_ref()
            .map(|t| format!("{} need review", t.attention_count)),
        "system" => snap
            .system
            .as_ref()
            .and_then(|s| s.hostname.clone()),
        _ => None,
    }
}
```

Then call the walk at the **end** of `build_snapshot_with_piped`, immediately before `snap` is returned (after the `config: loaded` note):

```rust
    // Project the resolved state into per-component statuses (must be last: it
    // reads adapter_health + the folded fields the blocks above populated).
    registry_walk(&mut snap, config);

    snap
```

Confirm the field/method names used: `ws.scripts`, `ws.tools`, `ws.findings` are `Section`s with a `.status: String` (used by `note_section_freshness` already in this file); `ws.populated_section_count()`, `f.risk_tally()`, `s.total()`, `t.attention_count`, `s.hostname` are all already used elsewhere in `snapshot.rs`/`main.rs` (see Task references). If any differ, grep the existing usage in `snapshot.rs` and match it exactly.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rexops-app 2>&1 | tail -20`
Expected: PASS — the three new walk tests green, and every existing snapshot test still green (the walk only *adds* to the snapshot).

- [ ] **Step 5: Gates green, then commit**

Run: `cargo fmt && cargo clippy -p rexops-app -- -D warnings && cargo test -p rexops-app 2>&1 | tail -5`
Expected: all green.

```bash
git add crates/rexops-app/src/snapshot.rs
git commit -m "feat(rexops): registry_walk projects resolved state into components (Phase A)"
```

---

### Task 5: Roster-agreement guard across status/adapters/components

**Files:**
- Modify: `crates/rexops-app/src/snapshot.rs` (add the cross-view guard test)
- Test: inline `#[cfg(test)]` in `crates/rexops-app/src/snapshot.rs`

**Interfaces:**
- Consumes: `build_snapshot_with_piped`, `build_adapter_registry`, `real_adapter_ids`, `OpsSnapshot.components`.
- Produces: nothing (test-only) — locks the unification invariant.

- [ ] **Step 1: Write the failing test (expected to PASS once written — it guards Tasks 3–4)**

Add to the `#[cfg(test)] mod tests` in `crates/rexops-app/src/snapshot.rs`:

```rust
#[test]
fn status_adapters_and_components_never_disagree_on_the_live_roster() {
    // THE PHASE-A INVARIANT: the three views must agree. The set of components
    // reporting a real (non-Unknown, non-Planned-maturity) health must be
    // exactly the adapter roster — so the cockpit's "live" cards, `status`'s
    // adapter_health, and `adapters`' registry can never drift apart.
    let cfg = AppConfig::default();
    let snap = build_snapshot_with_piped(&cfg, Some(WORKSTATE_FEED));
    let reg = build_adapter_registry(&cfg);

    let mut from_adapter_health: Vec<String> = snap
        .adapter_health
        .keys()
        .map(|id| id.as_str().to_owned())
        .collect();
    from_adapter_health.sort();

    let mut from_registry: Vec<String> =
        reg.list().iter().map(|e| e.id.as_str().to_owned()).collect();
    from_registry.sort();

    // Components whose maturity is "live" must be exactly the adapter roster.
    let mut live_components: Vec<String> = snap
        .components
        .iter()
        .filter(|c| c.maturity == "live")
        .map(|c| c.id.clone())
        .collect();
    live_components.sort();

    let mut expected = real_adapter_ids()
        .iter()
        .map(|s| (*s).to_owned())
        .collect::<Vec<_>>();
    expected.sort();

    assert_eq!(from_adapter_health, expected, "status roster");
    assert_eq!(from_registry, expected, "adapters roster");
    assert_eq!(live_components, expected, "live component cards");
}
```

> Note: this assumes the four `Live` rows (`workstate`, `system`, `bulwark`, `proto`) minus the ones not in `real_adapter_ids()` equals the roster. `proto` is `Live` in the table but is **not** in `real_adapter_ids()` (it has no health adapter today — only a launch). So this test would FAIL for `proto`. Resolve it in Step 2.

- [ ] **Step 2: Reconcile `proto`'s maturity with the live roster**

`proto` has a launch but no resolved health adapter in Phase A, so it must not count as a "live" *health* card yet. In `crates/rexops-core/src/component.rs`, change `proto`'s maturity from `Maturity::Live` to `Maturity::FeedReady` (it has a `proto` feed contract on disk; its health lights up when that feed is consumed in Phase D, alongside ScriptVault/ToolFoundry). Update the Task 1 doc-comment expectation if you keep notes.

This keeps the invariant honest: only components with a **resolved health adapter today** carry `Live`. After the edit, the `Live` rows are exactly `workstate`, `system`, `bulwark` — matching `real_adapter_ids()`.

Re-confirm Task 1's test `live_components_have_a_non_planned_health_source` still passes (FeedReady + `Feed` source is allowed).

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p rexops-core component:: && cargo test -p rexops-app 2>&1 | tail -20`
Expected: PASS — core registry tests still green after the `proto` maturity edit; the new agreement test green.

- [ ] **Step 4: Gates green, then commit**

Run: `cargo fmt && cargo clippy --workspace -- -D warnings && cargo test --workspace 2>&1 | tail -8`
Expected: all green across the workspace (this is the first full-workspace gate of the phase).

```bash
git add crates/rexops-core/src/component.rs crates/rexops-app/src/snapshot.rs
git commit -m "test(rexops): guard status/adapters/components roster agreement (Phase A)"
```

---

### Task 6: `rexops components` CLI subcommand

**Files:**
- Modify: `crates/rexops-cli/src/main.rs` (add the `Components` subcommand + human/JSON printers)
- Test: inline `#[cfg(test)]` in `crates/rexops-cli/src/main.rs` (a small formatter test) — or, if `main.rs` has no test module, a focused render-string test on the printer fn.

**Interfaces:**
- Consumes: `rexops_app::build_snapshot` / `load_config`; `rexops_core::OpsSnapshot.components` (Task 2/4).
- Produces: the `Components` clap variant; `fn print_components_human(snap: &OpsSnapshot)`.

- [ ] **Step 1: Write the failing test**

Add a test module (or extend an existing one) in `crates/rexops-cli/src/main.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rexops_core::{AdapterHealth, ComponentStatus, OpsSnapshot};

    fn snap_with_one() -> OpsSnapshot {
        let mut s = OpsSnapshot::new();
        s.push_component(ComponentStatus {
            id: "pulse".to_owned(),
            name: "Pulse".to_owned(),
            group: "monitor".to_owned(),
            maturity: "planned".to_owned(),
            health: AdapterHealth::Unknown,
            freshness: None,
            vital: None,
            launchable: false,
        });
        s
    }

    #[test]
    fn components_human_lists_the_row_with_its_maturity() {
        let out = render_components_human(&snap_with_one());
        assert!(out.contains("Pulse"), "names the component:\n{out}");
        assert!(out.contains("planned"), "shows maturity:\n{out}");
        assert!(out.contains("monitor"), "shows the group:\n{out}");
    }
}
```

To make the printer testable as a string (the existing printers `println!` directly), implement the body in a `render_components_human(snap) -> String` and have `print_components_human` just `print!` it. Write the failing test against `render_components_human`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-cli components_human_lists 2>&1 | tail -20`
Expected: FAIL — `render_components_human` not found.

- [ ] **Step 3: Write the minimal implementation**

Add the variant to the `Commands` enum in `crates/rexops-cli/src/main.rs`:

```rust
    /// List the suite component registry (id, group, maturity, health, vital).
    Components,
```

Add the dispatch arm in `run`, alongside `Status`/`Adapters`:

```rust
        Commands::Components => {
            let snapshot = build_snapshot(&config);
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&snapshot.components)?);
            } else {
                print!("{}", render_components_human(&snapshot));
            }
        }
```

Add the printers:

```rust
/// Print the resolved component roster (the `components` subcommand, human form).
fn print_components_human(snap: &OpsSnapshot) {
    print!("{}", render_components_human(snap));
}

/// Render the human component roster to a String (separated from printing so it
/// can be unit-tested without capturing stdout).
fn render_components_human(snap: &OpsSnapshot) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "Components ({} in the suite map):", snap.components.len());
    if snap.components.is_empty() {
        let _ = writeln!(out, "  (none — run a refresh)");
        return out;
    }
    for c in &snap.components {
        let mark = match c.health {
            AdapterHealth::Healthy => "✓",
            AdapterHealth::Degraded => "!",
            AdapterHealth::Unavailable => "✗",
            AdapterHealth::Unknown => "·",
        };
        let vital = c.vital.as_deref().unwrap_or("-");
        let _ = writeln!(
            out,
            "  {mark} {:<22} {:<11} {:<10} {}",
            c.name, c.group, c.maturity, vital
        );
    }
    out
}
```

Mark `print_components_human` `#[allow(dead_code)]` if nothing calls it yet, or simply call `render_components_human` directly in the dispatch arm (preferred — drop `print_components_human` to avoid a dead fn). Ensure `AdapterHealth` is imported in `main.rs` (it already is — used by `print_status_human`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rexops-cli 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Manual smoke check**

Run: `cargo run -p rexops-cli -- components 2>&1 | head -20`
Expected: a table listing all 11 component rows (Workstate, System, Bulwark, Proto, ScriptVault, ToolFoundry, Pulse, Tripwire, Rewind, rex-check, rex-forge), each with a health mark, group, maturity, and vital. Planned rows show `·` and `planned`.

Run: `cargo run -p rexops-cli -- components --json 2>&1 | head -5`
Expected: a JSON array of `ComponentStatus` objects.

Also confirm parity is intact:
Run: `cargo run -p rexops-cli -- adapters 2>&1 | tail -6`
Expected: still exactly the three adapters (`bulwark`, `system`, `workstate`) — unchanged from before the phase.

- [ ] **Step 6: Gates green, then commit**

Run: `cargo fmt && cargo clippy --workspace -- -D warnings && cargo test --workspace 2>&1 | tail -8`
Expected: all green across the workspace.

```bash
git add crates/rexops-cli/src/main.rs
git commit -m "feat(rexops): add 'rexops components' subcommand (Phase A)"
```

---

## Self-Review

**1. Spec coverage (against §2, §4, §6, §9-Phase-A of the design):**
- Component model (`Component`/`HealthSource`/`Maturity`/`COMPONENTS`) → Task 1. ✓
- `OpsSnapshot.components` (`ComponentStatus`) → Task 2. ✓
- Registry walk replaces hard-coded blocks (roster derived; projection, no re-probe) → Tasks 3–4. ✓
- "status/adapters/components can never disagree" invariant → Task 5. ✓
- `rexops components` CLI → Task 6. ✓
- `status` rendering the roster (design §6) → **deferred**: Phase A adds the `components` subcommand and the data; re-skinning `status` to the nine-row roster is cosmetic and folded into Phase C/F to avoid breaking the behavior-parity constraint this phase commits to. Noted, not a gap.
- Phases B–F (suite-ui widgets, cockpit screen, FeedReady wiring, monitors, gated `launch`) → **out of scope for this plan by design** (this plan is Phase A only; each later phase gets its own plan).

**2. Placeholder scan:** No `TBD`/`TODO`/"handle edge cases"/"similar to Task N". Every code step shows complete code. ✓

**3. Type consistency:**
- `ComponentStatus` fields are identical in Task 2 (definition), Task 4 (construction), Task 6 (rendering): `id, name, group, maturity, health, freshness, vital, launchable`. ✓
- `real_adapter_ids()` (Task 3) is consumed unchanged in Tasks 4–5. ✓
- `component_by_id` / `COMPONENTS` / `HealthSource::Planned` used consistently across Tasks 1, 3, 4. ✓
- `proto` maturity is `Live` in Task 1 then corrected to `FeedReady` in Task 5 Step 2 — flagged explicitly so an out-of-order reader sees the reconciliation. ✓
- `render_components_human` (Task 6) is the name tested and called. ✓

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-06-20-rexops-cockpit-phase-a-registry-spine.md`.
```
