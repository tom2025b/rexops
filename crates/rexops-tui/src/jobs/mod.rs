//! Background job process and state management.

mod manager;
pub mod process;

#[cfg(test)]
pub(crate) use manager::{toast_for, JOB_HISTORY_CAP, JOB_OUTPUT_CAP};
pub use manager::{JobRecord, LastOutcome};
pub use process::{spawn, JobExit, JobHandle, JobOutput};
