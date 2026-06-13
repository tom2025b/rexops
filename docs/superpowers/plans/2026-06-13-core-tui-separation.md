# Core / TUI Separation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the executable business logic (tool catalog, job process model, job-outcome records, tool launcher, snapshot-refresh driver) out of `rexops-tui` into the shared `rexops-app` crate, leaving the TUI as rendering + input + thin glue.

**Architecture:** Two-layer separation. `rexops-core` stays pure data (untouched). `rexops-app` becomes the genuine shared business-logic layer (it already owns `build_snapshot` and is allowed side effects). `rexops-tui` keeps only ratatui/crossterm/`suite_ui` rendering, input decoding, and `App` glue that calls into the app layer. The one friction point — three `suite_ui` presentation enums (`Outcome`, `JobState`, `ToastKind`) used by job code — is severed by introducing plain domain enums in `rexops-app` and mapping to `suite_ui` at the TUI render boundary.

**Tech Stack:** Rust 2021, workspace crates, `std::process`/`std::thread`/`std::sync::mpsc` for jobs, serde in core, ratatui/crossterm/`suite-ui` (git dep) in the TUI only. Four cargo gates apply throughout: `cargo build`, `cargo test --workspace`, `cargo clippy --workspace`, `cargo fmt --check`.

**Baseline (must stay green every step):** 174 tests — 7 `rexops-app`, 49 core/adapters, 118 `rexops-tui`. Run `cargo test --workspace` to confirm.

**Conventions carried from the codebase:**
- Files stay well under 300 LOC; educational module docs.
- `#![deny(clippy::unwrap_used, clippy::expect_used)]` + `#![warn(clippy::all, clippy::pedantic)]` at each crate root.
- Launcher/launch tests must pin an OFF-PATH fake id via config when asserting the resolved command (a real `which <tool>` on the dev box otherwise wins).
- `rexops-app` must NEVER gain a `suite-ui` dependency. If a move wants it, that code is a render concern and stays in the TUI.

**Re-export discipline (the mechanism that keeps each step green):** When a type moves from `rexops-tui` to `rexops-app`, the TUI module it left (`tools::`, `jobs::`) re-exports it from its new home so the many `use crate::tools::…` / `use crate::jobs::…` call sites compile unchanged. Import cleanup happens only in the final tidy task.

---

## File Structure (after this plan)

**rexops-app — new files:**
- `crates/rexops-app/src/jobs/mod.rs` — module wiring + re-exports for the jobs submodule.
- `crates/rexops-app/src/jobs/process.rs` — `JobHandle`, `JobOutput`, `JobExit`, `spawn` (moved verbatim from the TUI).
- `crates/rexops-app/src/jobs/outcome.rs` — `JobOutcome`, `JobLifecycle` domain enums + `LastOutcome`, `JobRecord` records.
- `crates/rexops-app/src/tools/mod.rs` — module wiring + re-exports for the tools submodule.
- `crates/rexops-app/src/tools/catalog.rs` — `RunMode`, `ToolEntry`, `CATALOG`, `by_id`, `is_streamable` (moved).
- `crates/rexops-app/src/tools/launcher.rs` — `ForegroundRunner`, `LaunchCommand`, `ChildExit`, `LaunchReport`, `launch_tool`, `resolve_command`, `resolve_launch_command` (moved).
- `crates/rexops-app/src/refresh.rs` — `spawn_refresh`, `panicked_snapshot` (extracted from `App`).

**rexops-app — modified:**
- `crates/rexops-app/src/lib.rs` — declare new modules; re-export the new public surface.

**rexops-tui — modified (shrink to glue):**
- `crates/rexops-tui/src/tools/mod.rs` — re-export catalog + launcher from `rexops_app`; keep nothing local.
- `crates/rexops-tui/src/tools/catalog.rs` — deleted (moved).
- `crates/rexops-tui/src/tools/launcher.rs` — reduced to the `impl ForegroundRunner for Tui` only (the terminal-touching part), or moved to `lib.rs`/`runtime.rs` — see Task 5.
- `crates/rexops-tui/src/jobs/mod.rs` — re-export process + outcome types from `rexops_app`; keep `manager` local.
- `crates/rexops-tui/src/jobs/process.rs` — deleted (moved).
- `crates/rexops-tui/src/jobs/manager.rs` — keeps the `impl App` methods; `LastOutcome`/`JobRecord`/`toast_for`/`job_state` adapted to map domain → `suite_ui`.
- `crates/rexops-tui/src/app/state.rs` — `request_refresh` calls `rexops_app::spawn_refresh`; `panicked_snapshot` removed (moved).

---

## Task 0: Confirm the green baseline

**Files:** none (verification only).

- [ ] **Step 1: Run the full suite and record the count**

Run: `cargo test --workspace 2>&1 | grep "test result"`
Expected: every line `ok`; totals add to **174** passed, 0 failed.

- [ ] **Step 2: Confirm clippy + fmt are clean**

