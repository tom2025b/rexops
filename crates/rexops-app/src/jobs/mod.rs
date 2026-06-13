//! Background job process model, outcome classification, and history records.
//!
//! These types carry no presentation meaning; a front-end maps [`JobOutcome`] to
//! its own UI vocabulary at the render boundary.

pub mod manager;
pub mod outcome;
pub mod process;

pub use manager::{
    FinishedJob, JobManager, PollOutcome, StartOutcome, JOB_HISTORY_CAP, JOB_OUTPUT_CAP,
};
pub use outcome::{JobLifecycle, JobOutcome, JobRecord, LastOutcome};
pub use process::{spawn, JobExit, JobHandle, JobOutput};
