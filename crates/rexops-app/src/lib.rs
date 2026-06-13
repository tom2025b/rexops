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
//! - Shared tool catalog plus launch resolution/reporting.
//! - Shared background refresh and job process orchestration.
//!
//! Architectural rules (enforced):
//! - No Ratatui, crossterm, suite_ui, terminal state, or TUI state.
//! - Terminal handoff stays behind the front-end supplied ForegroundRunner trait.
//! - Depends on rexops-core (pure data) + rexops-adapters (thin probes) only.
//! - Keep files small. No god modules.
//!
//! Callers:
//! - rexops-cli uses load_config + build_snapshot + build_adapter_registry.
//!   It is one-shot, so `build_snapshot` reading stdin inline is correct.
//! - rexops-tui uses load_config + read_piped_stdin (once, at startup) +
//!   spawn_refresh, the shared job runner, and the shared tool launcher. It
//!   owns rendering, input, terminal suspension, and suite_ui mapping.

mod config;
pub mod jobs;
mod refresh;
mod snapshot;
pub mod tools;

// Config, snapshots, and refresh orchestration.
pub use config::load_config;
pub use refresh::{panicked_snapshot, spawn_refresh, RefreshController};
pub use rexops_core::{AppConfig, OpsSnapshot};
pub use snapshot::{
    build_adapter_registry, build_snapshot, build_snapshot_with_piped, read_piped_stdin,
};

// The tool catalog and launcher (shared with the front-ends). `pub mod tools`
// keeps the submodule path reachable too; these flat re-exports cover the
// common names.
pub use tools::{
    by_id, is_streamable, launch_tool, resolve_command, resolve_launch_command, Availability,
    AvailabilityTag, ChildExit, ForegroundRunner, LaunchCommand, LaunchReport, RunMode, ToolEntry,
    CATALOG,
};

// Background-job state machine plus its process and outcome/history data types
// (shared with the front-ends). The front-end maps JobOutcome / JobLifecycle to
// its own UI types and reacts to the manager's result values; rexops-app stays
// UI-free.
pub use jobs::{
    spawn, FinishedJob, JobExit, JobHandle, JobLifecycle, JobManager, JobOutcome, JobOutput,
    JobRecord, LastOutcome, PollOutcome, StartOutcome, JOB_HISTORY_CAP, JOB_OUTPUT_CAP,
};

// Keep lib.rs as the directory of contents only. Real behavior lives in
// focused modules under this crate.