Run: `cargo clippy --workspace --all-targets 2>&1 | grep -E "^error|^warning" ; cargo fmt --check && echo FMT-CLEAN`
Expected: no clippy errors/warnings; `FMT-CLEAN` printed.

---

## Task 1: Move the tool catalog into rexops-app

The catalog is pure static data with zero `suite_ui` / ratatui imports — the safest first move, and it proves the move-and-re-export pipeline.

**Files:**
- Create: `crates/rexops-app/src/tools/mod.rs`
- Create: `crates/rexops-app/src/tools/catalog.rs`
- Modify: `crates/rexops-app/src/lib.rs`
- Modify: `crates/rexops-tui/src/tools/mod.rs`
- Delete: `crates/rexops-tui/src/tools/catalog.rs`

- [ ] **Step 1: Create `crates/rexops-app/src/tools/catalog.rs`** with the exact contents of the current TUI `tools/catalog.rs`:

```rust
//! Static launcher catalog and per-tool execution-mode metadata.

/// How a tool runs when launched from a front-end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Hands over the real terminal (interactive tools).
    Foreground,
    /// Streams output into a background-job view.
    Background,
}

pub struct ToolEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub run_mode: RunMode,
    pub launch_args: &'static [&'static str],
}

pub const CATALOG: &[ToolEntry] = &[
    ToolEntry {
        id: "bulwark",
        name: "Bulwark",
        description: "Content/security inspection (live scan)",
        run_mode: RunMode::Foreground,
        launch_args: &["tui"],
    },
    ToolEntry {
        id: "proto",
        name: "Proto",
        description: "Guided protocol / checklist runner (interactive)",
        run_mode: RunMode::Foreground,
        launch_args: &[],
    },
    ToolEntry {
        id: "scripts",
        name: "Scripts",
        description: "Script inventory from Workstate",
        run_mode: RunMode::Background,
        launch_args: &[],
    },
    ToolEntry {
        id: "tools",
        name: "Tools",
        description: "Tool ownership & lifecycle from Workstate",
        run_mode: RunMode::Background,
        launch_args: &[],
    },
    ToolEntry {
        id: "workstate",
        name: "Workstate",
        description: "Snapshot source of truth",
        run_mode: RunMode::Background,
        launch_args: &[],
    },
];

pub fn by_id(id: &str) -> Option<&'static ToolEntry> {
    CATALOG.iter().find(|tool| tool.id == id)
}

/// True when the tool runs as a background job whose output can stream into a
/// view (as opposed to taking over the terminal).
pub fn is_streamable(tool_id: &str) -> bool {
    matches!(
        by_id(tool_id).map(|tool| tool.run_mode),
        Some(RunMode::Background)
    )
}
```

(The only change from the TUI original is the doc comment wording: "from the TUI" → "from a front-end", "Jobs screen" → "a background-job view", since this is now front-end-agnostic.)

- [ ] **Step 2: Create `crates/rexops-app/src/tools/mod.rs`**

```rust
//! Tool catalog, run mode, and launch orchestration (shared business logic).

pub mod catalog;

pub use catalog::{by_id, is_streamable, RunMode, ToolEntry, CATALOG};
```

(Launcher is added to this module in Task 5.)

- [ ] **Step 3: Declare and re-export from `crates/rexops-app/src/lib.rs`.** Add `mod tools;` alongside the existing `mod config;` / `mod snapshot;`, and add to the re-export block:

```rust
pub mod tools;
pub use tools::{by_id, is_streamable, RunMode, ToolEntry, CATALOG};
```

Use `pub mod tools;` (not `mod tools;`) so callers can also reach submodule items if needed; the flat re-exports cover the common names.

- [ ] **Step 4: Delete `crates/rexops-tui/src/tools/catalog.rs`.**

Run: `rm crates/rexops-tui/src/tools/catalog.rs`

- [ ] **Step 5: Re-point `crates/rexops-tui/src/tools/mod.rs`** to re-export the catalog from `rexops_app` instead of a local module. Replace the `pub mod catalog;` line and the catalog re-export with:

```rust
//! Tool catalog, run mode, and launch orchestration.
//!
//! The catalog now lives in `rexops_app` (shared business logic). This module
//! re-exports it so TUI call sites (`crate::tools::CATALOG`, etc.) are unchanged.

pub use rexops_app::{is_streamable, ToolEntry, CATALOG};

pub mod launcher;
// `resolve_launch_command` is the single public entry point for "what runs when
// this tool launches" — program plus catalog args. Both run surfaces (the
// foreground launcher and the background job manager) and the confirm-gate
// preview go through it, so they can never disagree about the invocation.
// `resolve_command` (program only) stays an internal helper of `launcher`.
pub use launcher::{
    launch_tool, resolve_launch_command, ChildExit, ForegroundRunner, LaunchCommand,
};
```

(Note: `tools/launcher.rs` still imports `super::catalog` — fix that in this step too: change `use super::catalog;` in `crates/rexops-tui/src/tools/launcher.rs` to `use rexops_app::tools::catalog;`. This keeps launcher compiling until it moves in Task 5.)

