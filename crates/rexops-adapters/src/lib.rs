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
//!
//! Everything else (exec, the private probe helpers) is `pub(crate)` or private.

mod adapter;
mod bulwark;
mod error;
mod exec;
mod types;

// Re-export the public API in a flat, convenient way.
// Callers should be able to `use rexops_adapters::{BulwarkAdapter, AdapterError};`
pub use adapter::Adapter;
pub use bulwark::{
    BulwarkAction, BulwarkAdapter, BulwarkCategory, BulwarkFinding, BulwarkLocation,
    BulwarkScanResult, BulwarkSeverity,
};
pub use error::AdapterError;
pub use types::{AdapterHealth, AdapterOutput};

// NOTE TO FUTURE EDITORS:
// Do NOT add any functions, constants, or re-exports that contain logic in this
// file. lib.rs is intentionally a directory of contents only. All behavior
// lives in the modules above. This makes the crate easy to audit and keeps
// the root under 50 lines forever.
