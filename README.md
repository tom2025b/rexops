# RexOps

Rust-based operational tooling and governance adapters for AI agents and infrastructure.

RexOps is the **ops cockpit** for your AI tooling surface: it observes live adapter health and renders Workstate's compiled scripts/tools/findings snapshot.

## What RexOps Is

- A thin orchestration + observability layer.
- Read-only by default (Phase 1); later adds safe, confirmed mutating workflows.
- Single pane of glass for health, risk, inventory, and reports across adapters.
- Strict modular Rust workspace: tiny crates with hard boundaries.
- Built for keyboard-first TUI + scriptable CLI + JSON output.
- Graceful degradation: missing optional tools never crash the system.

## What RexOps Is Not

- Not a replacement for specialist tools or Workstate's state compiler.
- Not a general-purpose task runner or CI system.
- Not a web dashboard (TUI + CLI first).
- Not "everything in one binary" — composition via small focused crates.

## Data Sources

| Source | Role | How RexOps Uses It |
|--------|------|--------------------|
| Workstate | Compiled source of truth | Scripts, tools, findings, section freshness |
| Bulwark | Live adapter probe | Optional binary health/version probe |
| System | Local host facts | Hostname, kernel, uptime, disk |

RexOps **orchestrates and summarizes**; Workstate owns the compiled operational state.

## Workspace Structure & Status

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full diagram and strict boundaries.

**Current status:** Workstate is the single source of truth for scripts/tools/findings. CLI and TUI consume the shared rexops-app snapshot builder.

Crates:

- **rexops-adapters** — Synchronous `Adapter` trait + `BulwarkAdapter`, `SystemAdapter`, and `WorkstateAdapter`. Outputs `AdapterOutput<T>`. Graceful degradation for optional binaries/snapshots.
- **rexops-core** — Domain models, newtypes (`ToolId`, `AdapterId`), `RiskSummary`, `OpsSnapshot`, `AppConfig`, `AdapterRegistry`/`ToolRegistry`. Pure data + transforms. Single source of truth. No UI, no exec. See `crates/rexops-core/src/`.
- **rexops-app** — Shared thin orchestration layer. `load_config()`, `build_snapshot()`, `build_adapter_registry()`. The single implementation (deduplicated from CLI+TUI). No UI. See `crates/rexops-app/`.
- **rexops-cli** — `rexops` binary with `status` and `adapters` commands, `--json` support. Thin shell: clap + formatting only. Delegates to rexops-app. Try: `cargo run -p rexops-cli -- status --json`.
- **rexops-tui** — Keyboard-first ratatui TUI. Screens: Dashboard, Adapters, System, Scripts, Tools, and Launcher. Scripts/tools/findings render from Workstate. Widgets/ extracted (HealthBadge, AdapterItem, LogLine). Logs/events pane, help popup. 'r' non-blocking (threads call rexops-app::build_snapshot). Run with: `cargo run -p rexops-tui`

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
- `cargo run -p rexops-tui` — Launch the ratatui dashboard. Keys: r=refresh, q/Esc/Ctrl-C=quit, ?=help, 1=Dashboard, 2=Adapters, 3=System, 4=Scripts, 5=Tools, 6=Launcher.
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
