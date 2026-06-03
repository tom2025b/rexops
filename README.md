# RexOps

Rust-based operational tooling and governance adapters for AI agents and infrastructure.

RexOps is the **ops cockpit** for your AI tooling surface: it observes, summarizes, inventories, and safely invokes the specialist tools (Bulwark, ScriptVault, ToolFoundry) without duplicating them.

## What RexOps Is

- A thin orchestration + observability layer.
- Read-only by default (Phase 1); later adds safe, confirmed mutating workflows.
- Single pane of glass for health, risk, inventory, and reports across adapters.
- Strict modular Rust workspace: tiny crates with hard boundaries.
- Built for keyboard-first TUI + scriptable CLI + JSON output.
- Graceful degradation: missing optional tools never crash the system.

## What RexOps Is Not

- Not a replacement for Bulwark (content inspection), ScriptVault (script management), or ToolFoundry (tool lifecycle).
- Not a general-purpose task runner or CI system.
- Not a web dashboard (TUI + CLI first).
- Not "everything in one binary" — composition via small focused crates.

## Relations to Specialist Tools

| Tool         | Role in the Ecosystem                  | How RexOps Uses It                  |
|--------------|----------------------------------------|-------------------------------------|
| Bulwark      | Content inspection / policy engine     | BulwarkAdapter: `inspect scan` for findings, risk summary |
| ScriptVault  | Script storage, favorites, recents     | ScriptVaultAdapter (stub + Scripts screen on 4): metadata, favorites, recents (demo data) |
| ToolFoundry  | Tool ownership, symlinks, health, lifecycle | ToolFoundryAdapter (stub + Tools screen on 5): inventory + per-tool health + symlinks (demo data) |

RexOps **orchestrates and summarizes**; the specialists do the real work.

## Workspace Structure & Status

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full diagram and strict boundaries.

**Current status:** Phase 1 foundation complete (4 adapters, core models, shared rexops-app layer, thin CLI, full 5-screen TUI). All changes pass the 4 quality gates.

Crates:

- **rexops-adapters** — (foundation, production-ready) Synchronous `Adapter` trait + `BulwarkAdapter` (real) + `SystemAdapter` + `ScriptVaultAdapter` + `ToolFoundryAdapter` (demo). Outputs `AdapterOutput<T>`. Graceful degradation. System/ScriptVault/ToolFoundry are lightweight (demo data, no hard external binary dep for the stubs).
- **rexops-core** — Domain models, newtypes (`ToolId`, `AdapterId`), `RiskSummary`, `OpsSnapshot`, `AppConfig`, `AdapterRegistry`/`ToolRegistry`. Pure data + transforms. Single source of truth. No UI, no exec. See `crates/rexops-core/src/`.
- **rexops-app** — Shared thin orchestration layer. `load_config()`, `build_snapshot()`, `build_adapter_registry()`. The single implementation (deduplicated from CLI+TUI). No UI. See `crates/rexops-app/`.
- **rexops-cli** — `rexops` binary with `status` and `adapters` commands, `--json` support. Thin shell: clap + formatting only. Delegates to rexops-app. Try: `cargo run -p rexops-cli -- status --json`.
- **rexops-tui** — Keyboard-first ratatui TUI. 5 screens: Dashboard (1), Adapters (2, with live filter), System (3), Scripts (4, ★ favorites), Tools (5, ownership/symlinks). Widgets/ extracted (HealthBadge, AdapterItem, LogLine). Logs/events pane, help popup. 'r' non-blocking (threads call rexops-app::build_snapshot). See `crates/rexops-tui/` (incl. `widgets/`) and `docs/TUI_DESIGN.md`. Run with: `cargo run -p rexops-tui`

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [docs/ROADMAP.md](docs/ROADMAP.md) for boundaries and what's next.

## Quality Gates (Non-Negotiable)

All changes must keep the gate green:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo build --all
```

Additional rules (see adapters for reference style):
- Files stay well under 300 lines (prefer <200).
- Every fallible public function returns `Result<T, CrateError>`.
- Zero `unwrap()` / `expect()` in non-test library code (`#![deny]`).
- Tests written alongside implementation (fixture-based parsers, exhaustive error paths).
- `cargo test --all` is the gate.

## Getting Started

```bash
git clone https://github.com/tom2025b/rexops.git
cd rexops

# Full quality gate (must stay green after any change)
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo build --all

# Try the CLI
cargo run -p rexops-cli -- status
cargo run -p rexops-cli -- status --json
cargo run -p rexops-cli -- adapters

# Launch the TUI (best in a real terminal)
cargo run -p rexops-tui
# Inside TUI: 1-5 to switch screens, r=refresh, ?=help, q=quit, j/k nav on adapters, type to filter
```

## Key Commands

- `cargo test -p rexops-adapters` — Run only adapter tests (fixture-based Bulwark parsing).
- `cargo run -p rexops-cli -- status` — Human status (adapter health + snapshot).
- `cargo run -p rexops-cli -- status --json` — Same as JSON (for scripts / TUI later).
- `cargo run -p rexops-cli -- adapters` — List adapters from the registry.
- `cargo run -p rexops-tui` — Launch the ratatui dashboard (keyboard-first). Keys: r=refresh (non-blocking), q/Esc/Ctrl-C=quit, ?=help (popup overlay), 1=Dashboard, 2=Adapters (navigable list+detail with j/k/enter + live type-to-filter), 3=System (structured info: hostname/kernel/uptime/disk + health), 4=Scripts (structured script list with ★ favorites from ScriptVault), 5=Tools (structured tool list with owner/health/symlink from ToolFoundry stub). Status bar adapts per screen. See docs/TUI_DESIGN.md. Works best in a real terminal.
- The four gate commands (`fmt --check`, `clippy -D warnings`, `test --all`, `build --all`) are mandatory for every change.

## Development Notes

- See `crates/rexops-adapters/` for the reference implementation of style (small modules, private exec, exhaustive errors, educational comments, Learning Notes at bottom of files).
- The fixture `crates/rexops-adapters/fixtures/bulwark/scan_sample.json` is PROVISIONAL (hand-authored; update with real `bulwark inspect scan --format json` output when the binary is available).
- New crates follow the same discipline: educational comments, small files, tests for happy + error paths.
- Config sample: see `examples/config.yaml`.
- Full error strategy: `docs/ERROR_HANDLING.md`.
- Roadmap and phase status: `docs/ROADMAP.md`.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

The workspace `Cargo.toml` declares `license = "MIT OR Apache-2.0"`.

