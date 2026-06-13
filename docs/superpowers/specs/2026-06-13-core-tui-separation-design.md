# Core / TUI Separation — Design Spec

**Date:** 2026-06-13
**Branch:** `refactor/core-separation`
**Status:** Approved — proceeding to implementation plan.

## Goal

Properly separate the core business logic from the TUI layer. The executable
business logic that currently lives in `rexops-tui` (job orchestration, the tool
launcher, the static tool catalog, and the snapshot-refresh driver) moves into
the shared `rexops-app` crate so both front-ends (CLI and TUI) consume one
source of truth, and `rexops-tui` shrinks to rendering + input + glue.

## The key architectural finding

The natural phrasing is "move the core logic into `rexops-core`." That is the
wrong destination, and the reason matters:

- `rexops-core` is contractually **pure data**: serde-only, no I/O, no process
  execution, and it does **not** depend on `rexops-adapters`. This is enforced
  at the crate root (`#![deny(clippy::unwrap_used, clippy::expect_used)]`, plus
  the documented "dependency flows adapters → core, not core → adapters" rule).
- The logic we are extracting — `jobs/process.rs` spawns child processes;
  `tools/launcher.rs` spawns processes; the refresh driver reads stdin and runs
  background threads — **is I/O and execution.** It cannot live in `rexops-core`
  without destroying the pure-data guarantee that makes core trivially testable.

So the executable business logic lands in **`rexops-app`**, which already owns
`build_snapshot` and is the crate that is *allowed* side effects. The separation
is therefore **two-layered**, not one:

| Layer | Crate | Rule |
|---|---|---|
| Pure data & state types | `rexops-core` | no I/O, no UI (unchanged) |
| Executable business logic | `rexops-app` | orchestration, process spawning, refresh — shared by CLI + TUI |
| Terminal rendering & input | `rexops-tui` | ratatui / crossterm / suite-ui only |

The win: `rexops-app` graduates from "thin snapshot builder" into the genuine
shared business-logic layer; the TUI becomes a presentation client; and the CLI
gains access to the job/launch logic for free should it ever want it.

## Scope decision

We **move the relocatable logic and keep the TUI `App` struct as-is.** App's
field set and its view/domain interleaving are left untouched. App methods
(`start_job`, `confirm_pending`, `request_refresh`, etc.) become thin callers
into the relocated `rexops-app` functions. A full split of App into a pure
domain-state struct vs. a view-state struct is explicitly **out of scope** and
recorded as a follow-up — it would touch every screen renderer and rework all
118 TUI tests for a larger, riskier change with no behavior gain right now.

## What MOVES (rexops-tui → rexops-app)

Four units that are already renderer-agnostic. Each is `suite_ui`-free or
trivially decoupled (see "MIXED" below).

1. **Job process model** — `jobs/process.rs` (~532 LOC).
   `JobHandle`, `JobOutput`, `JobExit`, `spawn`, `drain_into`, `poll_done`,
   `cancel`, and the `Drop` impl. Pure `std::process` + `std::sync::mpsc` +
   `std::thread` supervision with **zero UI imports**. Moves essentially
   verbatim, with its unit tests.

2. **Tool launcher** — `tools/launcher.rs` (~358 LOC).
   `resolve_command`, `resolve_launch_command`, `launch_tool`, and the
   `LaunchCommand` / `ChildExit` / `LaunchReport` data types. The terminal
   concern is already abstracted behind the **`ForegroundRunner` trait** — the
   trait definition moves to `rexops-app`; the `impl ForegroundRunner for Tui`
   (the only terminal-touching part) **stays in `rexops-tui`**.

3. **Tool catalog** — `tools/catalog.rs` (~69 LOC).
   `RunMode`, `ToolEntry`, `CATALOG`, `by_id`, `is_streamable`. Pure static data.

4. **Refresh driver** — currently inlined in `app/state.rs` as
   `App::request_refresh` (spawn a background thread → run
   `build_snapshot_with_piped` under `catch_unwind` → send the result over an
   `mpsc` channel). Extract the thread-spawn + panic-guard into an app-level
   helper, e.g. `spawn_refresh(config, piped, tx)`, living next to
   `build_snapshot`. `App::request_refresh` becomes a thin caller.

## MIXED — the `suite_ui` enum decoupling (the only real friction)

`jobs/manager.rs` and the job-related state on `App` map job results to three
**presentation enums** that live in the git-dependency `suite_ui`:
`Outcome { Success, Failure, Cancelled }`, `ToastKind`, and
`JobState { Idle, Running, Done, Cancelled }`.

`rexops-app` must **not** depend on `suite_ui` (a UI crate). To move the job
*logic* cleanly, introduce plain **domain enums in `rexops-app`**:

- `JobOutcome { Success, Failure, Cancelled }`
- `JobLifecycle { Idle, Running, Done, Cancelled }`