- [ ] **Step 6: Build and test**

Run: `cargo test --workspace 2>&1 | grep "test result"`
Expected: 174 passed, 0 failed (the catalog has no dedicated tests; the launcher and screen tests that reference it must still pass).

Run: `cargo clippy --workspace --all-targets 2>&1 | grep -E "^error|^warning" ; cargo fmt --check && echo FMT-CLEAN`
Expected: no errors/warnings; `FMT-CLEAN`.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor(app): move tool catalog from rexops-tui to rexops-app

Pure static data, no UI deps. TUI re-exports it so call sites are
unchanged. First step of the core/TUI separation.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Task 2: Move the job process model into rexops-app

`jobs/process.rs` is `std`-only (no UI imports) and self-contained, including its own test module. It moves verbatim.

**Files:**
- Create: `crates/rexops-app/src/jobs/mod.rs`
- Create: `crates/rexops-app/src/jobs/process.rs`
- Modify: `crates/rexops-app/src/lib.rs`
- Modify: `crates/rexops-tui/src/jobs/mod.rs`
- Delete: `crates/rexops-tui/src/jobs/process.rs`

- [ ] **Step 1: Create `crates/rexops-app/src/jobs/process.rs`** by copying the current TUI `crates/rexops-tui/src/jobs/process.rs` **verbatim, including its full `#[cfg(test)] mod tests`** (the `drain_into`/backpressure/budget/drop/cancel suite). It has no UI or cross-crate imports — only `std`. Do not edit the bodies.

- [ ] **Step 2: Create `crates/rexops-app/src/jobs/mod.rs`**

```rust
//! Background job process model and finished-job records (shared business logic).

pub mod process;

pub use process::{spawn, JobExit, JobHandle, JobOutput};
```

(The `outcome` submodule with `JobOutcome`/`JobLifecycle`/`LastOutcome`/`JobRecord` is added in Task 3.)

- [ ] **Step 3: Declare and re-export from `crates/rexops-app/src/lib.rs`.** Add `pub mod jobs;` and to the re-export block:

```rust
pub use jobs::{spawn, JobExit, JobHandle, JobOutput};
```

- [ ] **Step 4: Delete `crates/rexops-tui/src/jobs/process.rs`.**

Run: `rm crates/rexops-tui/src/jobs/process.rs`

- [ ] **Step 5: Re-point `crates/rexops-tui/src/jobs/mod.rs`.** Replace `pub mod process;` and the `process::` re-export with a re-export from `rexops_app`. The file becomes:

```rust
//! Background job process and state management.
//!
//! The process model (`JobHandle`, `spawn`, …) now lives in `rexops_app`; this
//! module re-exports it and keeps the App-glue job state transitions in `manager`.

mod manager;

#[cfg(test)]
pub(crate) use manager::{toast_for, JOB_HISTORY_CAP, JOB_OUTPUT_CAP};
pub use manager::{JobRecord, LastOutcome};
pub use rexops_app::{spawn, JobExit, JobHandle, JobOutput};
```

(`JobRecord`/`LastOutcome` still come from `manager` for now — they move in Task 3.)

- [ ] **Step 6: Fix `manager.rs`'s `use super::{JobExit, JobOutput};`** — it still resolves via the re-export in `jobs/mod.rs`, so no change is needed. Verify by building.

- [ ] **Step 7: Build and test**

Run: `cargo test --workspace 2>&1 | grep "test result"`
Expected: 174 passed. The 13-ish job process tests now run under `rexops-app` instead of `rexops-tui` — the per-crate split shifts but the total holds. Confirm `rexops-app` test count rose by the process-test count and `rexops-tui` fell by the same.

Run: `cargo clippy --workspace --all-targets 2>&1 | grep -E "^error|^warning" ; cargo fmt --check && echo FMT-CLEAN`
Expected: no errors/warnings; `FMT-CLEAN`.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor(app): move job process model from rexops-tui to rexops-app

JobHandle/JobOutput/JobExit/spawn are std-only process supervision with
no UI deps; moved verbatim with their test suite. TUI re-exports them.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Task 3: Introduce domain job enums and move the outcome records

This severs the only `suite_ui` coupling in the job logic. We add plain domain enums + records in `rexops-app`, move `LastOutcome`/`JobRecord` there, and have the TUI map domain → `suite_ui` at the render boundary (`toast_for`, `job_state` stay in the TUI).

**Files:**
- Create: `crates/rexops-app/src/jobs/outcome.rs`
- Modify: `crates/rexops-app/src/jobs/mod.rs`
- Modify: `crates/rexops-app/src/lib.rs`
- Modify: `crates/rexops-tui/src/jobs/mod.rs`
- Modify: `crates/rexops-tui/src/jobs/manager.rs`

- [ ] **Step 1: Write the failing test** for the domain outcome classifier. Append to a new `crates/rexops-app/src/jobs/outcome.rs`:

