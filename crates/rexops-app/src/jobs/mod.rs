//! Background-job outcome and history records (shared business logic).
//!
//! Only the *data* types live here — how a finished job is summarised and kept
//! in history. The process plumbing (`JobHandle`, `spawn`, …) and the App-glue
//! state transitions stay in the TUI for now. These types carry no presentation
//! meaning (no colour/toast); a front-end maps [`JobOutcome`] to its own UI
//! vocabulary at the render boundary, which is what keeps rexops-app free of any
//! UI dependency.

pub mod outcome;

pub use outcome::{JobOutcome, JobRecord, LastOutcome};