The job model in app speaks these domain enums. The TUI maps domain → `suite_ui`
at the **render boundary only** (a small `From`/match in the screens or a tiny
adapter in `manager.rs`). `ToastKind` is purely a TUI concern and stays in the
TUI — the relocated logic returns a domain outcome and the TUI decides the toast.
This severs the only thing blocking the job move; it is small and mechanical.

## What STAYS in rexops-tui (genuinely UI)

- `lib.rs` (terminal lifecycle, panic hook, `run()` entry, `impl ForegroundRunner for Tui`).
- `runtime.rs` `run()` — the draw/poll event loop. (`step()` already takes the
  event as a parameter and stays testable.)
- All of `screens/` and `ui/` (rendering).
- `input/keymap.rs` (crossterm key decoding) and `input/action.rs` (the `Action`
  enum is the TUI's own input vocabulary).
- `app/` — the `App` struct, `update.rs` (`on_action`), `navigation.rs`,
  `commands/dispatch.rs`, `commands/palette.rs`. These orchestrate *over* `App`
  and view state; their methods become thin callers into relocated app fns.
- `jobs/manager.rs` — stays, but adapts to the app-layer job model + domain
  enums (it is the App↔job-model glue, not the job model itself).

## What stays in rexops-core

Everything. The pure-data layer is **untouched** by this refactor.

## Dependency graph (after)

```
core  (pure data, serde only)
  ▲
  ├── adapters (only I/O for probes)  ──▶ core
  │
app  (business logic: snapshot + jobs + launcher + catalog + refresh)  ──▶ core, adapters
  ▲
  ├── cli  ──▶ core, app, tui
  └── tui  ──▶ core, app, suite-ui   (rendering + input + glue)
```

No new crate. No new external dependency in `rexops-app` (it stays serde + the
two internal crates; the job/launcher code is `std`-only). `suite-ui` remains a
`rexops-tui`-only dependency.

## Order of execution

Each step compiles, keeps all tests green (baseline: **174** — 7 app + 49
core/adapters + 118 tui), and stays clippy + fmt clean. Each is an independent,
reversible commit. Tests migrate alongside the code they cover.

1. **Catalog → app.** Lowest risk (pure data, no `suite_ui`). Re-export from the
   TUI's `tools` module so `screens/` keep compiling unchanged. Proves the
   move-and-re-export pipeline.
2. **Job process model (`process.rs`) → app.** Self-contained, zero UI. Move its
   unit tests with it. TUI `jobs` module re-exports the types.
3. **Domain enums + `manager.rs` decoupling.** Add `JobOutcome` / `JobLifecycle`
   in app; map domain → `suite_ui` at the TUI render boundary. Unblocks the job
   logic from the UI crate.
4. **Launcher (`launcher.rs`) → app.** Move the `ForegroundRunner` trait + the
   resolve/launch fns + data types; keep `impl ForegroundRunner for Tui` in the
   TUI. Move the 16 launcher tests (pin off-PATH fake ids per the known PATH
   gotcha).
5. **Refresh driver → app.** Extract `spawn_refresh`; `App::request_refresh`
   calls it. Keep the `catch_unwind` panic-guard semantics intact.
6. **Tidy & verify.** Update `docs/ARCHITECTURE.md`; re-confirm the `#![deny]` /
   `#![warn]` headers on `rexops-app`; full `cargo fmt --check`,
   `cargo clippy --workspace`, `cargo test --workspace`; smoke-run both the
   `rexops` (CLI/TUI) and `rexops-tui` binaries.

## Risks & mitigations

- **Re-export churn:** moving a type can break many `use` sites in `screens/`.
  *Mitigation:* re-export moved types from their old TUI module path so call
  sites are unchanged within a step; clean up imports only in the final tidy.
- **PATH test fragility (known):** launcher tests must pin an off-PATH fake id
  via config, or a real `which <tool>` hit on the dev box wins. Carry that
  convention into the moved tests.
- **`suite_ui` leakage:** the guard is simple — `rexops-app/Cargo.toml` must
  never gain a `suite-ui` dep. If a move wants it, that signals a render concern
  that should stay in the TUI.
- **Scope creep into App decomposition:** explicitly deferred. If a step tempts
  a wider App rewrite, stop and keep the thin-caller shape.

## Success criteria

- Job model, launcher, catalog, and refresh-driver live in `rexops-app`.
- `rexops-app` has **no** `suite-ui` dependency; `rexops-core` is unchanged.
- All 174 tests pass; clippy + fmt clean; both binaries run.
- `rexops-tui` contains only rendering, input, and thin glue over the app layer.

## Out of scope (follow-ups)

- Splitting `App` into a pure domain-state struct vs. a view-state struct.
- Any new mutating actions or feature work.
- Touching `rexops-core` internals.
