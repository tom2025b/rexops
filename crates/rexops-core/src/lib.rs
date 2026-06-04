#![deny(clippy::unwrap_used, clippy::expect_used)]
#![warn(clippy::all, clippy::pedantic)]
// Allow a few pedantic lints that are noisy for small data crates with
// typed errors, newtypes, and constructors. Spirit of pedantic is honored.
#![allow(
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::unnecessary_map_or,
    clippy::module_name_repetitions
)]

//! rexops-core — single source of truth for domain models, state, and config.
//!
//! This crate owns *all* shared types used by CLI, TUI, and future app layers.
//! It contains zero execution logic, zero UI, and zero knowledge of how to spawn
//! external processes. It only defines data, newtypes, transformations, and
//! validation that can be tested in complete isolation.
//!
//! Architectural guarantees (enforced here and in dependents):
//! - No God files. Every .rs stays well under 300 lines (ideally < 200).
//! - Every fallible public function returns `Result<T, CoreError>`.
//! - Zero `unwrap()` / `expect()` in non-test code (denied at crate root).
//! - Pure data + pure functions. Serde for (de)serialization of snapshots/config.
//! - Depends on rexops-adapters only to reuse AdapterHealth/AdapterOutput and
//!   lift them into higher-level OpsSnapshot without duplication.
//!
//! Public surface (re-exports for convenience):
//! - CoreError — the only error type exported from this crate.
//! - Newtypes: AdapterId, ToolId.
//! - Health: AdapterHealth (re-exported for callers; ToolHealth may evolve here later).
//! - Config: AppConfig + supporting types.
//! - Snapshot: OpsSnapshot (now includes optional SystemInfo, ScriptVaultInfo, ToolFoundryInfo), RiskSummary, ReportSummary, JobStatus.
//! - Registries: AdapterRegistry, ToolRegistry (data containers only).
//!
//! Everything else lives in focused modules. lib.rs is a table of contents only.

// Module declarations — order is declaration order, not importance.
mod config;
mod error;
mod ids;
mod models;
mod registry;

// Re-export the primary public API in a flat namespace so callers can write:
//   use rexops_core::{AppConfig, OpsSnapshot, AdapterId, CoreError};
pub use config::{AdapterConfig, AppConfig, Defaults};
pub use error::CoreError;
pub use ids::{AdapterId, ToolId};
pub use models::{JobStatus, OpsSnapshot, ReportSummary, RiskSummary};
pub use registry::{AdapterEntry, AdapterRegistry, ToolEntry, ToolRegistry};

// Re-export key adapter types so the rest of RexOps does not have to depend
// directly on rexops-adapters everywhere (reduces coupling at call sites).
pub use rexops_adapters::{
    AdapterHealth, AdapterOutput, BulwarkScanInfo, ScriptVaultInfo, SystemInfo, ToolFoundryInfo,
};

// NOTE TO FUTURE EDITORS:
// Do NOT add behavior, constructors with side effects, or rendering code here.
// lib.rs must stay a pure directory of contents (< 60 lines forever).
// All logic and data definitions live in the modules listed above.
