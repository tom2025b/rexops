# RexOps Error Handling

## Philosophy
- Errors are typed per crate for precise handling.
- Excellent user messages + suggested fixes (actionable, not cryptic).
- Graceful degradation: missing optional tool → Unavailable health + clear banner, never crash.
- thiserror in libraries for rich Display + source.
- anyhow only at binary entrypoints if needed (e.g. CLI main for top-level reporting).
- TUI shows degraded state + banners; never lets an adapter error take down the whole UI.

## Per-Crate Error Types (Planned)

- **rexops-adapters** (existing):
  - `AdapterError` (keep exactly as-is).
  - Variants: `BinaryNotFound`, `CommandFailed`, `JsonParse`, `Timeout`, `Io`.
  - BinaryNotFound is *not* fatal — it maps to health Unavailable.

- **rexops-core**:
  - `CoreError`.
  - Covers: config load/parse/validate errors, registry lookup failures, snapshot construction invariants, type conversion/lift errors from adapters, timeout wrappers if core adds higher-level timing.
  - Example good message: "failed to load config from ~/.config/rexops/config.yaml: missing required field 'adapters'".

- **rexops-cli**:
  - `CliError`.
  - Clap errors, command dispatch failures, formatting/output errors, wrapping of CoreError/AdapterError with context like "while running 'rexops status'".
  - Human-friendly: suggest "install bulwark via cargo install bulwark-inspect or disable in config".

- **rexops-tui**:
  - `TuiError` (or use core + adapter errors directly + render-specific).
  - Event loop, rendering, input handling errors.
  - Most adapter/core errors are caught at screen level and turned into "Degraded: ..." banners + detail in logs pane.

## Recommended Patterns

1. Never use `?` blindly across crate boundaries without mapping.
   - In CLI: `map_err(|e| CliError::Core(e.into()))` or similar.
2. At binary boundary (main), use `anyhow::Result` + `context` for the last mile, then print nicely.
3. For TUI, prefer `Result<T, ...>` from services but render `match health { Unavailable => "⚠ Adapter unavailable — ...", ... }`.
4. Timeouts, missing tools, bad config, permission issues: explicit variants + suggestions.
5. Tests: every error path has a unit test (missing bin, bad JSON, permission denied simulation via fake binary, malformed config).

## User-Facing Messages (Examples to Aim For)

- "Binary 'bulwark' not found on PATH — install via `cargo install bulwark-inspect` or disable the 'bulwark' adapter in config."
- "Config error: adapters.bulwark.binary must be a non-empty string (got empty)."
- "Scan timed out after 30s. The tool may be busy or the input too large. Try with --timeout 60 or reduce payload size."
- "Tools reports degraded health (version 0.9). Some features (symlink repair) will be unavailable."

## Implementation Notes

- Put error types in `error.rs` (or `errors.rs`) per crate, like adapters does.
- Re-export the crate's primary error from lib.rs.
- Use `#[source]` / `#[from]` liberally for chaining.
- In core, when lifting `AdapterOutput`, preserve the original health/error context instead of swallowing it.

See ARCHITECTURE.md for how errors flow into OpsSnapshot (health is part of snapshot, errors are surfaced separately for commands).

Keep this in sync with actual code as crates are added.
