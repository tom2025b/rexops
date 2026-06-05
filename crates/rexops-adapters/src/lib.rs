#![deny(clippy::unwrap_used, clippy::expect_used)]
#![warn(clippy::all, clippy::pedantic)]
// Allow a few pedantic lints that are noisy for a small foundation crate with
// typed errors, constructors, and prose documentation. The spirit of pedantic
// is still honored (we have no unwraps, we have tests, etc.).
#![allow(
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::unnecessary_map_or
)]

//! rexops-adapters — thin, read-only integration layer for external tools.
//!
//! This crate exists so that the rest of RexOps (core, executor, TUI, CLI)
//! never has to know how to spawn `bulwark`, parse its JSON, or handle the
//! "binary not installed" case. All of that lives here, behind a tiny,
//! strongly-typed surface.
//!
//! Non-negotiable architectural rules (enforced in this crate):
//! - No God files. Every .rs stays well under 300 lines (ideally < 200).
//! - Every fallible public function returns `Result<T, AdapterError>`.
//! - Zero `unwrap()` / `expect()` in non-test code (denied at the crate root).
//! - No Tokio, no async, no execution/mutation logic in phase 1.
//! - The only thing that ever calls std::process::Command is inside exec.rs.
//!
//! Public surface (re-exports):
//! - AdapterError — the only error type you should ever see from this crate.
//! - Adapter, AdapterHealth, AdapterOutput — the common vocabulary.
//! - BulwarkAdapter + Bulwark* types — the first concrete adapter.
//! - SystemAdapter + SystemInfo — lightweight always-available system info (second adapter).
//! - ScriptVaultAdapter + ScriptVaultInfo/Script — read-only consumer of the ScriptVault export feed (third adapter; provisional contract; reads in-memory text or standard path).
//! - ToolFoundryAdapter + ToolFoundryInfo/Tool — read-only consumer of the ToolFoundry rexops-feed contract (fourth adapter, real).
//! - BulwarkFeedAdapter + BulwarkScanInfo/ScanItem — read-only consumer of the Bulwark scan export feed (fifth adapter; provisional contract; reads in-memory text or standard path).
//! - WorkstateAdapter + WorkstateInfo/Project — read-only consumer of the Workstate snapshot feed (sixth adapter; provisional contract; per-project repo health; reads in-memory text or standard path).
//!
//! Everything else (exec, the private probe helpers) is `pub(crate)` or private.

mod adapter;
mod bulwark;
mod bulwark_feed;
mod error;
mod exec;
mod scriptvault;
mod system;
mod toolfoundry;
mod types;
mod workstate;

// Re-export the public API in a flat, convenient way.
// Callers should be able to `use rexops_adapters::{BulwarkAdapter, AdapterError};`
pub use adapter::Adapter;
pub use bulwark::{
    BulwarkAction, BulwarkAdapter, BulwarkCategory, BulwarkFinding, BulwarkLocation,
    BulwarkScanResult, BulwarkSeverity,
};
pub use bulwark_feed::{BulwarkFeedAdapter, BulwarkScanInfo, RiskTally, ScanItem, Severity};
pub use error::AdapterError;
pub use scriptvault::{Script, ScriptVaultAdapter, ScriptVaultInfo};
pub use system::{SystemAdapter, SystemInfo};
pub use toolfoundry::{Tool, ToolFoundryAdapter, ToolFoundryInfo};
pub use types::{AdapterHealth, AdapterOutput};
pub use workstate::{status_to_health, Provenance, Section, WorkstateAdapter, WorkstateInfo};

// NOTE TO FUTURE EDITORS:
// Do NOT add any functions, constants, or re-exports that contain logic in this
// file. lib.rs is intentionally a directory of contents only. All behavior
// lives in the modules above. This makes the crate easy to audit and keeps
// the root under 50 lines forever.
