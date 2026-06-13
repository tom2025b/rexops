//! Background job state management.
//!
//! The state machine, process model, and outcome/history data types now live in
//! `rexops_app` (`JobManager` + friends). This module re-exports the types the
//! TUI render path needs and keeps the App-glue transitions and render-boundary
//! outcome mapping in `manager`.

mod manager;

pub(crate) use manager::to_suite_outcome;
pub(crate) use rexops_app::{JobOutput, JobRecord};

// Test-only: the data/process helpers and caps the job tests drive directly.
#[cfg(test)]
pub(crate) use manager::toast_for;
#[cfg(test)]
pub(crate) use rexops_app::{spawn, LastOutcome, JOB_HISTORY_CAP, JOB_OUTPUT_CAP};