```rust
//! Finished-job outcome and history records, plus the front-end-agnostic
//! domain enums they use. No presentation types here (no toast/colour) — the
//! front-end maps these to its own UI vocabulary at the render boundary.

/// How a finished job ended, as domain truth (no UI/colour meaning).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobOutcome {
    Success,
    Failure,
    Cancelled,
}

/// Where the single job slot is in its lifecycle, as domain truth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobLifecycle {
    Idle,
    Running { name: String },
    Done { name: String, ok: bool },
    Cancelled { name: String },
}

/// How the last job ended, reduced to what a status bar and history need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastOutcome {
    pub name: String,
    pub ok: bool,
    pub cancelled: bool,
}

impl LastOutcome {
    /// Classify this outcome into the domain enum. Cancelled takes precedence
    /// over ok/failure (a cancelled job is neither a clean success nor a real
    /// failure).
    pub fn outcome(&self) -> JobOutcome {
        if self.cancelled {
            JobOutcome::Cancelled
        } else if self.ok {
            JobOutcome::Success
        } else {
            JobOutcome::Failure
        }
    }
}

/// One entry in a bounded job history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRecord {
    pub name: String,
    pub outcome: LastOutcome,
    pub summary: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_outcome_takes_precedence_over_ok() {
        let o = LastOutcome {
            name: "x".to_owned(),
            ok: true,
            cancelled: true,
        };
        assert_eq!(o.outcome(), JobOutcome::Cancelled);
    }

    #[test]
    fn ok_maps_to_success_and_not_ok_to_failure() {
        let ok = LastOutcome {
            name: "x".to_owned(),
            ok: true,
            cancelled: false,
        };
        let bad = LastOutcome {
            name: "x".to_owned(),
            ok: false,
            cancelled: false,
        };
        assert_eq!(ok.outcome(), JobOutcome::Success);
        assert_eq!(bad.outcome(), JobOutcome::Failure);
    }
}
```

- [ ] **Step 2: Run the new tests to verify they compile and fail-then-pass.** Since `outcome.rs` isn't wired into a module yet, first add `pub mod outcome;` to `crates/rexops-app/src/jobs/mod.rs` and the re-export:

```rust
pub mod outcome;
pub mod process;

pub use outcome::{JobLifecycle, JobOutcome, JobRecord, LastOutcome};
pub use process::{spawn, JobExit, JobHandle, JobOutput};
```

Run: `cargo test -p rexops-app jobs::outcome 2>&1 | grep "test result"`
Expected: 2 passed.

- [ ] **Step 3: Re-export from `crates/rexops-app/src/lib.rs`.** Add to the re-export block:

```rust
pub use jobs::{JobLifecycle, JobOutcome, JobRecord, LastOutcome};
```

- [ ] **Step 4: Remove the duplicate definitions from `crates/rexops-tui/src/jobs/manager.rs`.** Delete the local `LastOutcome` struct + its `impl` (lines defining `as_outcome`), and the local `JobRecord` struct. Replace the top imports so they come from the app layer:

```rust
//! App-owned background job state transitions, plus the TUI-side mapping of the
//! domain job outcome/lifecycle to suite_ui presentation types.

use super::{JobExit, JobOutput};
use crate::app::{App, Screen};
use crate::tools;
use rexops_app::{JobLifecycle, JobOutcome, JobRecord, LastOutcome};

pub(crate) const JOB_HISTORY_CAP: usize = 50;
pub(crate) const JOB_OUTPUT_CAP: usize = 1000;
```

- [ ] **Step 5: Rewrite `toast_for` to map the domain outcome → `suite_ui`.** Replace the existing `toast_for` with:

```rust
/// Map a finished outcome to the toast text + kind shown in the TUI. This is the
/// render-boundary translation from the domain `JobOutcome` to suite_ui.
pub(crate) fn toast_for(outcome: &LastOutcome) -> (String, suite_ui::ToastKind) {
    use suite_ui::ToastKind;
    let name = &outcome.name;
    match outcome.outcome() {
        JobOutcome::Success => (format!("{name} — done"), ToastKind::Success),
        JobOutcome::Failure => (format!("{name} — failed"), ToastKind::Failure),
        JobOutcome::Cancelled => (format!("{name} — cancelled"), ToastKind::Cancelled),
    }
}
```

- [ ] **Step 6: Rewrite `App::job_state` to build the domain lifecycle, then map to `suite_ui::JobState`.** Replace the existing `job_state` method body with a version that uses `JobOutcome`:

```rust
    pub fn job_state(&self) -> suite_ui::JobState<'_> {
        if let Some(job) = &self.job {
            return suite_ui::JobState::Running { name: &job.name };
        }
        match &self.last_outcome {
            Some(outcome) => match outcome.outcome() {
                JobOutcome::Cancelled => suite_ui::JobState::Cancelled {
                    name: &outcome.name,
                },
                JobOutcome::Success => suite_ui::JobState::Done {
                    name: &outcome.name,
                    ok: true,
                },
                JobOutcome::Failure => suite_ui::JobState::Done {
                    name: &outcome.name,
                    ok: false,
                },
            },
            None => suite_ui::JobState::Idle,
        }
    }
```

