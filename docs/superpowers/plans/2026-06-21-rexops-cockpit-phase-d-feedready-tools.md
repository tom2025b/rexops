# RexOps Cockpit — Phase D: FeedReady Tools — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Promote **ScriptVault** and **ToolFoundry** to full, launchable, `Live` cockpit components by making the `COMPONENTS` registry the **single source of truth for launch data** — retiring the parallel `tools/catalog.rs::CATALOG` in favour of a registry-derived view, so adding/launching a tool is a one-row registry change and `resolve_launch_command` / the Launcher screen / the palette / `is_streamable` / `refreshes_after` can never disagree.

**Architecture:** Today two structures describe launches: the registry's `LaunchSpec` (drives the card's `launchable` flag) and `CATALOG`'s `ToolEntry` (drives the *actual* command args + the Launcher screen + palette). They carry the same launch facts (`run_mode`/`args`/`refresh_after`); `ToolEntry` only adds a human `description`. Phase D adds that one missing fact to the registry (`Component.blurb`), exposes a registry **launchable view** (`rexops_core::launchable_components()`), repoints every launch consumer to read the registry, and **deletes `CATALOG`/`ToolEntry`**. ScriptVault + ToolFoundry then get a `LaunchSpec` and flip `FeedReady → Live`. Their feed health/freshness/vital already work (Phase A); only launch is added. Launch *resolution* is unchanged (`which <id>` then the adapter's configured `binary`), so the moment `scriptvault`/`toolfoundry` resolve as one word they launch — and never invite a launch they can't fulfil (the `arm_tool` health+resolve gate already covers that). No binaries installed, no wrappers added.

**Tech Stack:** Rust 2021, `rexops-core` (`Component`/`LaunchSpec`/`COMPONENTS`/`component_by_id`), `rexops-app` (`snapshot.rs` registry walk), `rexops-tui` (`tools/`, `screens/launchpad.rs`, `commands/palette.rs`, `app/`), `cargo test`/`clippy`/`fmt`. Tests are off-screen registry/unit tests + the existing `TestBackend` launcher/palette render tests.

## Global Constraints

- Files stay **under 300 LOC**; each `.rs` keeps its `// Learning Notes` footer (existing convention).
- All four cargo gates (`build`, `test`, `clippy --workspace -- -D warnings`, `fmt --check`) green at **every** task's commit. Baseline at branch base `04b41a8`: full `cargo test --workspace` green (rexops-tui lib 163; rexops-core/app/cli green).
- **One launch source.** After this plan, the only description of a launchable tool is its `COMPONENTS` row. `CATALOG` and `ToolEntry` are deleted; nothing outside `rexops-core` defines launch args, run mode, or refresh-after.
- **Bulwark + Proto behaviour is preserved exactly** — same Launcher list, same order, same args (`bulwark tui`, `proto` bare), same run modes. A guard test asserts the launchable set + their args are unchanged.
- **Card rendering unchanged except the two promotions** — ScriptVault/ToolFoundry cards gain an *arming* marker (were read-only) and stop being dim-free `feed-ready` → now `live`; the banner rollup goes 3/11 → 5/11. No other card changes; all Phase B/C cockpit tests still pass.
- **No new launch/resolution code path** — `resolve_launch_command` keeps `which`-then-config-`binary`; only its *args source* moves to the registry. No binaries installed, no wrappers/aliases added.
- Conventional commits: `feat(rexops): … (Phase D)` / `refactor(rexops): …` / `test(rexops): …`.
- Run all `cargo` from the worktree root (`/home/tom/projects/rexops/.claude/worktrees/rexops-cockpit-phase-d`).

---

## File Structure

