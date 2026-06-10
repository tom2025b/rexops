# RexOps Architecture

This document is the single source of truth for RexOps workspace layout, crate boundaries, and data flow. It is intentionally kept concise.

## Workspace Layout

```
rexops/
├── Cargo.toml (workspace root)
├── crates/
│   ├── rexops-adapters/          # UNTOUCHED foundation (current)
│   ├── rexops-core/              # domain models, central OpsSnapshot, registries, health, AppConfig
│   ├── rexops-app/               # shared orchestration (load_config, build_snapshot, build_adapter_registry)
│   ├── rexops-cli/               # thin shell: clap, dispatch, human/JSON formatting
│   └── rexops-tui/               # ratatui TUI: screens, widgets, keyboard, event loop only
├── docs/
│   ├── ARCHITECTURE.md
│   ├── ROADMAP.md
│   ├── ERROR_HANDLING.md
│   └── TUI_DESIGN.md
├── examples/
│   └── config.yaml               # Sample AppConfig
├── README.md
```

## Crate Responsibilities (Strict Boundaries)

- **rexops-adapters**:
  - Thin integration layer only.
  - Adapter trait + concrete impls for Bulwark, System, and Workstate.
  - Outputs `AdapterOutput<T>`.
  - Graceful degradation for missing bins, timeouts, parse errors.
  - No domain models from core. No UI. No exec outside exec.rs (private).

- **rexops-core**:
  - Single source of truth for all shared domain types.
  - Newtypes: `ToolId`, `AdapterId`.
  - Models: `ToolHealth`, `RiskSummary`, `JobStatus`, `ReportSummary`, `OpsSnapshot`, `AppConfig`.
  - Registries: `ToolRegistry`, `AdapterRegistry`.
  - Pure data + transformations. Serde + thiserror.
  - NO ratatui, NO `std::process`, NO direct CLI rendering, NO TUI widgets.
  - Does not depend on adapter execution or UI crates.

- **rexops-cli**:
  - Argument parsing (clap), command dispatch.
  - Formatting (human vs JSON).
  - Calls rexops-app for load_config / build_snapshot / registry. Tiny `main.rs`.
  - No domain logic, no config loading, no adapter construction.

- **rexops-tui**:
  - Ratatui app shell only.
  - Screens for Dashboard, Adapters, System, Scripts, Tools, and Launcher; widgets, keymap, event loop.
  - Never owns domain logic; calls services from rexops-app.
  - **Renders through the shared `suite-ui` crate** (theme, panes, status bar, key hints, overlays) — see "Shared UI layer" below. It owns screen *composition* and RexOps-specific view models, not the suite's common chrome.
  - Fast startup, keyboard-first, graceful degradation, excellent empty/error states.

- **rexops-app**:
  - Thin shared orchestration layer (the "app services").
  - `load_config()` — one implementation of the documented search + fallback.
  - `build_snapshot(config)` — the single place that probes enabled adapters and assembles `OpsSnapshot` (including system plus Workstate scripts/tools/findings data).
  - `build_adapter_registry(config)` — used by the CLI `adapters` subcommand.
  - No UI, no mutation, no new domain rules. CLI and TUI are now trivial shells calling these.

## Shared UI layer: suite-ui

RexOps is part of a family of tools (RexOps, ScriptVault, and others) that
historically shared **only data formats, never code**. That rule still governs
the **logic/domain layer** — each tool's adapters, snapshots, and orchestration
stay decoupled. It deliberately **no longer applies to the presentation layer**:
the suite's common terminal-UI chrome lives in one crate, `suite-ui`, and RexOps
**imports and renders through it** instead of carrying its own copy.

The split, stated precisely:

- **Logic / domain layer — still decoupled.** Everything in `rexops-core`,
  `rexops-adapters`, and `rexops-app` is RexOps' own. `suite-ui` owns **zero**
  domain types: every widget takes a `Theme`, a borrowed data slice, and a
  `Rect`, and draws. It draws the box; RexOps owns the behaviour.
- **UI / presentation layer — shared, by import.** The theme/palette (cyan/amber
  accent + the single `NO_COLOR` gate), the rounded `pane`, and the shared
  widgets/overlays come from `suite-ui`: `StatusBar` (the footer job segment),
  `KeyHints` (per-screen footer shortcuts), `SearchBar`, `Toast`, `PaletteFrame`,
  `HelpSheet`, `ConfirmModal`, the `Health`/`Outcome` styling, and the
  `keys` bindings. `rexops-tui` has no local theme or chrome code.

**Dependency.** `rexops-tui` depends on `suite-ui` by **path**
(`../../../linux-ops-suite/crates/suite-ui`), the suite-wide convention: the
repos sit side-by-side under `~/projects`, so edits to `suite-ui` are picked up
instantly without a commit/rev-bump cycle. The trade-off — a consumer can't build
in isolation without the sibling repo present — is handled in CI by checking out
`linux-ops-suite` next to RexOps before the build (see `.github/workflows/ci.yml`,
the `rust` job). ScriptVault, the other early adopter, uses the same path-dep +
sibling-checkout pattern.

**What RexOps keeps local — and why.** Only what is genuinely its own: the seven
`screens/*` (screen composition), the RexOps view-model widgets
(`widgets/health_badge.rs`, `adapter_item.rs`, `log_line.rs`), and three small
**extension points** that translate RexOps' domain onto the shared chrome —
`health::to_suite` (`AdapterHealth` → `suite_ui::Health`), `App::as_outcome`
(a finished job's result → `suite_ui::Outcome`), and `App::job_state` (the live
job → `suite_ui::JobState`). These are the seams that let the shared, NO_COLOR-safe
widgets render RexOps state without `suite-ui` ever learning a RexOps type.

Several shared widgets (`Health`, the `StatusBar` cancelled state, the `Toast`
job-lifecycle kinds) were generalized *from* RexOps into `suite-ui`, then adopted
back — which is why RexOps' usage is the most complete of any tool in the suite.

## Central Source of Truth

- All shared domain types live in `rexops-core`.
- `OpsSnapshot` aggregates adapter health + tool state + reports.
- TUI uses **derived view models only** (never raw core types directly in widgets).
- Avoid duplication: adapters return normalized `AdapterOutput<T>`; core lifts/transforms into snapshots.
- Config is loaded in core (or app), validated once, passed down.

## Data Flow

1. rexops-app loads config and probes enabled adapters.
2. Workstate snapshots populate scripts, tools, and findings; live probes populate adapter health and system facts.
3. Core models hold the normalized `OpsSnapshot`.
4. CLI and TUI render the snapshot or adapter registry without owning adapter logic.

## Quality Gates (Non-Negotiable)

Every change must keep these green:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo build --all
```

- Files stay small (<300 LOC ideal; reference adapters style).
- Zero `unwrap`/`expect` in non-test library code (enforced by `#![deny]`).
- Every fallible public fn returns `Result<T, CrateSpecificError>`.
- Tests alongside impl (fixture-based for parsers, error paths).

## Active Adapters

- `WorkstateAdapter` — **implemented**; the single source of truth for scripts/tools/findings.
- Lightweight `SystemAdapter` (read-only ps/df/uptime/etc.) — **implemented**.
- `BulwarkAdapter` — **implemented** as a live optional probe.
- All degrade gracefully; adapters are optional by design.

See ERROR_HANDLING.md for error strategy. See ROADMAP.md for timeline.