(The `JobLifecycle` enum is exported for any front-end that wants domain truth, but the TUI maps straight to `suite_ui::JobState` here. `JobLifecycle` is currently unused by the TUI — that's fine; it's part of the shared surface. If clippy flags it as unused across the workspace, keep it: it is `pub` API of `rexops-app` and exercised by `outcome.rs` tests via construction in a unit test — add a trivial construction test if needed to satisfy dead-code analysis. See Step 8.)

- [ ] **Step 7: Update `crates/rexops-tui/src/jobs/mod.rs`** so `JobRecord`/`LastOutcome` re-export from `rexops_app`, not `manager`:

```rust
//! Background job process and state management.
//!
//! The process model and the outcome/record types now live in `rexops_app`;
//! this module re-exports them and keeps the App-glue state transitions and the
//! suite_ui mapping in `manager`.

mod manager;

#[cfg(test)]
pub(crate) use manager::{toast_for, JOB_HISTORY_CAP, JOB_OUTPUT_CAP};
pub use rexops_app::{spawn, JobExit, JobHandle, JobOutput, JobRecord, LastOutcome};
```

- [ ] **Step 8: Guard `JobLifecycle` against dead-code lint.** Add a construction test to `crates/rexops-app/src/jobs/outcome.rs` tests module so the variants are exercised:

```rust
    #[test]
    fn job_lifecycle_variants_construct() {
        let _ = JobLifecycle::Idle;
        let _ = JobLifecycle::Running { name: "j".to_owned() };
        let _ = JobLifecycle::Done { name: "j".to_owned(), ok: true };
        let _ = JobLifecycle::Cancelled { name: "j".to_owned() };
    }
```

- [ ] **Step 9: Build and test**

Run: `cargo test --workspace 2>&1 | grep "test result"`
Expected: 174 + 3 (the 2 outcome tests + the lifecycle construction test) = **177 passed**, 0 failed. (The `app/tests/jobs.rs` suite — which exercises `start_job`/`poll_job`/outcomes — must still pass unchanged, proving the domain-enum swap is behavior-preserving.)

Run: `cargo clippy --workspace --all-targets 2>&1 | grep -E "^error|^warning" ; cargo fmt --check && echo FMT-CLEAN`
Expected: no errors/warnings; `FMT-CLEAN`. If clippy still flags `JobLifecycle` as never-read in non-test code, that is acceptable for `pub` API — but the construction test should satisfy it. If not, annotate the enum with a short doc and `#[allow(dead_code)]` is NOT permitted (it is real API); instead leave it and confirm clippy is clean with the test present.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "refactor(app): move job outcome records + add domain enums to rexops-app

LastOutcome/JobRecord move to rexops-app; new JobOutcome/JobLifecycle
domain enums replace the suite_ui Outcome coupling in job logic. The TUI
maps domain -> suite_ui at the render boundary (toast_for, job_state).
rexops-app stays free of any UI dependency.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Task 4: Move the tool launcher into rexops-app

`launcher.rs` depends only on `rexops_core::AppConfig` and the catalog (now in app). The `ForegroundRunner` trait + resolve/launch fns + `LaunchCommand`/`ChildExit`/`LaunchReport` all move. The `impl ForegroundRunner for Tui` is NOT here (it's in the TUI's `lib.rs` already) — confirm and keep it there.

**Files:**
- Create: `crates/rexops-app/src/tools/launcher.rs`
- Modify: `crates/rexops-app/src/tools/mod.rs`
- Modify: `crates/rexops-app/src/lib.rs`
- Delete: `crates/rexops-tui/src/tools/launcher.rs`
- Modify: `crates/rexops-tui/src/tools/mod.rs`

- [ ] **Step 1: Create `crates/rexops-app/src/tools/launcher.rs`** with the current TUI launcher contents, changing only the `use super::catalog;` line to `use super::catalog;` (it stays `super::catalog` because in `rexops-app` the launcher and catalog are siblings under `tools/`). Copy the file **including its full `#[cfg(test)] mod tests`** (FakeRunner, the disabled-adapter, config-over-PATH, and graceful-skip tests). Update the top doc comment:

```rust
//! launcher.rs — launch orchestration for specialist tools (shared logic).
//!
//! Decides *what* to launch and how to report the result. It does not own
//! terminal state; the caller supplies a `ForegroundRunner` that knows how to
//! suspend/restore its UI around a child process (the TUI implements it on its
//! terminal guard).

use std::io;
use std::process::Command;

use rexops_core::AppConfig;

use super::catalog;
```

The rest of the file (the trait, the three data types, `launch_tool`, `resolve_command`, `resolve_launch_command`, `command_from_path`, `command_from_config`, and the whole test module) is copied verbatim.

- [ ] **Step 2: Wire it into `crates/rexops-app/src/tools/mod.rs`:**

