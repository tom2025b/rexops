#![deny(clippy::unwrap_used, clippy::expect_used)]
#![warn(clippy::all, clippy::pedantic)]
// A few allows for a small orchestration crate (similar to the other crates).
#![allow(
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::unnecessary_map_or
)]

//! rexops-app — thin shared application / orchestration layer.
//!
//! Responsibilities:
//! - Single source for loading AppConfig (search paths + fallback to defaults).
//! - Single source for building OpsSnapshot from a config (probes enabled adapters).
//! - Single source for building AdapterRegistry (used by `rexops adapters`).
//!
//! Architectural rules (enforced):
//! - No Ratatui, no terminal IO, no TUI state.
//! - No long-lived services or direct mutation of external systems.
//! - Depends on rexops-core (pure data) + rexops-adapters (thin probes) only.
//! - Re-exports the two main fns so callers write `use rexops_app::{load_config, build_snapshot};`
//! - Keep files small. No god modules.
//!
//! Callers:
//! - rexops-cli uses load_config + build_snapshot + build_adapter_registry.
//!   It is one-shot, so `build_snapshot` reading stdin inline is correct.
//! - rexops-tui uses load_config + read_piped_stdin (once, at startup) +
//!   build_snapshot_with_piped (inside each refresh thread, fed the captured
//!   stdin). It must never read stdin per refresh — stdin is consume-once.

mod config;
mod snapshot;
pub mod tools;

// Re-export the primary public surface in a flat namespace.
pub use config::load_config;
pub use snapshot::{
    build_adapter_registry, build_snapshot, build_snapshot_with_piped, read_piped_stdin,
};
// The tool catalog (shared with the front-ends). `pub mod tools` keeps the
// submodule path reachable too; these flat re-exports cover the common names.
pub use tools::{by_id, is_streamable, RunMode, ToolEntry, CATALOG};

// Re-export a few core types that callers often need alongside the builders
// so they don't have to add an extra direct dependency on rexops-core just for
// the type names in signatures (convenience, not required).
pub use rexops_core::{AppConfig, OpsSnapshot};

// Keep lib.rs as the directory of contents only. Real behavior lives in
// config.rs and snapshot.rs.