- **Modify** `crates/rexops-core/src/component.rs` — `Component` gains `pub blurb: &'static str` (the human description the Launcher/palette need); update its doc + Learning Notes.
- **Modify** `crates/rexops-core/src/component_table.rs` — all 11 rows gain a `blurb`; ScriptVault + ToolFoundry gain a `LaunchSpec` and flip to `Maturity::Live`; add `pub fn launchable_components() -> Vec<&'static Component>` (the registry launchable view, in table order) + its tests.
- **Modify** `crates/rexops-core/src/lib.rs` — re-export `launchable_components`.
- **Rewrite** `crates/rexops-tui/src/tools/catalog.rs` — delete `CATALOG`/`ToolEntry`; the module becomes registry-backed helpers: `launchable() -> Vec<&'static Component>`, `by_id`, `is_streamable`, `refreshes_after`, all reading the registry `LaunchSpec`. (Keep the `RunMode` reference via `rexops_core`.)
- **Modify** `crates/rexops-tui/src/tools/mod.rs` — update the `pub use` (drop `ToolEntry`/`CATALOG`; export the new view).
- **Modify** `crates/rexops-tui/src/tools/launcher.rs` — `resolve_launch_command` reads args from `component_by_id(id).launch`.
- **Modify** `crates/rexops-tui/src/screens/launchpad.rs` — iterate the registry launchable view (`&'static Component`) instead of `&[ToolEntry]`; `description` → `blurb`.
- **Modify** `crates/rexops-tui/src/commands/palette.rs` — `run <tool>` rows iterate the registry view; `description` → `blurb`.
- **Modify** `crates/rexops-tui/src/app/state.rs` — the launch-availability cache iterates the registry view.
- **Modify** `crates/rexops-tui/src/app/update.rs` — Launcher `selected_tool` bounds/Enter use the registry view.
- **Modify** tests in `screens/launchpad.rs` + `app/tests/launcher.rs` — update `CATALOG`/`ToolEntry` references to the registry view.

> Why the view returns `Vec<&'static Component>` (not an iterator or a slice): the Launcher screen indexes it by `selected_tool: usize` and needs `.len()`, so an owned ordered `Vec` of `'static` refs is the simplest indexable shape. It's tiny (≤ a handful of entries), rebuilt only on the Launcher's render/move paths — never per-frame hot inner loop math beyond what `CATALOG` did.

---

### Task 1: `Component.blurb` + the registry launchable view (core)

**Files:**
- Modify: `crates/rexops-core/src/component.rs` (add `blurb` field)
- Modify: `crates/rexops-core/src/component_table.rs` (add `blurb` to all 11 rows; add `launchable_components()` + tests)
- Modify: `crates/rexops-core/src/lib.rs` (re-export)
- Test: inline `#[cfg(test)]` in `component_table.rs`

**Interfaces:**
- Produces:
  - `Component` gains `pub blurb: &'static str` — a short human description (the one fact `ToolEntry` had that the registry lacked).
  - `pub fn launchable_components() -> Vec<&'static Component>` — the components whose `launch.is_some()`, in `COMPONENTS` (display) order. The single list the Launcher screen + palette will iterate.

> This task only adds the field + view in core; it does NOT yet add LaunchSpecs to ScriptVault/ToolFoundry (Task 2) or change any tui consumer (Tasks 3–5). After it, `launchable_components()` yields exactly `[bulwark, proto]` (the two rows that already have a `LaunchSpec`), proving the view matches today's CATALOG set before anything is repointed.

- [ ] **Step 1: Add `blurb` to the `Component` struct**

In `crates/rexops-core/src/component.rs`, add the field (keep all others + the doc):

```rust
/// One box in the suite metaphor, as pure data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Component {
    pub id: &'static str,
    pub name: &'static str,
    pub role: &'static str,
    /// A short human description for the Launcher screen + command palette (the
    /// "what is this tool" line). The registry is the single source of launch
    /// data, so this lives here rather than in a separate catalog.
    pub blurb: &'static str,
    pub group: ComponentGroup,
    pub health: HealthSource,
    pub launch: Option<LaunchSpec>,
    pub feed: Option<FeedSpec>,
    pub maturity: Maturity,
}
```

- [ ] **Step 2: Write the failing test**

In `crates/rexops-core/src/component_table.rs`, add to the `tests` module:

```rust
    #[test]
    fn every_component_has_a_nonempty_blurb() {
        for c in COMPONENTS {
            assert!(!c.blurb.is_empty(), "{} must have a blurb", c.id);
        }
    }

    #[test]
    fn launchable_view_is_exactly_the_rows_with_a_launch_spec() {
        let ids: Vec<&str> = launchable_components().iter().map(|c| c.id).collect();
        // Before Task 2, only bulwark + proto carry a LaunchSpec, in table order.
        assert_eq!(ids, vec!["bulwark", "proto"]);
        // And the view must agree with the predicate it claims to implement.
        for c in launchable_components() {
            assert!(c.launch.is_some(), "{} in the view must be launchable", c.id);
        }
    }
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p rexops-core component_table 2>&1 | tail -20`
Expected: FAIL to COMPILE — `blurb` missing from every row literal, and `launchable_components` not found.

- [ ] **Step 4: Add `blurb` to all 11 rows + the view fn**