```rust
//! Tool catalog, run mode, and launch orchestration (shared business logic).

pub mod catalog;
pub mod launcher;

pub use catalog::{by_id, is_streamable, RunMode, ToolEntry, CATALOG};
pub use launcher::{
    launch_tool, resolve_command, resolve_launch_command, ChildExit, ForegroundRunner,
    LaunchCommand, LaunchReport,
};
```

- [ ] **Step 3: Re-export from `crates/rexops-app/src/lib.rs`.** Extend the tools re-export line:

```rust
pub use tools::{
    by_id, is_streamable, launch_tool, resolve_command, resolve_launch_command, ChildExit,
    ForegroundRunner, LaunchCommand, LaunchReport, RunMode, ToolEntry, CATALOG,
};
```

- [ ] **Step 4: Delete `crates/rexops-tui/src/tools/launcher.rs`.**

Run: `rm crates/rexops-tui/src/tools/launcher.rs`

- [ ] **Step 5: Re-point `crates/rexops-tui/src/tools/mod.rs`** to re-export the launcher surface from `rexops_app`, dropping the local `pub mod launcher;`:

```rust
//! Tool catalog, run mode, and launch orchestration.
//!
//! Both the catalog and the launch orchestration now live in `rexops_app`
//! (shared business logic). This module re-exports them so TUI call sites
//! (`crate::tools::…`) are unchanged. The terminal-touching `ForegroundRunner`
//! impl stays in the TUI (on its `Tui` guard).

pub use rexops_app::{
    is_streamable, launch_tool, resolve_launch_command, ChildExit, ForegroundRunner,
    LaunchCommand, ToolEntry, CATALOG,
};
```

- [ ] **Step 6: Confirm the `impl ForegroundRunner for Tui`** in `crates/rexops-tui/src/lib.rs` still compiles — it now implements `rexops_app::ForegroundRunner` (re-exported via `crate::tools::ForegroundRunner`). Check the `use` there points at `crate::tools::ForegroundRunner` or `rexops_app::ForegroundRunner`; either resolves to the same trait. No body change.

- [ ] **Step 7: Build and test**

Run: `cargo test --workspace 2>&1 | grep "test result"`
Expected: 177 passed, 0 failed. The 16 launcher tests now run under `rexops-app`. (PATH gotcha already handled — the moved tests pin off-PATH fake ids via config, carried verbatim.)

Run: `cargo clippy --workspace --all-targets 2>&1 | grep -E "^error|^warning" ; cargo fmt --check && echo FMT-CLEAN`
Expected: no errors/warnings; `FMT-CLEAN`.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor(app): move tool launcher from rexops-tui to rexops-app

ForegroundRunner trait + resolve/launch logic + LaunchCommand/ChildExit/
LaunchReport move to rexops-app (depends only on core AppConfig + catalog).
The terminal-touching impl ForegroundRunner for Tui stays in the TUI.
16 launcher tests move with it.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Task 5: Extract the snapshot-refresh driver into rexops-app

The background-thread refresh — spawn a thread, run `build_snapshot_with_piped` under `catch_unwind`, send the result over a channel, fall back to a panic-note snapshot — is pure app-layer orchestration. Extract it from `App` into `rexops-app`; `App::request_refresh` becomes a thin caller.

**Files:**
- Create: `crates/rexops-app/src/refresh.rs`
- Modify: `crates/rexops-app/src/lib.rs`
- Modify: `crates/rexops-tui/src/app/state.rs`

- [ ] **Step 1: Write the failing test** for the panic-note fallback. Create `crates/rexops-app/src/refresh.rs`:

