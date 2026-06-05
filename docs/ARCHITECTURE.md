# RexOps Architecture

This document is the single source of truth for RexOps workspace layout, crate boundaries, and data flow. It is intentionally kept concise.

## Workspace Layout (Current + Planned)

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
│   └── TUI_DESIGN.md (future)
├── examples/
│   └── config.yaml               # Sample AppConfig
├── README.md
└── (future: tests/, benches/ at workspace root if needed)
```

## Crate Responsibilities (Strict Boundaries)

- **rexops-adapters** (existing, do not touch in Phase 1+ without explicit reason):
  - Thin integration layer only.
  - Adapter trait + concrete impls (Bulwark first).
  - Outputs `AdapterOutput<T>`.
  - Graceful degradation for missing bins, timeouts, parse errors.
  - No domain models from core. No UI. No exec outside exec.rs (private).

- **rexops-core** (Phase 1 target):
  - Single source of truth for all shared domain types.
  - Newtypes: `ToolId`, `AdapterId`.
  - Models: `ToolHealth`, `RiskSummary`, `JobStatus`, `ReportSummary`, `OpsSnapshot`, `AppConfig`.
  - Registries: `ToolRegistry`, `AdapterRegistry` (orchestration via app layer later).
  - Pure data + transformations. Serde + thiserror.
  - NO ratatui, NO `std::process`, NO direct CLI rendering, NO TUI widgets.
  - Depends on rexops-adapters for lifting `AdapterOutput` into snapshots (thin dep).

- **rexops-cli**:
  - Argument parsing (clap), command dispatch.
  - Formatting (human vs JSON).
  - Calls rexops-app for load_config / build_snapshot / registry. Tiny `main.rs`.
  - No domain logic, no config loading, no adapter construction.

- **rexops-tui** (much later):
  - Ratatui app shell only.
  - Screens (Dashboard, Adapters/Status, Tools/Inventory, Detail), widgets, keymap, event loop.
  - Never owns domain logic — calls services from core/app.
  - Fast startup, keyboard-first, graceful degradation, excellent empty/error states.

- **rexops-app**:
  - Thin shared orchestration layer (the "app services").
  - `load_config()` — one implementation of the documented search + fallback.
  - `build_snapshot(config)` — the single place that probes enabled adapters and assembles `OpsSnapshot` (including system plus Workstate scripts/tools/findings data).
  - `build_adapter_registry(config)` — used by the CLI `adapters` subcommand.
  - No UI, no mutation, no new domain rules. CLI and TUI are now trivial shells calling these.

## Central Source of Truth

- All shared domain types live in `rexops-core`.
- `OpsSnapshot` aggregates adapter health + tool state + reports.
- TUI uses **derived view models only** (never raw core types directly in widgets).
- Avoid duplication: adapters return normalized `AdapterOutput<T>`; core lifts/transforms into snapshots.
- Config is loaded in core (or app), validated once, passed down.

## Data Flow (Read-Only Phase)

1. Adapters (e.g. BulwarkAdapter) run external CLIs → `AdapterOutput<Concrete>`.
2. Core consumes adapter outputs + any persisted state → `OpsSnapshot`.
3. CLI / TUI query snapshot or registries for read views.
4. Later: app layer owns snapshot refresh, caching, scheduling.

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