In `crates/rexops-core/src/component_table.rs`, add a `blurb` to each `Component` literal. Use these (concise, accurate):

```text
workstate    → "Suite brain — registry + state hub"
system       → "Host facts (kernel, uptime, load)"
bulwark      → "Content/security inspection (live scan)"
proto        → "Protocol / checklist runner (interactive picker)"
scriptvault  → "Script library + runner"
toolfoundry  → "Tool build/lifecycle manager"
pulse        → "Heartbeat / liveness monitor"
tripwire     → "Change/intrusion alarm"
rewind       → "Black-box event recorder"
rex-check    → "Suite health checks / doctor"
rex-forge    → "Scaffolder — new tools from templates"
```

(For `bulwark`/`proto`, reuse the exact `description` strings currently in `CATALOG` so the Launcher copy is unchanged.)

Then add the view fn above the `tests` module:

```rust
/// The launchable components — those with a `LaunchSpec` — in registry (display)
/// order. This is the single list the Launcher screen and command palette
/// iterate, replacing the old hand-maintained `CATALOG`. Adding a launchable tool
/// is now exactly: give its registry row a `launch`.
pub fn launchable_components() -> Vec<&'static Component> {
    COMPONENTS.iter().filter(|c| c.launch.is_some()).collect()
}
```

- [ ] **Step 5: Re-export from lib.rs**

In `crates/rexops-core/src/lib.rs`, add `launchable_components` to the `component_table` re-export (next to `component_by_id, COMPONENTS`):

```rust
pub use component_table::{component_by_id, launchable_components, COMPONENTS};
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p rexops-core component_table 2>&1 | tail -20`
Expected: `every_component_has_a_nonempty_blurb` + `launchable_view_is_exactly_the_rows_with_a_launch_spec` PASS, plus the existing registry tests (`every_component_id_is_unique_and_valid`, etc.).

- [ ] **Step 7: Gates green, then commit**

Run: `cargo fmt && cargo clippy -p rexops-core -- -D warnings && cargo test -p rexops-core 2>&1 | tail -5`
Expected: all green.

```bash
git add crates/rexops-core/src/component.rs crates/rexops-core/src/component_table.rs crates/rexops-core/src/lib.rs
git commit -m "feat(rexops): Component.blurb + launchable_components registry view (Phase D)"
```

---

### Task 2: Promote ScriptVault + ToolFoundry (registry rows → launchable + Live)

**Files:**
- Modify: `crates/rexops-core/src/component_table.rs` (two rows)
- Modify: `crates/rexops-app/src/snapshot.rs` (test expectations for the live roster)
- Test: inline tests in both files

**Interfaces:**
- Consumes: `LaunchSpec`, `RunMode` (already imported in `component_table.rs`).
- Produces: `scriptvault` + `toolfoundry` rows now have `launch: Some(LaunchSpec { Foreground, &[], refresh_after: false })` and `maturity: Maturity::Live`. `launchable_components()` now yields `[bulwark, proto, scriptvault, toolfoundry]` (table order).

- [ ] **Step 1: Write the failing test**

In `crates/rexops-core/src/component_table.rs` `tests`, add:

```rust
    #[test]
    fn scriptvault_and_toolfoundry_are_launchable_live() {
        for id in ["scriptvault", "toolfoundry"] {
            let c = component_by_id(id).expect("present");
            assert!(c.launch.is_some(), "{id} must be launchable in Phase D");
            assert_eq!(c.maturity, Maturity::Live, "{id} must be Live in Phase D");
        }
        // The launchable view now includes them, after bulwark + proto.
        let ids: Vec<&str> = launchable_components().iter().map(|c| c.id).collect();
        assert_eq!(ids, vec!["bulwark", "proto", "scriptvault", "toolfoundry"]);
    }
```