```rust
//! refresh.rs — the background snapshot-refresh driver.
//!
//! Spawns a worker thread that builds an OpsSnapshot from the captured config +
//! piped stdin and sends it over a channel. A panicking adapter probe is caught
//! so a snapshot ALWAYS arrives (the worker never unwinds without sending) — the
//! fallback carries a note so the failure is visible rather than reading as an
//! empty "nothing probed yet" state. Long-lived front-ends drive their refresh
//! through this so the panic-safety and the consume-once stdin discipline live
//! in one shared place instead of being re-implemented per front-end.

use std::sync::mpsc::Sender;
use std::thread;

use rexops_core::OpsSnapshot;

use crate::build_snapshot_with_piped;
use rexops_core::AppConfig;

/// The fallback snapshot delivered when an adapter probe panics mid-refresh.
/// Empty (no probe data survived the unwind) but carries a note + the `panicked`
/// flag so the failure surfaces in the UI/log instead of looking identical to a
/// never-probed state — a silent crash is the worst outcome for an ops tool.
pub fn panicked_snapshot() -> OpsSnapshot {
    let mut snap = OpsSnapshot::new();
    snap.panicked = true;
    snap.add_note("refresh failed: an adapter probe panicked — partial/empty results");
    snap
}

/// Spawn a background refresh: build a snapshot from `config` + `piped` on a
/// worker thread and send it over `tx`. A panicking probe is caught and replaced
/// with [`panicked_snapshot`] so a result ALWAYS arrives — the caller's
/// "refreshing" flag (cleared on receipt) can never get stuck. `piped` is the
/// stdin captured ONCE at startup, cloned in by the caller; this function never
/// reads stdin (it is consume-once — see `build_snapshot`).
pub fn spawn_refresh(tx: Sender<OpsSnapshot>, config: AppConfig, piped: Option<String>) {
    thread::spawn(move || {
        let snapshot = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            build_snapshot_with_piped(&config, piped.as_deref())
        }))
        .unwrap_or_else(|_| panicked_snapshot());
        let _ = tx.send(snapshot);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panicked_snapshot_is_flagged_and_noted() {
        let snap = panicked_snapshot();
        assert!(snap.panicked, "fallback must set the panicked flag");
        assert!(
            snap.notes.iter().any(|n| n.contains("probe panicked")),
            "fallback must carry a visible note"
        );
    }

    #[test]
    fn spawn_refresh_delivers_a_snapshot_over_the_channel() {
        use std::sync::mpsc;
        use std::time::Duration;
        let (tx, rx) = mpsc::channel();
        // Disable the binary-probing adapters so this is deterministic in CI.
        let mut cfg = AppConfig::default();
        for name in ["bulwark", "system", "workstate"] {
            cfg.adapters.insert(
                name.to_owned(),
                rexops_core::AdapterConfig {
                    enabled: false,
                    ..Default::default()
                },
            );
        }
        spawn_refresh(tx, cfg, None);
        let snap = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("a snapshot must arrive");
        assert!(!snap.panicked, "a clean build must not be flagged panicked");
    }
}
```

- [ ] **Step 2: Wire the module into `crates/rexops-app/src/lib.rs`.** Add `mod refresh;` and to the re-export block:

```rust
pub use refresh::{panicked_snapshot, spawn_refresh};
```

- [ ] **Step 3: Run the new tests to verify they pass.**

Run: `cargo test -p rexops-app refresh 2>&1 | grep "test result"`
Expected: 2 passed.

- [ ] **Step 4: Rewrite `App::request_refresh`** in `crates/rexops-tui/src/app/state.rs` to delegate. Replace the whole `request_refresh` body (the inline thread-spawn + catch_unwind) with:

```rust
    pub fn request_refresh(&mut self) {
        if self.refreshing {
            return;
        }
        self.refreshing = true;
        self.log_event("Refresh requested (background thread)");
        // The thread-spawn, panic-guard, and consume-once stdin discipline live
        // in rexops_app::spawn_refresh (shared with any future front-end). We
        // clone the captured stdin so this refresh routes the same bytes as
        // every other — never re-reading the consume-once pipe (see the field
        // doc on `piped_stdin`).
        rexops_app::spawn_refresh(self.tx.clone(), self.config.clone(), self.piped_stdin.clone());
    }
```

- [ ] **Step 5: Remove `App::panicked_snapshot`** from `crates/rexops-tui/src/app/state.rs` (the method defined around the old lines 261-266) — it now lives in `rexops_app`. Search for any other caller of `Self::panicked_snapshot()` / `App::panicked_snapshot()` in the TUI:

Run: `grep -rn "panicked_snapshot" crates/rexops-tui/src`
Expected after edit: no references in `rexops-tui` (the only caller was inside the old `request_refresh`, now gone). If a test referenced it, update that test to call `rexops_app::panicked_snapshot()`.

- [ ] **Step 6: Build and test**

Run: `cargo test --workspace 2>&1 | grep "test result"`
Expected: 177 + 2 (the refresh tests) = **179 passed**, 0 failed. The `app/tests/refresh.rs` suite (6 tests) must still pass, proving `request_refresh`/`apply_snapshot` behavior is preserved.

Run: `cargo clippy --workspace --all-targets 2>&1 | grep -E "^error|^warning" ; cargo fmt --check && echo FMT-CLEAN`
Expected: no errors/warnings; `FMT-CLEAN`.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor(app): extract snapshot-refresh driver into rexops-app

spawn_refresh + panicked_snapshot move out of App into rexops-app, where
the panic-guard and consume-once stdin discipline are shared. App::
request_refresh is now a thin caller. Refresh behavior unchanged.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Task 6: Tidy, verify, and document

Final cleanup: prune now-unused imports, confirm the crate-root lint headers, update the architecture doc, smoke-run both binaries.

**Files:**
- Modify: `crates/rexops-app/src/lib.rs` (doc comment: app is now the business-logic layer)
- Modify: `docs/ARCHITECTURE.md`
- Modify: any TUI file with a now-unused import (clippy will name them)

- [ ] **Step 1: Let clippy find dead imports.**

Run: `cargo clippy --workspace --all-targets --fix --allow-dirty 2>&1 | tail -20`
Then: `cargo clippy --workspace --all-targets 2>&1 | grep -E "^error|^warning"`
Expected: clean. Manually review the `--fix` diff (`git diff`) before keeping it — confirm it only removed unused imports, nothing functional.

