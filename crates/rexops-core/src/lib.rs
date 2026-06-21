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
//! This crate owns all shared types used by CLI, TUI, and the app layer.
//! It contains zero execution logic, zero UI, and zero knowledge of how to spawn
//! external processes. It only defines data, newtypes, transformations, and
//! validation that can be tested in complete isolation.
//!
//! Architectural guarantees (enforced here and in dependents):
//! - No God files. Every .rs stays well under 300 lines (ideally < 200).
//! - Every fallible public function returns `Result<T, CoreError>`.
//! - Zero `unwrap()` / `expect()` in non-test code (denied at crate root).
//! - Pure data + pure functions. Serde for (de)serialization of snapshots/config.
//! - Does NOT depend on rexops-adapters (execution layer). The dependency flows
//!   the correct way: adapters → core, not core → adapters.

// Module declarations — order is declaration order, not importance.
mod adapter_models;
mod adapter_types;
mod component;
mod component_table;
mod config;
mod error;
mod ids;
mod models;
mod registry;
mod system_info;
mod workstate_info;

// Re-export the primary public API in a flat namespace so callers can write:
//   use rexops_core::{AppConfig, OpsSnapshot, AdapterId, CoreError};
pub use adapter_models::findings::{FindingsInfo, RiskTally, ScanItem, Severity};
pub use adapter_models::scripts::{Script, ScriptsInfo};
pub use adapter_models::tools::{Tool, ToolsInfo};
pub use adapter_types::{AdapterHealth, AdapterOutput};
pub use component::{
    Component, ComponentGroup, ComponentId, FeedSpec, HealthSource, LaunchSpec, Maturity, RunMode,
};
pub use component_table::{component_by_id, COMPONENTS};
pub use config::{AdapterConfig, AppConfig, Defaults};
pub use error::CoreError;
pub use ids::{AdapterId, ToolId};
pub use models::{format_unix_millis_utc, JobStatus, OpsSnapshot, ReportSummary, RiskSummary};
pub use registry::{AdapterEntry, AdapterRegistry, ToolEntry, ToolRegistry};
pub use system_info::SystemInfo;
pub use workstate_info::{status_to_freshness, Freshness, Provenance, Section, WorkstateInfo};

// lib.rs stays as a directory of contents. Logic and data definitions live in
// the modules listed above.
