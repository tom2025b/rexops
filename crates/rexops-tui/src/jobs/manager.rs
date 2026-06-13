//! App-owned background job state transitions, plus the TUI-side mapping of the
//! domain job outcome (`rexops_app::JobOutcome`) to suite_ui presentation types.
//!
//! The outcome/history *data* types (`LastOutcome`, `JobRecord`) and the
//! domain classifier (`LastOutcome::outcome`) now live in rexops-app. This file
//! keeps the App glue and the render-boundary translation from domain → UI.

use super::{JobExit, JobOutput};
use crate::app::{App, Screen};
use crate::tools;
use rexops_app::{JobOutcome, JobRecord, LastOutcome};

pub(crate) const JOB_HISTORY_CAP: usize = 50;
pub(crate) const JOB_OUTPUT_CAP: usize = 1000;

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
    pub(crate) fn start_job(&mut self, id: &str, name: &str) {
        if self.job.is_some() {
            self.log_event(format!(
                "{name}: a job is already running (cancel it first)"
            ));
            return;
        }
        // Resolve the FULL command — program plus catalog args — through the same
        // entry point the confirm-gate preview rendered, so the job runs exactly
        // what the user approved. Resolving only the program here (as before) and
        // dropping the args would silently diverge from the preview the moment a
        // background tool needs a subcommand.
        let Some(command) = tools::resolve_launch_command(id, self.config()) else {
            self.log_event(format!("{name} has no launch command yet"));
            return;
        };
        let display = command.display();
        match super::spawn(id, name, &command.program, &command.args) {
            Some(handle) => {
                self.job_output.clear();
                self.jobs_scroll = 0; // fresh output → follow the bottom
                self.last_job = None;
                self.last_outcome = None;
                self.current_screen = Screen::Jobs;
                self.log_event(format!("{name}: job started ({display})"));
                self.job = Some(handle);
            }
            None => self.log_event(format!("{name}: failed to start ({display})")),
        }
    }

    pub fn job_state(&self) -> suite_ui::JobState<'_> {
        if let Some(job) = &self.job {
            return suite_ui::JobState::Running { name: &job.name };
        }
        match &self.last_outcome {
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

    pub(crate) fn push_job_output(&mut self, out: JobOutput) {
        if self.job_output.len() == JOB_OUTPUT_CAP {
            self.job_output.pop_front();
        }
        self.job_output.push_back(out);
    }

    /// Scroll the Jobs output. `up` moves toward older lines (increasing the
    /// from-bottom offset), `down` toward newer; reaching `0` resumes auto-follow.
    /// The offset is clamped to the buffer so it can never point past the top.
    pub(crate) fn scroll_jobs_output(&mut self, up: bool) {
        if up {
            // Never scroll so far the whole pane would be above the buffer; one
            // line must always remain. Render clamps further to the actual pane.
            let max = self.job_output.len().saturating_sub(1);
            self.jobs_scroll = (self.jobs_scroll + 1).min(max);
        } else {
            self.jobs_scroll = self.jobs_scroll.saturating_sub(1);
        }
    }

    /// Poll the running job for new output and completion. Returns `true` if
    /// anything changed that the UI must repaint — new output lines arrived, or
    /// the job finished — and `false` when there was nothing to do (no job, or a
    /// running job that produced nothing this tick). The runtime uses this to
    /// avoid redrawing an idle frame.
    pub fn poll_job(&mut self) -> bool {
        let Some(job) = self.job.as_mut() else {
            return false;
        };

        let mut scratch = Vec::new();
        let drained = job.drain_into(&mut scratch);
        let exited = job.poll_done();
        let got_output = !scratch.is_empty();
        for out in scratch {
            self.push_job_output(out);
        }
        // Auto-follow: when scrolled to the bottom (jobs_scroll == 0) new output
        // simply stays visible. When scrolled up it holds its offset; the render
        // clamps it to the buffer. We deliberately keep this simple — no
        // pin-against-front-pop bookkeeping; a little drift while scrolled up is
        // fine.

        if let (Some(exit), true) = (exited, drained) {
            // Safe: `poll_job` returns early above (line ~130) when `self.job` is
            // None, and nothing between there and here clears it — the field is
            // only set to None at the end of *this* branch. So whenever `exited`
            // is Some, the job is necessarily still present. The mutable borrow
            // from the early `as_mut` has ended, so we re-borrow shared here.
            #[allow(clippy::expect_used)]
            let job = self.job.as_ref().expect("job present while finishing");
            let name = job.name.clone();
            let id = job.id.clone();
            let (summary, outcome) = match exit {
                JobExit::Code(0) => (
                    format!("{name}: finished (exit 0)"),
                    LastOutcome {
                        name: name.clone(),
                        ok: true,
                        cancelled: false,
                    },
                ),
                JobExit::Code(code) => (
                    format!("{name}: finished (exit {code})"),
                    LastOutcome {
                        name: name.clone(),
                        ok: false,
                        cancelled: false,
                    },
                ),
                JobExit::Signalled => (
                    format!("{name}: cancelled / signalled"),
                    LastOutcome {
                        name: name.clone(),
                        ok: false,
                        cancelled: true,
                    },
                ),
            };

            self.log_event(summary.clone());
            self.last_job = Some(summary.clone());
            self.last_outcome = Some(outcome.clone());
            self.job_history.push_back(JobRecord {
                name: name.clone(),
                outcome: outcome.clone(),
                summary,
            });
            if self.job_history.len() > JOB_HISTORY_CAP {
                self.job_history.pop_front();
            }
            self.toast = Some(toast_for(&outcome));
            self.job = None;
            // Snap back to following the bottom so the job's FINAL lines are
            // visible. If the user had scrolled up mid-run to read earlier output,
            // leaving the offset pinned would strand the pane on a now-static
            // buffer showing "— scrolled" against a job that's already done.
            self.jobs_scroll = 0;
            // Only re-probe when THIS tool's run could change what an adapter
            // observes (catalog `refresh_after`). A self-contained job (e.g. a
            // checklist runner) finishing must not silently re-probe every
            // adapter — its output is already on screen.
            if tools::refreshes_after(&id) {
                self.request_refresh();
            }
            return true; // job finished — header/history/toast all changed
        }

        // Still running: a repaint is only needed if output actually arrived.
        got_output
    }

    pub(crate) fn cancel_job(&mut self) {
        if let Some(job) = self.job.as_mut() {
            job.cancel();
            let name = job.name.clone();
            self.log_event(format!("{name}: cancel requested"));
        }
    }
}
