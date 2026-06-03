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
//! Responsibilities (per the Phase 1 architecture plan):
//! - Single source for loading AppConfig (search paths + fallback to defaults).
//! - Single source for building OpsSnapshot from a config (probes enabled adapters).
//! - Single source for building AdapterRegistry (used by `rexops adapters`).
//!
//! This crate is intentionally *optional* for early development. CLI and TUI
//! were allowed to duplicate a little glue while core stabilized. Once core was
//! solid we extracted the duplication here.
//!
//! Architectural rules (enforced):
//! - No Ratatui, no terminal IO, no TUI state.
//! - No long-lived services or mutation of external systems (yet).
//! - Depends on rexops-core (pure data) + rexops-adapters (thin probes) only.
//! - Re-exports the two main fns so callers write `use rexops_app::{load_config, build_snapshot};`
//! - Keep files small. No god modules.
//!
//! Callers:
//! - rexops-cli uses load_config + build_snapshot + build_adapter_registry.
//! - rexops-tui uses load_config + build_snapshot (inside refresh threads).
//!
//! Future (when we grow):
//! - Workflows, dry-run hooks, job queueing, report persistence, etc. can live
//!   here without touching the thin front-ends or the pure core.

mod config;
mod snapshot;

// Re-export the primary public surface in a flat namespace.
pub use config::load_config;
pub use snapshot::{build_adapter_registry, build_snapshot};

// Re-export a few core types that callers often need alongside the builders
// so they don't have to add an extra direct dependency on rexops-core just for
// the type names in signatures (convenience, not required).
pub use rexops_core::{AppConfig, OpsSnapshot};

// NOTE TO FUTURE EDITORS:
// lib.rs is a directory of contents only. All real behavior is in config.rs
// and snapshot.rs. This file must stay tiny (< 60 lines).

// Learning Notes:
// - This is the classic "facade" or "application service" layer in a clean
//   architecture: it knows which adapters to call and how to assemble the
//   snapshot, but contains zero UI and zero business rules (risk math lives in
//   core::RiskSummary, adapter parsing lives in adapters).
// - By putting the builders here we made the "duplication of load_config/
//   build_snapshot only as temporary skeleton" problem go away exactly as the
//   plan described.
// - Because we re-export AppConfig/OpsSnapshot, a caller can do
//   `use rexops_app::{load_config, build_snapshot, AppConfig};` if they like.
//   They can still depend on rexops-core directly for other types (AdapterId,
//   etc.) — both are fine.
