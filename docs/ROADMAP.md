# RexOps Roadmap

Concise status and remaining work for the RexOps ops cockpit.

## Guiding Principles
- Keep It Simple.
- Respect the excellent adapters foundation — never duplicate its concerns.
- Keep CLI, TUI, app, core, and adapters on their current boundaries.
- All changes pass the 4 quality gates (fmt/clippy/test/build).
- Graceful degradation everywhere; optional components never crash the system.

## Completed Foundation
- [x] rexops-adapters complete (BulwarkAdapter + SystemAdapter + WorkstateAdapter + trait + types + error + exec; fixture tests).
- [x] Expand docs + examples (ARCHITECTURE.md, ROADMAP.md, ERROR_HANDLING.md, examples/config.yaml, updated README, TUI_DESIGN.md).
- [x] rexops-core: domain models, newtypes, OpsSnapshot (with system/scripts/tools/findings/workstate), AppConfig, registries (pure data).
- [x] rexops-cli (minimal): inspection commands (status, adapters), --json/--human, thin dispatch over core+adapters.
- [x] rexops-tui shell + modular screens (Dashboard, Adapters, System, Scripts, Tools on '5').
- [x] All changes pass 4 gates; crate-level boundaries + graceful enabled flags.
- [x] examples/config.yaml matches AppConfig + documents the active adapters.

## Current Orchestration
- rexops-app: snapshot builder, adapter registry, config loading. CLI and TUI are thin shells calling it.
- Workstate snapshot consumer — done; scripts/tools/findings come from Workstate only.
- SystemAdapter (lightweight read-only) — done.
- TUI launcher uses confirmation before starting external tools.

## Remaining Work
- Comprehensive error-path coverage + integration tests (mock adapters).
- Benchmarks if hot paths emerge.
- Packaging (cargo install, optional binary releases).
- Full docs (crate READMEs, man pages or --help quality).
- Relations docs for how RexOps consumes Workstate and live probes.

## Out of Scope
- Full async runtime in adapters or core (keep sync until proven needed).
- Direct mutation without confirmation + audit trail.
- Web UI or remote agent mode.
- Heavy dependencies outside crates that directly need them.

## North Star
RexOps is the single pane of glass: live health of your AI tooling surface, inventory of tools, risk summaries, and safe invocation surface, with Workstate as the compiled state source.

Update this file when completed work or priorities shift. Keep dates minimal; focus on current facts.