- [ ] **Step 2: Update `crates/rexops-app/src/lib.rs` module doc** to reflect the expanded responsibility. Change the "Responsibilities" list to add:

```rust
//! - Owns the executable business logic shared by the front-ends: the tool
//!   catalog, the job process model + outcome records, the tool launcher
//!   (behind a ForegroundRunner the front-end implements), and the background
//!   snapshot-refresh driver. The front-ends contribute only rendering, input,
//!   and glue over this layer.
```

Keep the "No Ratatui, no terminal IO, no TUI state" rule line — it is still true and now load-bearing (it's why `suite_ui` stayed out).

- [ ] **Step 3: Update `docs/ARCHITECTURE.md`.** Read it first (`Read crates`… actually `docs/ARCHITECTURE.md`), then update the crate-responsibility section so `rexops-app` is described as the shared business-logic layer (catalog, jobs, launcher, refresh) and `rexops-tui` as rendering + input + glue. Add a one-line note that `suite_ui` is a TUI-only dependency and `rexops-app` must never depend on it. Match the doc's existing prose style.

- [ ] **Step 4: Verify the `suite_ui` boundary held.**

Run: `grep -rn "suite" crates/rexops-app/Cargo.toml crates/rexops-app/src`
Expected: **no matches** — `rexops-app` is free of any `suite_ui`/`suite-ui` reference.

- [ ] **Step 5: Verify the core crate is untouched.**

Run: `git diff --stat main -- crates/rexops-core`
Expected: no files changed under `rexops-core`.

- [ ] **Step 6: Full gate sweep.**

Run: `cargo fmt --check && echo FMT-CLEAN`
Run: `cargo clippy --workspace --all-targets 2>&1 | grep -E "^error|^warning" || echo CLIPPY-CLEAN`
Run: `cargo test --workspace 2>&1 | grep "test result"`
Expected: `FMT-CLEAN`; `CLIPPY-CLEAN`; all `ok`, totalling **179 passed**, 0 failed.

- [ ] **Step 7: Smoke-run both binaries.**

Run: `cargo run -p rexops-cli -- status --json | head -5`
Expected: valid JSON snapshot output, exit 0.
Run: `cargo run -p rexops-cli -- adapters | head -5`
Expected: the adapters listing, exit 0.
Run: `echo q | cargo run -p rexops-tui 2>&1 | tail -3 ; echo "tui-exit=$?"`
Expected: the TUI starts and quits cleanly (or note: a TTY may be required; if it errors on no-TTY, that is the pre-existing behavior, not a regression — confirm by checking `git stash` of the branch behaves identically). The reliable check is that it compiles and the 118 TUI tests pass.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor: tidy imports, document core/TUI separation

Prune dead imports, update rexops-app + ARCHITECTURE.md docs to reflect
app as the shared business-logic layer and TUI as rendering+glue. Verify
the suite_ui boundary (app stays UI-free) and that rexops-core is untouched.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:**
- "Job process model → app" → Task 2. ✓
- "Tool launcher → app (ForegroundRunner trait moves, impl for Tui stays)" → Task 4. ✓
- "Tool catalog → app" → Task 1. ✓
- "Refresh driver → app (`spawn_refresh`)" → Task 5. ✓
- "`suite_ui` enum decoupling via domain enums" → Task 3. ✓
- "App struct stays as-is, methods become thin callers" → Tasks 3 (job_state/toast_for adapt), 5 (request_refresh thin). App field set untouched. ✓
- "rexops-core untouched" → verified in Task 6 Step 5. ✓
- "rexops-app no suite-ui dep" → verified in Task 6 Step 4. ✓
- "All tests stay green; tests migrate with code" → every task ends with the suite; counts tracked (174 → 179 as app-side tests are added). ✓
- "Each step independently committable/reversible" → one commit per task. ✓

**Type consistency:**
- Domain classifier method named `outcome()` (was `as_outcome()` against `suite_ui::Outcome`); used consistently in `toast_for` and `job_state` (Task 3). The old `as_outcome` is removed.
- `JobOutcome { Success, Failure, Cancelled }` and `JobLifecycle { Idle, Running, Done, Cancelled }` defined once (Task 3 Step 1), referenced consistently.
- `spawn_refresh(tx, config, piped)` signature defined in Task 5 Step 1, called identically in Task 5 Step 4.
- `LaunchReport` is re-exported from app in Task 4 (it was not in the TUI's `tools::mod` re-export list before, but is `pub` in launcher; exporting it is additive and harmless).

**Placeholder scan:** no TBD/TODO/"handle edge cases"/"similar to Task N"; every code step shows full code; every command shows expected output.

**Note on test totals:** the workspace total rises from 174 to 179 because Task 3 adds 3 app tests and Task 5 adds 2. No existing test is deleted — process/launcher tests *move* between crates (the per-crate split shifts; the moved-test totals are conserved). If a moved test count looks off, check it landed in its new crate rather than being dropped.
