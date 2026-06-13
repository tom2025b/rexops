//! Finished-job outcome classification and history record types.

/// How a finished job ended, as domain truth — no UI/colour meaning. A
/// front-end maps this to its own presentation vocabulary at the render
/// boundary; keeping it UI-free is what lets these types live in rexops-app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobOutcome {
    Success,
    Failure,
    Cancelled,
}

/// Where the single background-job slot is in its lifecycle, as domain truth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobLifecycle {
    Idle,
    Running { name: String },
    Done { name: String, ok: bool },
    Cancelled { name: String },
}

/// How the last job ended, reduced to what a status bar and history need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastOutcome {
    pub name: String,
    pub ok: bool,
    pub cancelled: bool,
}

impl LastOutcome {
    /// Classify into the domain outcome. Cancelled takes precedence over
    /// ok/failure: a cancelled job is neither a clean success nor a real
    /// failure, so the `ok` flag is ignored when `cancelled` is set.
    pub fn outcome(&self) -> JobOutcome {
        if self.cancelled {
            JobOutcome::Cancelled
        } else if self.ok {
            JobOutcome::Success
        } else {
            JobOutcome::Failure
        }
    }
}

/// One entry in a bounded job history (shown on the Jobs screen).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRecord {
    pub name: String,
    pub outcome: LastOutcome,
    pub summary: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_takes_precedence_over_ok() {
        let o = LastOutcome {
            name: "x".to_owned(),
            ok: true,
            cancelled: true,
        };
        assert_eq!(o.outcome(), JobOutcome::Cancelled);
    }

    #[test]
    fn ok_is_success_and_not_ok_is_failure() {
        let ok = LastOutcome {
            name: "x".to_owned(),
            ok: true,
            cancelled: false,
        };
        let bad = LastOutcome {
            name: "x".to_owned(),
            ok: false,
            cancelled: false,
        };
        assert_eq!(ok.outcome(), JobOutcome::Success);
        assert_eq!(bad.outcome(), JobOutcome::Failure);
    }

    #[test]
    fn job_lifecycle_variants_construct() {
        let _ = JobLifecycle::Idle;
        let _ = JobLifecycle::Running {
            name: "j".to_owned(),
        };
        let _ = JobLifecycle::Done {
            name: "j".to_owned(),
            ok: true,
        };
        let _ = JobLifecycle::Cancelled {
            name: "j".to_owned(),
        };
    }
}
