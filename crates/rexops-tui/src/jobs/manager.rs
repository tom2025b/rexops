//! TUI-side job glue: the UI reactions to the rexops-app [`JobManager`]'s state
//! transitions, plus the render-boundary mapping of domain job types
//! (`JobOutcome`, `JobLifecycle`) to suite_ui presentation types.
//!
//! The job state machine — the one job slot, output buffer, scrollback, history,
//! and the pure transitions over them — now lives in `rexops_app::JobManager`.
//! This file keeps only what is genuinely front-end: switching to the Jobs
//! screen, flashing a toast, writing the activity log, requesting a refresh, and
//! translating domain → UI.

use crate::app::{App, Screen};
use rexops_app::{JobOutcome, LastOutcome, StartOutcome};

/// Map the domain [`JobOutcome`] to the suite_ui presentation enum. This is the
/// render-boundary translation: rexops-app speaks domain outcomes; the TUI turns
/// them into the suite-wide `Outcome` (which knows glyphs/colours). Kept here as
/// a `pub(crate)` free fn because the orphan rule forbids a `From` impl between
/// two types both foreign to this crate.
pub(crate) fn to_suite_outcome(outcome: JobOutcome) -> suite_ui::Outcome {
    match outcome {
        JobOutcome::Success => suite_ui::Outcome::Success,
        JobOutcome::Failure => suite_ui::Outcome::Failure,
        JobOutcome::Cancelled => suite_ui::Outcome::Cancelled,
    }
}

pub(crate) fn toast_for(outcome: &LastOutcome) -> (String, suite_ui::ToastKind) {
    use suite_ui::{Outcome, ToastKind};
    let name = &outcome.name;
    match to_suite_outcome(outcome.outcome()) {
        Outcome::Success => (format!("{name} — done"), ToastKind::Success),
        Outcome::Failure => (format!("{name} — failed"), ToastKind::Failure),
        Outcome::Cancelled => (format!("{name} — cancelled"), ToastKind::Cancelled),
        // suite_ui::Outcome is #[non_exhaustive]: a future variant we don't yet
        // model falls back to the neutral cancelled/indeterminate styling rather
        // than misreporting a job as cleanly done or failed.
        _ => (format!("{name} — finished"), ToastKind::Cancelled),
    }
}

impl App {
    /// Start a background job for a catalog tool, then narrate the result. The
    /// state change is the manager's; the screen switch and activity-log lines
    /// are the front-end's, driven by the returned [`StartOutcome`].
    pub(crate) fn start_job(&mut self, id: &str, name: &str) {
        // Clone the config so the `&mut self.jobs` borrow and the config read
        // don't overlap (config is reached through a `&self` accessor, which
        // borrows the whole struct). A job start is a rare user action and
        // AppConfig is small — already cloned per refresh — so the cost is moot.
        let config = self.config().clone();
        match self.jobs.start(id, name, &config) {
            StartOutcome::Started { display } => {
                self.current_screen = Screen::Jobs;
                self.log_event(format!("{name}: job started ({display})"));
            }
            StartOutcome::AlreadyRunning => {
                self.log_event(format!("{name}: a job is already running (cancel it first)"));
            }
            StartOutcome::NoCommand => {
                self.log_event(format!("{name} has no launch command yet"));
            }
            StartOutcome::SpawnFailed { display } => {
                self.log_event(format!("{name}: failed to start ({display})"));
            }
        }
    }

    /// Poll the running job for output and completion. Returns `true` when the UI
    /// must repaint. On the finishing tick, react to the outcome: log the summary,
    /// flash a toast, and — only when this tool opts in via the catalog's
    /// `refresh_after` (carried on `FinishedJob::should_refresh`) — request a
    /// refresh now the tool slot is free. A self-contained job must not silently
    /// re-probe every adapter.
    pub fn poll_job(&mut self) -> bool {
        let outcome = self.jobs.poll();
        if let Some(finished) = outcome.finished {
            self.log_event(finished.summary);
            self.toast = Some(toast_for(&finished.outcome));
            if finished.should_refresh {
                self.request_refresh();
            }
        }
        outcome.repaint
    }

    /// The status-bar job state, built from the manager's slot. A live job
    /// outranks any recorded last outcome (running shows while a handle is
    /// present). Borrows `self.jobs` directly so the returned `JobState` can hold
    /// `&str` into it — going through an owned domain enum would return a
    /// reference to a temporary.
    pub fn job_state(&self) -> suite_ui::JobState<'_> {
        if let Some(job) = &self.jobs.job {
            return suite_ui::JobState::Running { name: &job.name };
        }
        match &self.jobs.last_outcome {
            Some(outcome) => match to_suite_outcome(outcome.outcome()) {
                suite_ui::Outcome::Cancelled => suite_ui::JobState::Cancelled {
                    name: &outcome.name,
                },
                suite_ui::Outcome::Success => suite_ui::JobState::Done {
                    name: &outcome.name,
                    ok: true,
                },
                suite_ui::Outcome::Failure => suite_ui::JobState::Done {
                    name: &outcome.name,
                    ok: false,
                },
                // suite_ui::Outcome is #[non_exhaustive]: an unmodeled future
                // variant surfaces as a non-clean Done rather than falsely
                // claiming a successful exit.
                _ => suite_ui::JobState::Done {
                    name: &outcome.name,
                    ok: false,
                },
            },
            None => suite_ui::JobState::Idle,
        }
    }

    /// Request cancellation of the running job, if any, and log it.
    pub(crate) fn cancel_job(&mut self) {
        if let Some(name) = self.jobs.cancel() {
            self.log_event(format!("{name}: cancel requested"));
        }
    }
}
