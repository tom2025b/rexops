# RexOps Roadmap

Concise, incremental plan. We ship the "ops cockpit" that orchestrates/summarizes without duplicating Bulwark, ScriptVault, or ToolFoundry.

## Guiding Principles
- Keep It Simple.
- Respect the excellent adapters foundation — never duplicate its concerns.
- Incremental: docs first, then core, then cli, then tui. Add next adapter only after core stable.
- All changes pass the 4 quality gates (fmt/clippy/test/build).
- Graceful degradation everywhere; optional components never crash the system.

## Phase 1 — Foundation (Current)
- [x] rexops-adapters complete (BulwarkAdapter + SystemAdapter + ScriptVaultAdapter + ToolFoundryAdapter + trait + types + error + exec; fixture tests).
- [x] Expand docs + examples (ARCHITECTURE.md, ROADMAP.md, ERROR_HANDLING.md, examples/config.yaml, updated README, TUI_DESIGN.md).
- [x] rexops-core: domain models, newtypes, OpsSnapshot (with system/scriptvault/toolfoundry), AppConfig, registries (pure data).
- [x] rexops-cli (minimal): inspection commands (status, adapters), --json/--human, thin dispatch over core+adapters.
- [x] rexops-tui shell + modular screens (Dashboard, Adapters, System, Scripts, Tools on '5').
- [x] All changes pass 4 gates; crate-level boundaries + graceful enabled flags.
- [x] examples/config.yaml matches AppConfig + documents the 4 adapters.

## Phase 2 — TUI Shell (When Foundation Solid)
- rexops-tui crate. (Started — see docs/TUI_DESIGN.md)
- Basic ratatui shell + event loop (main.rs + app.rs + ui.rs).
- Dashboard screen showing live OpsSnapshot: color-coded adapter health table, risk summary, messages/notes, status bar.
- Non-blocking refresh: 'r' spawns a std thread that probes adapters and sends the result back via mpsc so the UI stays responsive.
- Keyboard: q/Esc/Ctrl-C quit (always), r refresh, ?/h help.
- Excellent degraded/empty states + clean terminal restore (panic hook + explicit restore).
- Widgets, keymap, layout, status bar, banners.
- Later in phase: more screens, search/filter, detail panes, auto-tick refreshes.

## Phase 3 — Orchestration & Next Adapters
- (Optional) rexops-app: snapshot builder, adapter registry, workflows, config loading, dry-run hooks. — **implemented** (deduped load_config + build_snapshot + build_adapter_registry; CLI and TUI now thin shells calling it).
- ScriptVaultAdapter (read-only) — done (stub with favorites/recents).
- ToolFoundryAdapter (ownership/lifecycle/health/symlinks) — done (stub with demo tools).
- SystemAdapter (lightweight read-only) — done.
- First mutating (or confirmation-wrapped) operations behind explicit flags. (future)

## Phase 4 — Polish, Testing, Distribution
- Comprehensive error-path coverage + integration tests (mock adapters).
- Benchmarks if hot paths emerge.
- Packaging (cargo install, optional binary releases).
- Full docs (crate READMEs, man pages or --help quality).
- Relations docs for how RexOps sits alongside the three tools it orchestrates.

## Out of Scope (for now)
- Full async runtime in adapters or core (keep sync until proven needed).
- Direct mutation without confirmation + audit trail.
- Web UI or remote agent mode.
- Heavy dependencies (tokio, ratatui, etc.) until the calling crate actually needs them.

## North Star
RexOps is the single pane of glass: live health of your AI tooling surface, inventory of tools, risk summaries, and safe invocation surface — while Bulwark/ScriptVault/ToolFoundry remain the specialists.

Update this file when phases complete or priorities shift. Keep dates minimal; focus on completed items.
