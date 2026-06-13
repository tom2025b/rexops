//! Tool catalog and run-mode metadata (shared business logic).
//!
//! The catalog moved here from rexops-tui so both front-ends consume one
//! source of truth for the known toolset. Launch orchestration stays in the
//! TUI for now; only the static catalog lives here.

pub mod catalog;

pub use catalog::{by_id, is_streamable, RunMode, ToolEntry, CATALOG};
