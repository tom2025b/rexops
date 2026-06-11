//! App-owned background job state transitions, plus the finished-job
//! outcome and history record types they produce.

use super::{JobExit, JobOutput};
use crate::app::{App, Screen};
use crate::tools;

pub(crate) const JOB_HISTORY_CAP: usize = 50;
pub(crate) const JOB_OUTPUT_CAP: usize = 1000;

/// How the last job ended, reduced to what the status bar and history need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastOutcome {
    pub name: String,
    pub ok: bool,
    pub cancelled: bool,
}

impl LastOutcome {
    pub fn as_outcome(&self) -> suite_ui::Outcome {
        if self.cancelled {
            suite_ui::Outcome::Cancelled
        } else if self.ok {
            suite_ui::Outcome::Success
        } else {
            suite_ui::Outcome::Failure
        }
    }
}

/// One entry in the bounded job history shown on the Jobs screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRecord {
    pub name: String,
    pub outcome: LastOutcome,
    pub summary: String,
}

pub(crate) fn toast_for(outcome: &LastOutcome) -> (String, suite_ui::ToastKind) {
    use suite_ui::{Outcome, ToastKind};
    let name = &outcome.name;
    match outcome.as_outcome() {
        Outcome::Success => (format!("{name} — done"), ToastKind::Success),
        Outcome::Failure => (format!("{name} — failed"), ToastKind::Failure),
        Outcome::Cancelled => (format!("{name} — cancelled"), ToastKind::Cancelled),
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
        let Some(command) = tools::resolve_command(id, &self.config) else {
            self.log_event(format!("{name} has no launch command yet"));
            return;
        };
        match super::spawn(name, &command) {
            Some(handle) => {
                self.job_output.clear();
                self.last_job = None;
                self.last_outcome = None;
                self.current_screen = Screen::Jobs;
                self.log_event(format!("{name}: job started ({command})"));
                self.job = Some(handle);
            }
            None => self.log_event(format!("{name}: failed to start ({command})")),
        }
    }

    pub fn job_state(&self) -> suite_ui::JobState<'_> {
        if let Some(job) = &self.job {
            return suite_ui::JobState::Running { name: &job.name };
        }
        match &self.last_outcome {
            Some(outcome) => match outcome.as_outcome() {
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

    pub fn poll_job(&mut self) {
        let Some(job) = self.job.as_mut() else {
            return;
        };

        let mut scratch = Vec::new();
        let drained = job.drain_into(&mut scratch);
        let exited = job.poll_done();
        for out in scratch {
            self.push_job_output(out);
        }

        if let (Some(exit), true) = (exited, drained) {
            let job = self.job.as_ref().expect("job present while finishing");
            let name = job.name.clone();
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
            self.job_history.push(JobRecord {
                name: name.clone(),
                outcome: outcome.clone(),
                summary,
            });
            if self.job_history.len() > JOB_HISTORY_CAP {
                self.job_history.remove(0);
            }
            self.toast = Some(toast_for(&outcome));
            self.job = None;
            self.request_refresh();
        }
    }

    pub(crate) fn cancel_job(&mut self) {
        if let Some(job) = self.job.as_mut() {
            job.cancel();
            let name = job.name.clone();
            self.log_event(format!("{name}: cancel requested"));
        }
    }
}
