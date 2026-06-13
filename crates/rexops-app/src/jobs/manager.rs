//! The single-slot background-job state machine: spawn one job, stream its
//! output into a bounded buffer, scroll it, reap it on completion, and keep a
//! bounded history of finished jobs.
//!
//! This owns the job *state* and the *pure transitions* over it. It deliberately
//! knows nothing about the front-end: no terminal, no toast, no activity log, no
//! screen switching, no snapshot refresh. Transitions that have a UI side effect
//! return a small result value describing what happened, and the front-end reacts
//! (logs a line, flips a toast, switches to the Jobs screen, requests a refresh).
//! That boundary is what lets the manager live in rexops-app.

use std::collections::VecDeque;

use crate::tools::resolve_launch_command;
use rexops_core::AppConfig;

use super::{spawn, JobExit, JobHandle, JobOutput, JobRecord, LastOutcome};

/// Cap on retained finished-job history entries (oldest roll off the front).
pub const JOB_HISTORY_CAP: usize = 50;
/// Cap on retained live-output lines (oldest roll off the front).
pub const JOB_OUTPUT_CAP: usize = 1000;

/// What [`JobManager::start`] did, for the front-end to narrate. The manager has
/// already performed the state change (or declined to); this only carries the
/// facts the UI needs to log it and decide whether to switch to the Jobs view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartOutcome {
    /// A job was spawned. `display` is the resolved command line (for the log).
    /// The front-end should switch to the Jobs screen.
    Started { display: String },
    /// A job is already running; nothing was started.
    AlreadyRunning,
    /// The tool has no launch command configured; nothing was started.
    NoCommand,
    /// The command resolved but the process failed to spawn. `display` is the
    /// command line that was attempted (for the log).
    SpawnFailed { display: String },
}

/// The result of one [`JobManager::poll`] tick.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PollOutcome {
    /// Whether anything changed that the UI must repaint (new output or the job
    /// finishing). `false` means the runtime can skip the redraw this tick.
    pub repaint: bool,
    /// `Some` only on the tick a job finished: the recorded outcome plus its
    /// human summary. The front-end uses it to flash a toast, log the summary,
    /// and (typically) request a fresh snapshot now the job has released the tool.
    pub finished: Option<FinishedJob>,
}

/// A job that completed on this tick: the classified outcome and its one-line
/// summary. The manager has already appended it to history and cleared the slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishedJob {
    pub outcome: LastOutcome,
    pub summary: String,
    /// Whether the front-end should re-probe now this tool has finished, decided
    /// from the catalog's per-tool `refresh_after` (via [`refreshes_after`]).
    /// Only tools whose run can change what an adapter observes opt in — a
    /// self-contained job (e.g. a checklist runner) finishing must not silently
    /// re-probe every adapter. The policy lives here so both surfaces agree.
    pub should_refresh: bool,
}

/// Owns the one job slot, its output/scroll, the last outcome, and the bounded
/// history. Construct with [`JobManager::default`]; drive it with `start` /
/// `poll` / `scroll` / `cancel`.
///
/// Not `Debug`: `JobHandle` wraps live OS process/thread handles and is not
/// `Debug`, and the front-end never needs to format the manager.
#[derive(Default)]
pub struct JobManager {
    /// The one running job, or `None` when idle. Public so the front-end render
    /// path can read `name`/`command` for the header without a wrapper.
    pub job: Option<JobHandle>,
    /// Live output buffer, bounded to `JOB_OUTPUT_CAP` (oldest roll off).
    pub output: VecDeque<JobOutput>,
    /// Scrollback offset in lines from the BOTTOM. `0` follows the bottom (newest
    /// output stays visible); a positive value pins the view that many lines up.
    pub scroll: usize,
    /// One-line summary of the last finished job, for the idle header. `None`
    /// until a job has finished (cleared when a new job starts).
    pub last_summary: Option<String>,
    /// How the last job ended, for the status bar. `None` until one finishes.
    pub last_outcome: Option<LastOutcome>,
    /// Bounded finished-job history (oldest first), capped to `JOB_HISTORY_CAP`.
    pub history: VecDeque<JobRecord>,
}

