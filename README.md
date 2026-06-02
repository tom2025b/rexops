# RexOps

Rust-based operational tooling and governance adapters for AI agents and infrastructure.

## What it does

- Provides the `rexops-adapters` crate: a thin, strictly-typed, read-only integration layer for external CLI tools.
- Starts with `BulwarkAdapter`, which invokes `bulwark inspect scan --format json` (or equivalent) and returns `AdapterOutput<BulwarkScanResult>` with full health/version context.
- Enforces strong architectural guarantees everywhere: files stay well under 300 lines (prefer <200), every fallible function returns `Result<T, AdapterError>`, zero `unwrap()`/`expect()` in non-test code, tests written alongside implementation, and the four `cargo` commands must always pass cleanly.
- Designed as the stable foundation for the rest of the RexOps workspace (future core, executor, CLI, TUI crates, plus adapters for ToolFoundry, ScriptVault, and system tools).

## Current crates

- **rexops-adapters** — Synchronous `Adapter` trait + `BulwarkAdapter` implementation. Graceful degradation for missing binaries, command failures, timeouts, and JSON parse errors. All vectors use `#[serde(default)]`.

This is early-stage work. The adapters layer is complete and production-ready as a foundation; higher-level crates are planned next.

## Getting started

```bash
git clone https://github.com/tom2025b/rexops.git
cd rexops

# Full quality gate (must stay green)
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo build --all
```

## Key commands

- `cargo test -p rexops-adapters` — Run only adapter tests (including fixture-based Bulwark result parsing).
- The four commands above are the non-negotiable gate for any change.

## Development notes

See `crates/rexops-adapters/` for the reference implementation of the required style (small focused modules, private `exec` helper, exhaustive error types, etc.). The fixture at `fixtures/bulwark/scan_sample.json` is marked PROVISIONAL because the real `bulwark` binary was not available during initial development.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

The workspace `Cargo.toml` declares `license = "MIT OR Apache-2.0"`.
