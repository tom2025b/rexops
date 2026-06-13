//! Background job process and state management.
//!
//! The outcome/history *data* types (`JobRecord`, `LastOutcome`) now live in
//! `rexops_app`; this module re-exports them so TUI call sites are unchanged.
//! The process plumbing (`process`) and the App-glue transitions + render-
//! boundary outcome mapping (`manager`) stay here.

mod manager;
pub mod process;

pub(crate) use manager::to_suite_outcome;
#[cfg(test)]
pub(crate) use manager::{toast_for, JOB_HISTORY_CAP, JOB_OUTPUT_CAP};
pub use process::{spawn, JobExit, JobHandle, JobOutput};
pub use rexops_app::{JobRecord, LastOutcome};