impl JobManager {
    /// Resolve `id`'s full launch command and spawn it as the one background job.
    /// Refuses (without spawning) if a job is already running or the command does
    /// not resolve. On success the output buffer is cleared and scroll reset to
    /// follow the bottom. The returned [`StartOutcome`] tells the caller what to
    /// narrate; the screen switch / logging are the caller's to perform.
    pub fn start(&mut self, id: &str, name: &str, config: &AppConfig) -> StartOutcome {
        if self.job.is_some() {
            return StartOutcome::AlreadyRunning;
        }
        // Resolve the FULL command — program plus catalog args — through the same
        // entry point the confirm-gate preview rendered, so the job runs exactly
        // what the user approved (a background tool's subcommand included).
        let Some(command) = resolve_launch_command(id, config) else {
            return StartOutcome::NoCommand;
        };
        let display = command.display();
        match spawn(name, &command.program, &command.args) {
            Some(handle) => {
                self.output.clear();
                self.scroll = 0; // fresh output → follow the bottom
                self.last_summary = None;
                self.last_outcome = None;
                self.job = Some(handle);
                StartOutcome::Started { display }
            }
            None => StartOutcome::SpawnFailed { display },
        }
    }

    /// Append one output line, evicting the oldest if at the cap.
    pub fn push_output(&mut self, out: JobOutput) {
        if self.output.len() == JOB_OUTPUT_CAP {
            self.output.pop_front();
        }
        self.output.push_back(out);
    }

    /// Scroll the output. `up` moves toward older lines (increasing the
    /// from-bottom offset), `down` toward newer; reaching `0` resumes auto-follow.
    /// The offset is clamped to the buffer so it can never point past the top.
    pub fn scroll(&mut self, up: bool) {
        if up {
            // Keep at least one line in view; render clamps further to the pane.
            let max = self.output.len().saturating_sub(1);
            self.scroll = (self.scroll + 1).min(max);
        } else {
            self.scroll = self.scroll.saturating_sub(1);
        }
    }

    /// Poll the running job for new output and completion. See [`PollOutcome`]:
    /// `repaint` is `true` when output arrived or the job finished; `finished` is
    /// `Some` only on the tick the job ended (already recorded to history and the
    /// slot cleared). Returns an all-`false`/`None` outcome when idle.
    pub fn poll(&mut self) -> PollOutcome {
        let Some(job) = self.job.as_mut() else {
            return PollOutcome::default();
        };

        let mut scratch = Vec::new();
        let drained = job.drain_into(&mut scratch);
        let exited = job.poll_done();
        let got_output = !scratch.is_empty();
        // Capture the name and id now, while the mutable borrow of the handle is
        // live — the `push_output` loop below needs `&mut self`, which ends this
        // borrow, and after it the handle would have to be re-read. Cloning here
        // avoids that re-borrow (and the unreachable-`expect` it used to need).
        // The id drives the per-tool refresh gate when the job finishes.
        let name = job.name.clone();
        let id = job.id.clone();
        for out in scratch {
            self.push_output(out);
        }
        // Auto-follow: at the bottom (scroll == 0) new output simply stays
        // visible; scrolled up it holds its offset and render clamps it. Kept
        // simple on purpose — a little drift while scrolled up is fine.

        if let (Some(exit), true) = (exited, drained) {
            let (summary, outcome) = Self::classify(&name, exit);

            self.last_summary = Some(summary.clone());
            self.last_outcome = Some(outcome.clone());
            self.history.push_back(JobRecord {
                name,
                outcome: outcome.clone(),
                summary: summary.clone(),
            });
            if self.history.len() > JOB_HISTORY_CAP {
                self.history.pop_front();
            }
            self.job = None;
            return PollOutcome {
                repaint: true,
                finished: Some(FinishedJob {
                    outcome,
                    summary,
                    // Per-tool gate: only re-probe when this tool's run can
                    // change what an adapter observes. Unknown ids → false.
                    should_refresh: crate::tools::refreshes_after(&id),
                }),
            };
        }

        // Still running: repaint only if output actually arrived.
        PollOutcome {
            repaint: got_output,
            finished: None,
        }
    }

    /// Request cancellation of the running job, if any. Returns the job's name so
    /// the caller can log it; `None` when there was no job to cancel.
    pub fn cancel(&mut self) -> Option<String> {
        let job = self.job.as_mut()?;
        job.cancel();
        Some(job.name.clone())
    }

    /// Build the summary line and recorded outcome for a finished job from its
    /// exit status. Cancelled/signalled is its own outcome (not a plain failure).
    fn classify(name: &str, exit: JobExit) -> (String, LastOutcome) {
        match exit {
            JobExit::Code(0) => (
                format!("{name}: finished (exit 0)"),
                LastOutcome {
                    name: name.to_owned(),
                    ok: true,
                    cancelled: false,
                },
            ),
            JobExit::Code(code) => (
                format!("{name}: finished (exit {code})"),
                LastOutcome {
                    name: name.to_owned(),
                    ok: false,
                    cancelled: false,
                },
            ),
            JobExit::Signalled => (
                format!("{name}: cancelled / signalled"),
                LastOutcome {
                    name: name.to_owned(),
                    ok: false,
                    cancelled: true,
                },
            ),
        }
    }
}
