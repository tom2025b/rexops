# RexOps Error Handling

## Philosophy
- Errors are typed per crate for precise handling.
- Excellent user messages + suggested fixes (actionable, not cryptic).
- Graceful degradation: missing optional tool → Unavailable health + clear banner, never crash.
- thiserror in libraries for rich Display + source.
- anyhow only at binary entrypoints if needed (e.g. CLI main for top-level reporting).
- TUI shows degraded state + banners; never lets an adapter error take down the whole UI.

## Per-Crate Error Types

- **rexops-adapters**:
  - `AdapterError`.
  - Variants: `BinaryNotFound`, `CommandFailed`, `JsonParse`, `Timeout`, `Io`.
  - BinaryNotFound is *not* fatal — it maps to health Unavailable.

- **rexops-core**:
  - `CoreError`.
  - Covers invalid ids and config validation errors.

- **rexops-cli**:
  - Uses clap for parse errors and returns process exit codes from command handling.
  - Human output and JSON both come from the shared app snapshot builder.

- **rexops-tui**:
  - Uses terminal/event-loop errors at the binary boundary.
  - Adapter and config problems are rendered as degraded state, unavailable health, or log messages.

## Recommended Patterns

1. Never use `?` blindly across crate boundaries without mapping.
   - In CLI: `map_err(|e| CliError::Core(e.into()))` or similar.
2. At binary boundaries, convert failures into clear stderr messages and explicit exit codes.
3. For TUI, prefer `Result<T, ...>` from services but render `match health { Unavailable => "⚠ Adapter unavailable — ...", ... }`.
4. Timeouts, missing tools, bad config, permission issues: explicit variants + suggestions.
5. Tests: cover missing binaries, bad JSON, malformed config, and graceful skips for unsupported snapshot versions.

## User-Facing Messages (Examples to Aim For)

- "Binary 'bulwark' not found on PATH — install the tool or disable the 'bulwark' adapter in config."
- "Config error: adapters.bulwark.binary must be a non-empty string (got empty)."
- "Scan timed out after 30s. The tool may be busy or the input too large. Try with --timeout 60 or reduce payload size."
- "Tools reports degraded health. Some data may be stale or unavailable."

## Implementation Notes

- Put error types in `error.rs` per crate, like adapters and core do.
- Re-export the crate's primary error from lib.rs.
- Use `#[source]` / `#[from]` liberally for chaining.
- When lifting `AdapterOutput`, preserve the original health/error context instead of swallowing it.

See ARCHITECTURE.md for how errors flow into OpsSnapshot (health is part of snapshot, errors are surfaced separately for commands).

Keep this in sync with actual crate behavior.