This also makes the Task-1 `launchable_view_is_exactly...` test's `["bulwark","proto"]` assertion **stale** — update that earlier test's expectation to the new four-element list (note it in the commit; it's the same invariant, new data):

```rust
        assert_eq!(ids, vec!["bulwark", "proto", "scriptvault", "toolfoundry"]);
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-core component_table 2>&1 | tail -20`
Expected: FAIL — both rows still `launch: None` / `FeedReady`.

- [ ] **Step 3: Promote the two rows**

In `crates/rexops-core/src/component_table.rs`, edit the `scriptvault` and `toolfoundry` rows:

```rust
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
```

- [ ] **Step 4: Fix the app-layer live-roster invariant (a deliberate semantic change)**

`crates/rexops-app/src/snapshot.rs` has the test
`status_adapters_and_components_never_disagree_on_the_live_roster`, which encodes
a **Phase-A invariant**: the three views (`adapter_health` keys, registry
`adapters`, and the **`live`-maturity component cards**) are *exactly* the adapter
roster (`bulwark`/`system`/`workstate`). Phase D **deliberately changes what
`live` means**: a feed-backed tool with a launch is now `Live` too, so the
`live`-cards leg of this invariant is no longer the adapter roster — it is the
adapter roster **plus** the two feed-backed launchables. The two *cross-source*
legs (`adapter_health == registry == adapter roster`) are unchanged: feeds are not
adapters, so the adapter roster stays 3.

Run: `cargo test -p rexops-app status_adapters_and_components_never_disagree 2>&1 | tail -20`
Expected: FAIL on the third assertion — `live_components` is now
`[bulwark, scriptvault, system, toolfoundry, workstate]` (5) but `expected` is the
3-id adapter roster.

Fix the test so it asserts the **two** invariants that still hold and the **new**
`live` meaning. Replace the single `live_components == expected` assertion block
(and its comment) with:

```rust
        // The two cross-source rosters still agree exactly with the adapter
        // roster — feeds are not adapters, so adding feed-backed Live tools does
        // not change adapter_health or the registry adapter list.
        assert_eq!(from_adapter_health, expected, "status roster");
        assert_eq!(from_registry, expected, "adapters roster");

        // Phase D: `live` now means "fully wired" — the adapter roster PLUS the
        // feed-backed launchable tools (ScriptVault + ToolFoundry). So the live
        // cards are a SUPERSET of the adapter roster, not equal to it. Assert the
        // exact new live set rather than the old "== adapter roster".
        let mut expected_live = expected.clone();
        expected_live.push("scriptvault".to_owned());
        expected_live.push("toolfoundry".to_owned());
        expected_live.sort();
        assert_eq!(live_components, expected_live, "live component cards");
```

Also rename the test to reflect the change (its old name claims the three "never
disagree on the live roster", which is no longer literally true):
`status_and_adapters_agree_on_the_roster_and_live_is_that_roster_plus_feed_tools`.
Update the doc comment's "must be exactly the adapter roster" sentence to the
Phase-D meaning (live = adapter roster + feed-backed launchables).

> Also grep for any *other* stale expectation: `grep -n "feed-ready\|\.launchable" crates/rexops-app/src/snapshot.rs`. If a test asserts `scriptvault`/`toolfoundry` are `feed-ready` or `!launchable`, flip it to the Phase-D truth (`live`, launchable when resolvable). The structural tests (one status per component) stay.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rexops-core component_table 2>&1 | tail -10`
Expected: `scriptvault_and_toolfoundry_are_launchable_live` + the updated view test PASS.

Run: `cargo test -p rexops-app 2>&1 | tail -8`
Expected: green (the live-roster expectation now matches).

- [ ] **Step 6: Gates green, then commit**

Run: `cargo fmt && cargo clippy --workspace -- -D warnings && cargo test -p rexops-core -p rexops-app 2>&1 | tail -6`
Expected: all green.

```bash
git add crates/rexops-core/src/component_table.rs crates/rexops-app/src/snapshot.rs
git commit -m "feat(rexops): ScriptVault + ToolFoundry become launchable Live components (Phase D)"
```

---

### Task 3: Rewrite `tools/catalog.rs` as a registry-backed view

**Files:**
- Rewrite: `crates/rexops-tui/src/tools/catalog.rs`
- Modify: `crates/rexops-tui/src/tools/mod.rs` (update `pub use`)
- Test: inline `#[cfg(test)]` in `catalog.rs`

**Interfaces:**
- Consumes: `rexops_core::{launchable_components, component_by_id, Component, RunMode}`.
- Produces (the new `tools::catalog` surface; `CATALOG` + `ToolEntry` are DELETED):
  - `pub fn launchable() -> Vec<&'static rexops_core::Component>` — re-exposes the registry view to the tui crate (the Launcher screen iterates this).
  - `pub fn by_id(id: &str) -> Option<&'static rexops_core::Component>` — `component_by_id`, narrowed name kept so callers read naturally.
  - `pub fn is_streamable(tool_id: &str) -> bool` — `true` iff the component's `launch.run_mode == Background`.
  - `pub fn refreshes_after(tool_id: &str) -> bool` — the component's `launch.refresh_after` (or `false`).

- [ ] **Step 1: Write the failing test**

Replace the `tests` module in `catalog.rs` with registry-backed assertions:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launchable_lists_the_registry_launch_rows_in_order() {
        let ids: Vec<&str> = launchable().iter().map(|c| c.id).collect();
        assert_eq!(ids, vec!["bulwark", "proto", "scriptvault", "toolfoundry"]);
    }

    #[test]
    fn run_mode_helpers_read_the_registry() {
        // All four current launchables are Foreground → not streamable.
        for id in ["bulwark", "proto", "scriptvault", "toolfoundry"] {
            assert!(!is_streamable(id), "{id} is Foreground, not streamable");
        }
        // Unknown ids are inert, never panic.
        assert!(!is_streamable("nope"));
        assert!(!refreshes_after("nope"));
        assert!(!refreshes_after("bulwark"), "bulwark does not refresh-after");
    }

    #[test]
    fn by_id_finds_a_registry_component() {
        assert_eq!(by_id("scriptvault").map(|c| c.name), Some("ScriptVault"));
        assert!(by_id("not-a-tool").is_none());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-tui catalog:: 2>&1 | tail -20`
Expected: FAIL to compile — `launchable`/registry-backed helpers not defined yet (the old `CATALOG`-based ones still present).

- [ ] **Step 3: Rewrite the module body**

Replace the entire non-test body of `crates/rexops-tui/src/tools/catalog.rs` with:

```rust
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
```

(Keep the existing Learning Notes footer, updated to reflect the registry view; delete the `RunMode` enum re-definition if any — it now comes from `rexops_core`.)

- [ ] **Step 4: Update `tools/mod.rs` exports**

In `crates/rexops-tui/src/tools/mod.rs`, change the catalog re-export (drop `ToolEntry`/`CATALOG`; add the view):

```rust
pub use catalog::{by_id, is_streamable, launchable, refreshes_after};
```

(Leave the `launcher::{…}` re-export untouched.)

- [ ] **Step 5: Run tests (catalog compiles in isolation; the crate may not yet)**

Run: `cargo test -p rexops-tui catalog:: 2>&1 | tail -20`
Expected: the three catalog tests PASS. The wider crate will still fail to build until Tasks 4–5 repoint the consumers (`launcher.rs`, `launchpad.rs`, `palette.rs`, `state.rs`, `update.rs`, launcher tests) — that's expected; **do not commit yet** (the crate must build to commit). Proceed to Task 4 and commit Tasks 3–5 together at the end of Task 5 (the crate is green only once all consumers are repointed).

> Commit boundary note: Task 3 alone leaves the crate non-building (consumers still import `CATALOG`). To honour "green at every commit", Tasks 3, 4, and 5 are ONE commit, made at the end of Task 5. Each task's steps are still followed in order; only the commit is deferred to the point the crate compiles.

---

### Task 4: Repoint `resolve_launch_command` to the registry args

**Files:**
- Modify: `crates/rexops-tui/src/tools/launcher.rs`
- Test: the existing launcher resolution tests (adjust if they reference `ToolEntry`)

**Interfaces:**
- Consumes: `tools::by_id` (now registry-backed) → `Component.launch.args`.
- Produces: `resolve_launch_command(id, config)` builds `LaunchCommand { program, args }` where `args` come from `component_by_id(id).launch.args` instead of `catalog::by_id(id).launch_args`.

- [ ] **Step 1: Update `resolve_launch_command`**

In `crates/rexops-tui/src/tools/launcher.rs`, change the args source (the program resolution via `resolve_command` — `which` then config `binary` — is unchanged):

```rust
/// Resolve the complete launch command for a tool, including any registry-owned
/// arguments needed to open its interactive surface.
pub fn resolve_launch_command(tool_id: &str, config: &AppConfig) -> Option<LaunchCommand> {
    let program = resolve_command(tool_id, config)?;
    let args = catalog::by_id(tool_id)
        .and_then(|c| c.launch)
        .map(|l| l.args.iter().map(|a| (*a).to_owned()).collect())
        .unwrap_or_default();
    Some(LaunchCommand { program, args })
}
```

(`catalog::by_id` now returns `&'static Component`; `c.launch` is `Option<LaunchSpec>`; `l.args` is `&'static [&'static str]`. Adjust the `use` if `catalog` isn't already in scope — it is, via the existing `launcher.rs` references.)

- [ ] **Step 2: Check the launcher resolution tests**

Run: `grep -n "ToolEntry\|CATALOG\|launch_args" crates/rexops-tui/src/tools/launcher.rs`
Expected: no remaining references (the only one was in `resolve_launch_command`, now changed). If a test in this file constructs a `ToolEntry` or asserts on `launch_args`, repoint it to the registry component (`component_by_id("proto").unwrap().launch.unwrap().args`). Most launcher resolution tests assert on the resolved *program* (config/`which`), which is unchanged.

> No commit here — see the Task 3 commit-boundary note; this lands with Task 5.

---

### Task 5: Repoint the Launcher screen, palette, and app state to the registry view

**Files:**
- Modify: `crates/rexops-tui/src/screens/launchpad.rs` (iterate `launchable()` / `&Component`; `description` → `blurb`)
- Modify: `crates/rexops-tui/src/commands/palette.rs` (iterate `launchable()`; `description` → `blurb`)
- Modify: `crates/rexops-tui/src/app/state.rs` (availability cache iterates `launchable()`)
- Modify: `crates/rexops-tui/src/app/update.rs` (Launcher `selected_tool` bounds/Enter use `launchable()`)
- Modify: `crates/rexops-tui/src/app/tests/launcher.rs` + `screens/launchpad.rs` tests (CATALOG → registry view)

**Interfaces:**
- Consumes: `tools::launchable()` (ordered `Vec<&'static Component>`), `Component.{id,name,blurb}`.
- Produces: every former `CATALOG` consumer now reads the registry view. The Launcher screen's row/detail render off `&Component`; the palette's `run <tool>` rows off `&Component`; the availability cache keys off `launchable()`; `selected_tool` indexes `launchable()`.

> Pattern for all sites: `CATALOG.iter()` → `tools::launchable().into_iter()` (or bind `let tools = tools::launchable();` once and index/iterate it); `tool.description` → `tool.blurb`; `&ToolEntry` → `&rexops_core::Component`. `tool.id` / `tool.name` are unchanged (both types have them).

- [ ] **Step 1: Launcher screen (`screens/launchpad.rs`)**

Replace `use crate::tools::{ToolEntry, CATALOG};` with `use crate::tools;` (and `use rexops_core::Component;` if needed for the row helper signature). Then:
- `render_launcher_row(app, index, tool: &ToolEntry, theme)` → `tool: &Component`.
- The list builder: `let tools = tools::launchable(); … tools.iter().enumerate().map(|(i, tool)| render_launcher_row(app, i, tool, theme))`.
- The detail pane: `tools::launchable().get(app.selected_tool)`; `tool.description` → `tool.blurb`.
- The test `catalog_includes_proto_as_launchable` → assert `tools::launchable().iter().any(|c| c.id == "proto")`; `catalog_ids_are_unique` → over `tools::launchable()`.

- [ ] **Step 2: Palette (`commands/palette.rs`)**

Replace `use crate::tools::CATALOG;` with `use crate::tools;`. The `run <tool>` row builders: `for tool in tools::launchable()` (both the build loop and any count loop); `tool.description` → `tool.blurb`.

- [ ] **Step 3: Availability cache (`app/state.rs`)**

In `refresh_launch_availability`, change `CATALOG.iter().map(|tool| (tool.id, …))` to iterate `tools::launchable()` — `(c.id, tools::resolve_launch_command(c.id, self.config()).is_some())`. The cache key type is `&'static str`; `Component.id` is `&'static str`, so it still fits. Update the `use crate::tools::{self, CATALOG};` import to drop `CATALOG`.

> Note: the availability cache `HashMap<&'static str, bool>` keys on `id`; `launchable()` returns `&'static Component` whose `id` is `&'static str`, so the map type is unchanged.

- [ ] **Step 4: Launcher navigation (`app/update.rs`)**

In `move_selection`'s `Screen::Launcher` arm and `activate_selection`'s `Screen::Launcher` arm, replace `tools::CATALOG.len()` / `tools::CATALOG.get(self.selected_tool)` with `tools::launchable().len()` / `tools::launchable().get(self.selected_tool)`. The armed values are `tool.id` / `tool.name` (both present on `Component`).

> Bind once per call where it reads cleaner: `let tools = tools::launchable();` then `tools.len()` / `tools.get(idx)`.

- [ ] **Step 5: Launcher tests (`app/tests/launcher.rs`)**

Update every `CATALOG` reference to `tools::launchable()` (e.g. `CATALOG.len()` → `crate::tools::launchable().len()`, `&CATALOG[i]` → `crate::tools::launchable()[i]`). The behavioural assertions (selection wraps, detail follows selection, arming opens the gate) are unchanged — they just index the registry view now.

> Confirm scope first: `grep -rn "CATALOG\|ToolEntry" crates/rexops-tui/src` — after Steps 1–5 there must be ZERO matches anywhere in the crate (the deletion is complete).

- [ ] **Step 6: Build, run the whole crate, then commit Tasks 3–5**

Run: `grep -rn "CATALOG\|ToolEntry" crates/rexops-tui/src 2>&1`
Expected: no matches.

Run: `cargo test -p rexops-tui 2>&1 | tail -8`
Expected: all rexops-tui tests pass (launcher list now == registry launchable view; the cockpit + palette + launcher tests green).

Run: `cargo fmt && cargo clippy --workspace -- -D warnings 2>&1 | tail -3`
Expected: clean.

```bash
git add crates/rexops-tui/src/tools/catalog.rs crates/rexops-tui/src/tools/mod.rs crates/rexops-tui/src/tools/launcher.rs crates/rexops-tui/src/screens/launchpad.rs crates/rexops-tui/src/commands/palette.rs crates/rexops-tui/src/app/state.rs crates/rexops-tui/src/app/update.rs crates/rexops-tui/src/app/tests/launcher.rs
git commit -m "refactor(rexops): retire CATALOG — launch data reads from the registry (Phase D)"
```

---

### Task 6: Unification guard + cockpit verification + smoke

**Files:**
- Modify: `crates/rexops-tui/src/app/tests/launcher.rs` (or `screens/launchpad.rs` tests) — add the unification guard
- Modify (optional): docs

**Interfaces:** none (verification + a guard test that locks the invariant).

- [ ] **Step 1: Add the unification guard test**

Add a test asserting the Launcher screen's list is exactly the registry's launchable set (so the two can never drift again). In `crates/rexops-tui/src/app/tests/launcher.rs`:

```rust
#[test]
fn launcher_list_is_exactly_the_registry_launchable_set() {
    // The Phase D invariant: there is ONE launch source. The Launcher's list must
    // equal the registry components with a LaunchSpec, in registry order — if a
    // future row gains/loses a launch, the screen follows with no second list to
    // update.
    let screen: Vec<&str> = crate::tools::launchable().iter().map(|c| c.id).collect();
    let registry: Vec<&str> = rexops_core::COMPONENTS
        .iter()
        .filter(|c| c.launch.is_some())
        .map(|c| c.id)
        .collect();
    assert_eq!(screen, registry, "Launcher list must equal the registry launch set");
    // And the two Phase D promotions are present.
    assert!(screen.contains(&"scriptvault"));
    assert!(screen.contains(&"toolfoundry"));
}
```

- [ ] **Step 2: Run it + the full crate**

Run: `cargo test -p rexops-tui launcher_list_is_exactly_the_registry_launchable_set 2>&1 | tail -8`
Expected: PASS.

Run: `cargo test -p rexops-tui 2>&1 | tail -6`
Expected: all green.

- [ ] **Step 3: Cockpit render verification (ScriptVault/ToolFoundry now arm)**

The cockpit cards for these two were read-only; they now carry an arming marker and read `live`. Confirm via the existing cockpit render test path that they render with a marker, and that the banner counts them. Add a focused assertion to `screens/cockpit.rs` tests (the `app_with_components` fixture builds its own snapshot; extend it or add a new test that pushes a launchable scriptvault component and asserts its card shows a marker `[`). Minimal new test:

```rust
    #[test]
    fn a_launchable_field_tool_renders_a_marker() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::new(tx, AppConfig::default(), None);
        let mut snap = OpsSnapshot::new();
        snap.push_component(ComponentStatus {
            id: "scriptvault".into(), name: "ScriptVault".into(), group: "field tool".into(),
            maturity: "live".into(), health: AdapterHealth::Healthy, freshness: None,
            vital: Some("3 scripts".into()), launchable: true,
        });
        app.apply_snapshot(snap);
        let text = render(&app);
        assert!(text.contains("ScriptVault"), "card present:\n{text}");
        assert!(text.contains('['), "launchable field tool shows a marker:\n{text}");
    }
```

Run: `cargo test -p rexops-tui screens::cockpit 2>&1 | tail -8`
Expected: PASS (this new test + the Phase B/C cockpit tests unchanged).

- [ ] **Step 4: Headless smoke**

Run: `cat crates/rexops-adapters/fixtures/workstate/snapshot_v3.json | cargo run -q -p rexops-cli -- components 2>/dev/null`
Expected: ScriptVault + ToolFoundry now show **`live`** (was `feed-ready`) with their vitals (`3 scripts` / `1 need review`); the other rows unchanged. (This is the registry projection the cockpit reads.)

> Interactive cockpit smoke (launching ScriptVault by its letter) requires a TTY + the `scriptvault` binary resolving as one word; if either is unavailable, the `app::tests::launcher` arming tests + the unification guard cover the path. Note the outcome in the report.

- [ ] **Step 5: Docs + final workspace gate**

Update `docs/TUI_DESIGN.md` (one line: launch data now lives in the registry; the Launcher screen + cockpit launch are a view over `COMPONENTS`). Update `LAST_WORK.md` at the suite root per the project rule (note: that file is in the linux-ops-suite repo, committed there separately, as in Phase C).

Run: `cargo fmt && cargo clippy --workspace -- -D warnings && cargo test --workspace 2>&1 | tail -8`
Expected: all green across the workspace.

```bash
git add crates/rexops-tui/src/app/tests/launcher.rs crates/rexops-tui/src/screens/cockpit.rs docs/TUI_DESIGN.md
git commit -m "test(rexops): lock Launcher list to the registry launch set + cockpit marker (Phase D)"
```

---

## Self-Review

**1. Spec coverage (against the Phase D design):**
- §3.1 `Component.blurb` → Task 1. ✓
- §3.2 registry launchable view (`launchable_components`) → Task 1. ✓
- §3.3 `resolve_launch_command` reads the registry args → Task 4. ✓
- §3.4 `is_streamable`/`refreshes_after` read the registry → Task 3. ✓
- §3.5 `CATALOG`/`ToolEntry` deleted; Launcher screen + palette + availability cache + nav over the registry view → Tasks 3 & 5. ✓
- §4 ScriptVault + ToolFoundry get a `LaunchSpec` + `Live` → Task 2. ✓
- §9.3 one launch source + guard test → Task 6 (`launcher_list_is_exactly_the_registry_launchable_set`). ✓
- §9.4 Bulwark + Proto unchanged → asserted by the ordered-list tests (Tasks 1/2/3) keeping `["bulwark","proto",…]` first with their existing args. ✓
- §9.2 banner 5/11 live → follows from Task 2 (maturity Live) + the existing banner `live` count; the smoke (Task 6) shows `live`. ✓

**2. Placeholder scan:** No TBD/TODO. Every code step shows complete code. The two "confirm via grep" steps (app-layer live-roster test in Task 2; residual `CATALOG` refs in Task 5) are concrete grep instructions with the exact edit named, not placeholders.

**3. Type consistency:**
- `Component.blurb: &'static str` (Task 1) read as `tool.blurb` in launchpad/palette (Task 5). ✓
- `launchable_components() -> Vec<&'static Component>` (Task 1) re-exposed as `tools::launchable()` (Task 3), iterated/indexed in launchpad/palette/state/update/tests (Tasks 5–6). ✓
- `catalog::by_id -> Option<&'static Component>` (Task 3) used by `resolve_launch_command` via `c.launch.args` (Task 4) — `launch: Option<LaunchSpec>`, `LaunchSpec.args: &'static [&'static str]`. ✓
- `is_streamable`/`refreshes_after` read `c.launch.run_mode`/`.refresh_after` (Task 3); callers (`state.rs` availability, `jobs/manager.rs` refreshes_after) unchanged in shape (still by id). ✓
- The availability cache `HashMap<&'static str, bool>` keys unchanged (`Component.id` is `&'static str`). ✓
- The Launcher `selected_tool: usize` indexes `tools::launchable()` (Task 5) — `.len()`/`.get()` available on `Vec`. ✓

**4. Commit-boundary correctness (green at every commit):** Tasks 1, 2, and 6 each build+commit independently. Tasks 3–5 are deliberately ONE commit (Task 3 alone leaves the crate non-building because consumers still import `CATALOG`); the plan states this explicitly and defers the commit to the end of Task 5, where `grep CATALOG` is empty and the crate is green. The four gates pass at each of the four commits.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-06-21-rexops-cockpit-phase-d-feedready-tools.md`.
