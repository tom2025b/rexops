//! app.rs — Application state and high-level behavior for the TUI.
//!
//! The App owns:
//! - The current OpsSnapshot (the single source of truth for what we show).
//! - A flag indicating whether a background refresh is in progress.
//! - A channel Sender so it can spawn workers that deliver new snapshots.
//! - A simple help-visible toggle (for the initial dashboard).
//!
//! It does *not* perform rendering (see ui.rs) and does *not* own the
//! terminal or event loop (see main.rs).
//!
//! Refresh implementation note:
//! We spawn an ordinary std::thread for each refresh (adapters are sync).
//! The thread calls the shared rexops_app::build_snapshot (the single
//! implementation) and sends the result back over mpsc. UI stays responsive.

use std::collections::VecDeque;
use std::sync::mpsc;

// The probe logic that used to live here (and was duplicated with CLI) has
// moved to rexops-app. We import the shared builder and the types we still
// need locally (AdapterId is used for health keys in a couple of places,
// AppConfig for the refresh thread).
use rexops_app::build_snapshot;
use rexops_core::{AppConfig, OpsSnapshot};

use crate::jobs::{self, JobExit, JobHandle, JobOutput};
use crate::launcher::{self, ForegroundRunner};
use crate::palette::{self, Command, PaletteCommand};

/// A mutating action that has been *requested* but not yet *confirmed*.
///
/// A mutating action never executes the moment the user asks for it: it first becomes a
/// `PendingAction`, which the UI renders as an explicit confirmation modal.
/// Only an explicit confirm (Enter) runs it; cancel (Esc) discards it.
///
/// It is deliberately a small enum, not a boxed trait object. The action set is
/// known and fixed. Launching a specialist tool is the current confirmed action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingAction {
    /// Launch a specialist tool FOREGROUND (hand over the terminal), by catalog
    /// id, shown to the user as `name`. Used for interactive tools.
    LaunchTool { id: String, name: String },
    /// Run a non-interactive tool as a BACKGROUND job, streaming its output into
    /// the Jobs screen. Same id/name keys as a launch; the difference is how it
    /// runs (background + streamed vs. foreground hand-over).
    RunJob { id: String, name: String },
}

impl PendingAction {
    /// The headline question shown in the confirmation modal.
    pub fn prompt(&self) -> String {
        match self {
            PendingAction::LaunchTool { name, .. } => format!("Launch {name}?"),
            PendingAction::RunJob { name, .. } => format!("Run {name} as a background job?"),
        }
    }

    /// A dry-run preview line: exactly what *would* happen, computed without
    /// performing it. Both variants resolve the command (PATH → config) the same
    /// way the real run does, but never spawn anything. Feed-only tools (no
    /// executable) report that nothing would run.
    pub fn preview(&self, config: &AppConfig) -> String {
        let id = match self {
            PendingAction::LaunchTool { id, .. } | PendingAction::RunJob { id, .. } => id,
        };
        match launcher::resolve_command(id, config) {
            Some(command) => format!("Will run:  {command}"),
            None => "No launch command yet (nothing will run)".to_owned(),
        }
    }
}

/// Structured record of how the last background job ended — the data the shared
/// `StatusBar` needs, kept apart from the human-readable `last_job` summary so
/// neither has to be parsed back out of the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastOutcome {
    /// The tool's display name.
    pub name: String,
    /// True if it finished cleanly (exit 0); false for a non-zero exit OR a
    /// cancel/signal. Combined with `cancelled` to pick the status-bar state.
    pub ok: bool,
    /// True if the job was cancelled / terminated by a signal rather than
    /// exiting on its own.
    pub cancelled: bool,
}

impl LastOutcome {
    /// Reduce to the suite's shared [`Outcome`](suite_ui::Outcome) — the single
    /// classification every job-event renderer (status bar, footer toast, history
    /// row) maps through, so the glyph/colour can never drift between them.
    /// Cancellation wins over the exit code: a cancelled job reads as cancelled
    /// even if it happened to exit 0 in the same instant.
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

/// One entry in the Jobs screen's history: a finished job's name, how it ended,
/// and the exact exit summary already shown for `last_job`. Output lines are NOT
/// retained — only the live/last run keeps its buffer (history is a roll-up of
/// outcomes, not a log archive), which keeps memory bounded regardless of how
/// chatty a tool is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRecord {
    /// The tool's display name.
    pub name: String,
    /// How it ended (drives the glyph/colour, same mapping as the status bar).
    pub outcome: LastOutcome,
    /// The one-line exit summary (e.g. "backup: finished (exit 0)").
    pub summary: String,
}

/// How many finished jobs the Jobs-screen history keeps (newest last). Bounded so
/// a long session can't grow the history without limit; older entries roll off.
const JOB_HISTORY_CAP: usize = 50;

/// How many live output lines the Jobs screen keeps for the current/last run.
/// The buffer rolls: once full, the oldest line drops as a new one arrives, so a
/// long-running or chatty tool can't grow memory without bound. The pane only
/// shows what fits anyway, so older-than-cap lines aren't reachable on screen.
const JOB_OUTPUT_CAP: usize = 1000;

/// Whether a tool runs as a streamed background job (non-interactive) or as a
/// foreground hand-over (interactive). Interactive TUIs can't be piped into a
/// pane, so they keep the foreground path. This is the single place that draws
/// the line; the palette and Launcher both route through it.
pub fn is_streamable(tool_id: &str) -> bool {
    // `proto` is an interactive checklist/protocol runner — it needs the real
    // terminal. Everything else in the catalog (bulwark scan, workstate snapshot,
    // the Workstate-backed sections) emits output and exits, so it streams.
    !matches!(tool_id, "proto")
}

/// Map a finished job's outcome to a footer toast (message + kind), reusing the
/// suite's job-event toast kinds so a flash reads the same as the status bar:
/// clean exit → `Success` (`✓`), non-zero exit → `Failure` (`✗`), cancel/signal
/// → `Cancelled` (`■`). The single place job outcomes become a toast.
fn toast_for(outcome: &LastOutcome) -> (String, suite_ui::ToastKind) {
    use suite_ui::{Outcome, ToastKind};
    let name = &outcome.name;
    // Classify once via the shared Outcome, then pick the matching message +
    // toast kind. No glyph/colour logic here — that lives in suite-ui.
    match outcome.as_outcome() {
        Outcome::Success => (format!("{name} — done"), ToastKind::Success),
        Outcome::Failure => (format!("{name} — failed"), ToastKind::Failure),
        Outcome::Cancelled => (format!("{name} — cancelled"), ToastKind::Cancelled),
    }
}

/// The top-level application state.
///
/// All data that the UI renders comes from (or is derived from) the
/// `snapshot` field. The rest of the fields are UI-only transient state.
pub struct App {
    /// The latest point-in-time view we have from the adapters.
    pub snapshot: OpsSnapshot,

    /// True while a background thread is currently running probes.
    /// Used to show a "Refreshing..." indicator and to ignore duplicate 'r'.
    pub refreshing: bool,

    /// Whether to show the inline help text (toggled by '?' or 'h').
    pub show_help: bool,

    /// Which top-level screen is currently active.
    pub current_screen: Screen,

    /// Sorted list of adapter ids from the current snapshot (for stable ordering in lists).
    pub adapter_names: Vec<String>,

    /// Currently selected adapter name (for Adapters screen; name-based so filtering works robustly).
    pub selected_adapter: Option<String>,

    /// Current filter string for the adapters list (live search).
    pub filter: String,

    /// Selected row in the Launcher screen (index into launchpad::CATALOG).
    pub selected_tool: usize,

    /// A mutating action awaiting explicit confirmation, if any.
    ///
    /// When `Some`, a confirmation modal is shown and ALL input is captured by
    /// it (see the gate at the top of `on_action`) — no keys reach the
    /// underlying screen until the user confirms (Enter) or cancels (Esc).
    pub pending_action: Option<PendingAction>,

    /// Whether the command palette overlay is open. While open it is modal: keys
    /// type into the query / move the selection / dispatch, and are swallowed
    /// from the underlying screen (see the gate in `on_action`).
    pub palette_open: bool,

    /// The palette's live filter text.
    pub palette_query: String,

    /// The selected row in the (filtered) palette list.
    pub palette_selected: usize,

    /// The one running background job, if any. `None` when idle. A single slot
    /// enforces one-job-at-a-time; arming a new job while this is `Some` is
    /// refused.
    pub job: Option<JobHandle>,

    /// The current job's streamed output lines (newest last), shown on the Jobs
    /// screen. Retained after the job finishes so the last run stays readable. A
    /// rolling buffer capped at [`JOB_OUTPUT_CAP`]: oldest lines drop once full so
    /// a chatty/long-running tool can't grow memory without bound.
    pub job_output: VecDeque<JobOutput>,

    /// A one-line summary of how the last job ended (name + exit), for the Jobs
    /// screen header once `job` is `None` again.
    pub last_job: Option<String>,

    /// Structured outcome of the last finished job: its name and whether it ended
    /// cleanly / was cancelled. Parallel to `last_job` (the display string) so the
    /// shared `StatusBar` can read real state instead of parsing the summary.
    /// `None` until a job has finished.
    pub last_outcome: Option<LastOutcome>,

    /// Finished jobs in completion order (newest last), capped at
    /// [`JOB_HISTORY_CAP`]. Shown as a roll-up on the Jobs screen so the user can
    /// see what ran this session, not just the single last run.
    pub job_history: Vec<JobRecord>,

    /// A transient job-event notification (message + kind), shown in the footer
    /// until the next keypress clears it. `None` when there is nothing to flash.
    /// Set when a job finishes; rendered with the shared `suite_ui::Toast`.
    pub toast: Option<(String, suite_ui::ToastKind)>,

    /// Loaded config (respects which adapters are enabled).
    pub config: AppConfig,

    /// Recent events/logs for the dashboard pane (newest last).
    pub recent_events: Vec<String>,

    /// Sender end of the channel that worker threads use to deliver completed
    /// snapshots. We keep it here so `request_refresh` can clone it.
    tx: mpsc::Sender<OpsSnapshot>,
}

/// Top-level screen selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Dashboard,
    Adapters,
    System,
    Scripts,
    /// Tools / inventory screen backed by the Workstate snapshot.
    Tools,
    /// Launcher screen: pick a tool from the static catalog and launch it.
    Launcher,
    /// Jobs screen: live (or last) output of a background job.
    Jobs,
}

impl App {
    /// Create a new App with an empty initial snapshot.
    /// The caller supplies the channel sender and loaded config (created in main).
    pub fn new(tx: mpsc::Sender<OpsSnapshot>, config: AppConfig) -> Self {
        Self {
            snapshot: OpsSnapshot::new(),
            refreshing: false,
            show_help: false,
            current_screen: Screen::default(),
            adapter_names: Vec::new(),
            selected_adapter: None,
            filter: String::new(),
            selected_tool: 0,
            pending_action: None,
            palette_open: false,
            palette_query: String::new(),
            palette_selected: 0,
            job: None,
            job_output: VecDeque::new(),
            last_job: None,
            last_outcome: None,
            job_history: Vec::new(),
            toast: None,
            config,
            recent_events: vec!["TUI started".to_owned()],
            tx,
        }
    }

    /// Spawn a background thread that probes adapters and sends a fresh
    /// snapshot back over the channel.
    ///
    /// If a refresh is already in flight we do nothing (simple debounce).
    pub fn request_refresh(&mut self) {
        if self.refreshing {
            return;
        }
        self.refreshing = true;
        self.log_event("Refresh requested (background thread)");

        // Clone the sender and config (small) so the thread can move them in.
        let tx = self.tx.clone();
        let cfg = self.config.clone();

        std::thread::spawn(move || {
            let snapshot = build_snapshot(&cfg);
            // If the receiver has been dropped (app shutting down) we just
            // ignore the send error — the thread will exit anyway.
            let _ = tx.send(snapshot);
        });
    }

    /// Called from the main loop when a new snapshot arrives from a worker.
    /// Rebuilds the adapter list for navigation UIs.
    pub fn apply_snapshot(&mut self, snapshot: OpsSnapshot) {
        self.snapshot = snapshot;
        // Rebuild sorted list of adapter names for consistent list UI ordering.
        let mut names: Vec<String> = self.snapshot.adapter_health.keys().cloned().collect();
        names.sort();
        self.adapter_names = names;
        // Maintain or reset selection by name (robust to filtering).
        let visible = self.filtered_adapter_names();
        if let Some(ref sel) = self.selected_adapter {
            if !visible.contains(sel) {
                self.selected_adapter = visible.first().cloned();
            }
        } else if !visible.is_empty() {
            self.selected_adapter = visible.first().cloned();
        }
        self.log_event("Snapshot updated from adapter probes");
    }

    /// Whether the current screen accepts live-filter typing. The shared `filter`
    /// string drives the Adapters list and the Dashboard adapters table, so both
    /// screens take character / backspace / esc input into it; every other screen
    /// leaves those keys for their own bindings. This is the single place the
    /// filter's scope is defined.
    fn filter_screen(&self) -> bool {
        matches!(self.current_screen, Screen::Adapters | Screen::Dashboard)
    }

    /// Returns the current filtered view of adapter names (live search shared by
    /// the Adapters screen and the Dashboard adapters table).
    pub fn filtered_adapter_names(&self) -> Vec<String> {
        if self.filter.is_empty() {
            self.adapter_names.clone()
        } else {
            let f = self.filter.to_lowercase();
            self.adapter_names
                .iter()
                .filter(|n| n.to_lowercase().contains(&f))
                .cloned()
                .collect()
        }
    }

    /// Append a log/event message (keeps last 8 for the pane).
    pub fn log_event(&mut self, msg: impl Into<String>) {
        self.recent_events.push(msg.into());
        if self.recent_events.len() > 8 {
            self.recent_events.remove(0);
        }
    }

    /// Toggle the help text overlay / hint area.
    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    /// Dismiss the transient job-event toast, if any. Called on every key event so
    /// a flash clears as soon as the user does anything — the app owns the
    /// (trivial) lifetime, keeping the shared `Toast` widget stateless.
    fn clear_toast(&mut self) {
        self.toast = None;
    }

    // --- command palette ----------------------------------------------------

    /// The palette's current filtered command list (by `palette_query`).
    pub fn palette_commands(&self) -> Vec<PaletteCommand> {
        palette::filter(&self.palette_query)
    }

    /// Open the palette (fresh query + selection). A no-op while an action is
    /// pending — the confirm modal owns input then.
    fn open_palette(&mut self) {
        if self.pending_action.is_some() {
            return;
        }
        self.palette_open = true;
        self.palette_query.clear();
        self.palette_selected = 0;
    }

    /// Close the palette and clear its transient state.
    fn close_palette(&mut self) {
        self.palette_open = false;
        self.palette_query.clear();
        self.palette_selected = 0;
    }

    /// Move the palette selection by `delta` rows, clamped to the filtered list
    /// (with wraparound), mirroring the list navigation elsewhere.
    fn palette_move(&mut self, down: bool) {
        let len = self.palette_commands().len();
        if len == 0 {
            self.palette_selected = 0;
            return;
        }
        self.palette_selected = if down {
            (self.palette_selected + 1) % len
        } else {
            (self.palette_selected + len - 1) % len
        };
    }

    /// Dispatch the selected palette command, then close the palette. A nav/
    /// action command is re-dispatched as its `Action`; a `run <tool>` command
    /// arms the same confirm gate the Launcher uses (never spawns directly).
    /// Returns true only if the dispatched action means "quit" (none of the
    /// palette commands do, but we thread the contract through faithfully).
    fn palette_activate(&mut self, runner: &mut impl ForegroundRunner) -> bool {
        let filtered = self.palette_commands();
        let Some(chosen) = filtered.get(self.palette_selected).cloned() else {
            self.close_palette();
            return false;
        };
        self.close_palette();
        match chosen.command {
            Command::Action(action) => self.on_action(action, runner),
            Command::RunTool { id, name } => {
                self.arm_tool(id, name);
                false
            }
        }
    }

    // --- background jobs -----------------------------------------------------

    /// Arm a tool for running: pick the path (streamed job vs. foreground launch)
    /// and stage the matching `PendingAction`. The confirm modal then requires an
    /// explicit Enter before anything runs — the single gated entry point shared
    /// by the Launcher screen and the palette.
    fn arm_tool(&mut self, id: String, name: String) {
        self.pending_action = Some(if is_streamable(&id) {
            PendingAction::RunJob { id, name: name.clone() }
        } else {
            PendingAction::LaunchTool { id, name: name.clone() }
        });
        self.log_event(format!("{name}: confirm (Enter) or cancel (Esc)"));
    }

    /// Spawn a confirmed background job. Refuses if one is already running (one at
    /// a time). Switches to the Jobs screen so the streaming output is visible.
    fn start_job(&mut self, id: &str, name: &str) {
        if self.job.is_some() {
            self.log_event(format!("{name}: a job is already running (cancel it first)"));
            return;
        }
        let Some(command) = launcher::resolve_command(id, &self.config) else {
            self.log_event(format!("{name} has no launch command yet"));
            return;
        };
        match jobs::spawn(name, &command) {
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

    /// The current job state mapped onto the suite's shared [`JobState`], for the
    /// persistent status bar. A live handle → `Running`; otherwise the last
    /// finished job's structured outcome → `Cancelled` / `Done{ok}`; nothing run
    /// yet → `Idle`. Borrows the relevant name, so it lives only as long as `self`.
    pub fn job_state(&self) -> suite_ui::JobState<'_> {
        if let Some(job) = &self.job {
            return suite_ui::JobState::Running { name: &job.name };
        }
        // Map the shared Outcome onto the status bar's richer JobState (which also
        // carries Running/Idle). Going through `as_outcome` keeps the cancelled-vs-
        // exit-code decision in one place rather than re-deriving it here.
        match &self.last_outcome {
            Some(o) => match o.as_outcome() {
                suite_ui::Outcome::Cancelled => suite_ui::JobState::Cancelled { name: &o.name },
                suite_ui::Outcome::Success => suite_ui::JobState::Done { name: &o.name, ok: true },
                suite_ui::Outcome::Failure => suite_ui::JobState::Done { name: &o.name, ok: false },
            },
            None => suite_ui::JobState::Idle,
        }
    }

    /// Append one output line to the rolling [`job_output`](Self::job_output)
    /// buffer, dropping the oldest line once it is at [`JOB_OUTPUT_CAP`]. Keeps
    /// memory bounded no matter how much a tool prints.
    fn push_job_output(&mut self, out: JobOutput) {
        if self.job_output.len() == JOB_OUTPUT_CAP {
            self.job_output.pop_front();
        }
        self.job_output.push_back(out);
    }

    /// Drain any output the running job has produced and, once it has exited,
    /// finish it (record how it ended, drop the handle, refresh the snapshot).
    /// Called every loop iteration from `main`; a no-op when idle.
    pub fn poll_job(&mut self) {
        let Some(job) = self.job.as_mut() else {
            return;
        };
        // Drain everything available this tick, learning whether the output
        // channel has disconnected (both reader threads finished and dropped their
        // senders — the only race-free "output is complete" signal). Collect into
        // a scratch buffer first so we can release the `&mut self.job` borrow
        // before pushing into `self.job_output`.
        let mut scratch: Vec<JobOutput> = Vec::new();
        let drained = job.drain_into(&mut scratch);
        let exited = job.poll_done();
        for out in scratch {
            self.push_job_output(out);
        }

        // Finish only once the child has exited AND its output has fully drained.
        // `try_wait` reporting the child gone can win the race against a reader
        // still flushing its last line, so requiring the channel to have
        // disconnected too is what stops trailing output from being lost. Until
        // both hold we return and try again next tick — never blocking the UI.
        if let (Some(exit), true) = (exited, drained) {
            let job = self.job.as_ref().expect("job present while finishing");
            let name = job.name.clone();
            let (summary, outcome) = match exit {
                JobExit::Code(0) => (
                    format!("{name}: finished (exit 0)"),
                    LastOutcome { name: name.clone(), ok: true, cancelled: false },
                ),
                JobExit::Code(code) => (
                    format!("{name}: finished (exit {code})"),
                    LastOutcome { name: name.clone(), ok: false, cancelled: false },
                ),
                JobExit::Signalled => (
                    format!("{name}: cancelled / signalled"),
                    LastOutcome { name: name.clone(), ok: false, cancelled: true },
                ),
            };
            self.log_event(summary.clone());
            self.last_job = Some(summary.clone());
            self.last_outcome = Some(outcome.clone());

            // Append to the bounded history (newest last) so the Jobs screen can
            // show what ran this session, not just the single last run.
            self.job_history.push(JobRecord {
                name: name.clone(),
                outcome: outcome.clone(),
                summary,
            });
            if self.job_history.len() > JOB_HISTORY_CAP {
                self.job_history.remove(0);
            }

            // Flash a job-event toast in the footer, using the suite's job-event
            // toast kinds (the same glyph/colour mapping as the status bar).
            self.toast = Some(toast_for(&outcome));

            self.job = None;
            // A finished job may have changed what a fresh probe would see.
            self.request_refresh();
        }
    }

    /// Cancel the running job (kills the child). The next `poll_job` reports it as
    /// signalled and finishes the bookkeeping. No-op when idle.
    fn cancel_job(&mut self) {
        if let Some(job) = self.job.as_mut() {
            job.cancel();
            let name = job.name.clone();
            self.log_event(format!("{name}: cancel requested"));
        }
    }

    /// Execute the pending action (the user confirmed). Clears `pending_action`
    /// either way. This is the ONLY place a mutating action actually runs, so
    /// every mutation is provably gated behind confirmation.
    fn confirm_pending(&mut self, runner: &mut impl ForegroundRunner) {
        let Some(action) = self.pending_action.take() else {
            return;
        };
        match action {
            PendingAction::LaunchTool { id, name } => {
                let report = launcher::launch_tool(&id, &name, &self.config, runner);
                self.log_event(report.message());
                if report.should_refresh() {
                    self.request_refresh();
                }
            }
            PendingAction::RunJob { id, name } => {
                self.start_job(&id, &name);
            }
        }
    }

    /// Discard the pending action (the user cancelled). Logs that nothing ran.
    fn cancel_pending(&mut self) {
        if let Some(action) = self.pending_action.take() {
            let name = match action {
                PendingAction::LaunchTool { name, .. } | PendingAction::RunJob { name, .. } => name,
            };
            self.log_event(format!("{name}: cancelled (nothing ran)"));
        }
    }

    /// Handle a high-level Action.
    ///
    /// Returns true if the action means "quit now".
    pub fn on_action(
        &mut self,
        action: crate::action::Action,
        launcher: &mut impl ForegroundRunner,
    ) -> bool {
        // Any key dismisses a lingering job-event toast (it has had at least one
        // draw cycle since `poll_job` set it). Done before the modal gates so the
        // flash clears even when a key is otherwise swallowed by a modal.
        self.clear_toast();

        // Confirmation gate: while an action is pending, the modal is modal.
        // Enter confirms (runs it), Esc cancels (discards it), and EVERY other
        // key is swallowed so nothing leaks through to the underlying screen.
        // We return early before normal dispatch — and never quit from here.
        // This is the innermost modal: it takes precedence over the palette.
        if self.pending_action.is_some() {
            match action {
                crate::action::Action::Activate => self.confirm_pending(launcher),
                crate::action::Action::Cancel => self.cancel_pending(),
                _ => {} // ignored: a modal swallows all other input
            }
            return false;
        }

        // Palette gate: while the palette is open it owns input — type to filter,
        // move the selection, Enter dispatches, Esc closes. Every other key is
        // swallowed from the underlying screen. `palette_activate` may dispatch a
        // nav/action command (which can never quit) or arm a job.
        if self.palette_open {
            match action {
                crate::action::Action::Cancel => self.close_palette(),
                crate::action::Action::Activate => return self.palette_activate(launcher),
                crate::action::Action::Up => self.palette_move(false),
                crate::action::Action::Down => self.palette_move(true),
                crate::action::Action::Backspace => {
                    self.palette_query.pop();
                    self.palette_selected = 0;
                }
                crate::action::Action::InputChar(c) if c.is_ascii_graphic() || c == ' ' => {
                    self.palette_query.push(c);
                    self.palette_selected = 0;
                }
                // Re-opening while open is a no-op; all else is swallowed.
                _ => {}
            }
            return false;
        }

        match action {
            crate::action::Action::Quit => true,
            crate::action::Action::Refresh => {
                self.request_refresh();
                false
            }
            crate::action::Action::ToggleHelp => {
                self.toggle_help();
                false
            }
            crate::action::Action::SwitchToDashboard => {
                self.current_screen = Screen::Dashboard;
                self.log_event("Switched to Dashboard screen");
                false
            }
            crate::action::Action::SwitchToAdapters => {
                self.current_screen = Screen::Adapters;
                self.log_event("Switched to Adapters screen");
                false
            }
            crate::action::Action::SwitchToSystem => {
                self.current_screen = Screen::System;
                self.log_event("Switched to System screen");
                false
            }
            crate::action::Action::SwitchToScripts => {
                self.current_screen = Screen::Scripts;
                self.log_event("Switched to Scripts screen");
                false
            }
            crate::action::Action::SwitchToTools => {
                self.current_screen = Screen::Tools;
                self.log_event("Switched to Tools screen");
                false
            }
            crate::action::Action::SwitchToLauncher => {
                self.current_screen = Screen::Launcher;
                self.log_event("Switched to Launcher screen");
                false
            }
            crate::action::Action::SwitchToJobs => {
                self.current_screen = Screen::Jobs;
                self.log_event("Switched to Jobs screen");
                false
            }
            crate::action::Action::OpenPalette => {
                self.open_palette();
                false
            }
            crate::action::Action::CancelJob => {
                self.cancel_job();
                false
            }
            crate::action::Action::Up => {
                if self.current_screen == Screen::Adapters {
                    let visible = self.filtered_adapter_names();
                    if !visible.is_empty() {
                        if let Some(cur) = &self.selected_adapter {
                            if let Some(pos) = visible.iter().position(|n| n == cur) {
                                let new_pos = if pos > 0 { pos - 1 } else { visible.len() - 1 };
                                self.selected_adapter = Some(visible[new_pos].clone());
                            }
                        }
                    }
                } else if self.current_screen == Screen::Launcher {
                    let len = crate::screens::launchpad::CATALOG.len();
                    if len > 0 {
                        self.selected_tool = (self.selected_tool + len - 1) % len;
                    }
                }
                false
            }
            crate::action::Action::Down => {
                if self.current_screen == Screen::Adapters {
                    let visible = self.filtered_adapter_names();
                    if !visible.is_empty() {
                        if let Some(cur) = &self.selected_adapter {
                            if let Some(pos) = visible.iter().position(|n| n == cur) {
                                let new_pos = (pos + 1) % visible.len();
                                self.selected_adapter = Some(visible[new_pos].clone());
                            }
                        }
                    }
                } else if self.current_screen == Screen::Launcher {
                    let len = crate::screens::launchpad::CATALOG.len();
                    if len > 0 {
                        self.selected_tool = (self.selected_tool + 1) % len;
                    }
                }
                false
            }
            crate::action::Action::Activate => {
                if self.current_screen == Screen::Adapters {
                    if let Some(name) = &self.selected_adapter {
                        // Surface the selected adapter in the event stream.
                        self.snapshot.add_note(format!(
                            "selected adapter detail: {name} (press r to refresh for live)"
                        ));
                    }
                } else if self.current_screen == Screen::Launcher {
                    // Do NOT launch yet. Requesting a launch only *arms* a
                    // pending action; the confirmation modal then requires an
                    // explicit Enter to actually run it. `arm_tool` picks the
                    // path (streamed background job vs. foreground hand-over).
                    // This is a single gated entry point shared with the palette.
                    if let Some(tool) = crate::screens::launchpad::CATALOG.get(self.selected_tool) {
                        self.arm_tool(tool.id.to_owned(), tool.name.to_owned());
                    }
                }
                false
            }
            crate::action::Action::Cancel => {
                if self.filter_screen() && !self.filter.is_empty() {
                    self.filter.clear();
                    let visible = self.filtered_adapter_names();
                    self.selected_adapter = visible.first().cloned();
                    false
                } else if self.current_screen == Screen::Launcher {
                    // Esc on the Launcher goes back to the Dashboard, not quit.
                    self.current_screen = Screen::Dashboard;
                    self.log_event("Launcher: back to Dashboard");
                    false
                } else {
                    true // real quit
                }
            }
            crate::action::Action::InputChar(c) => {
                if self.filter_screen() && c.is_ascii_graphic() {
                    self.filter.push(c);
                    let visible = self.filtered_adapter_names();
                    self.selected_adapter = visible.first().cloned();
                }
                false
            }
            crate::action::Action::Backspace => {
                if self.filter_screen() && !self.filter.is_empty() {
                    self.filter.pop();
                    let visible = self.filtered_adapter_names();
                    if let Some(cur) = &self.selected_adapter {
                        if !visible.contains(cur) {
                            self.selected_adapter = visible.first().cloned();
                        }
                    } else {
                        self.selected_adapter = visible.first().cloned();
                    }
                }
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Action;
    use crate::launcher::ChildExit;

    struct FakeRunner {
        calls: usize,
    }

    impl ForegroundRunner for FakeRunner {
        fn run_foreground(&mut self, _command: &str) -> std::io::Result<ChildExit> {
            self.calls += 1;
            Ok(ChildExit::Success)
        }
    }

    /// Build an App already on the Launcher screen for navigation tests.
    fn launcher_app() -> App {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default());
        app.current_screen = Screen::Launcher;
        app
    }

    /// A bare App (no job, fresh state) for status-mapping tests.
    fn bare_app() -> App {
        let (tx, _rx) = mpsc::channel();
        App::new(tx, AppConfig::default())
    }

    /// An App whose snapshot carries the given adapter names, on the Dashboard
    /// screen, for the live-filter tests. Names are applied via `apply_snapshot`
    /// so `adapter_names` is derived exactly as it is in production.
    fn dashboard_app_with_adapters(names: &[&str]) -> App {
        let mut app = bare_app();
        let mut snap = OpsSnapshot::new();
        for name in names {
            snap.adapter_health
                .insert((*name).to_owned(), rexops_core::AdapterHealth::Healthy);
        }
        app.apply_snapshot(snap);
        app.current_screen = Screen::Dashboard;
        app
    }

    #[test]
    fn typing_on_the_dashboard_drives_the_shared_filter() {
        // Before this change, InputChar was a no-op off the Adapters screen. Now
        // the Dashboard takes filter input too, narrowing the adapter view.
        let mut app = dashboard_app_with_adapters(&["bulwark", "scripts", "system"]);
        let mut runner = FakeRunner { calls: 0 };
        for c in "bul".chars() {
            app.on_action(Action::InputChar(c), &mut runner);
        }
        assert_eq!(app.filter, "bul");
        assert_eq!(app.filtered_adapter_names(), vec!["bulwark".to_owned()]);
    }

    #[test]
    fn esc_clears_the_dashboard_filter_without_quitting() {
        let mut app = dashboard_app_with_adapters(&["bulwark", "scripts"]);
        let mut runner = FakeRunner { calls: 0 };
        for c in "bul".chars() {
            app.on_action(Action::InputChar(c), &mut runner);
        }
        assert_eq!(app.filter, "bul");
        // Esc with a non-empty filter clears it and does NOT request quit.
        let quit = app.on_action(Action::Cancel, &mut runner);
        assert!(!quit, "esc must clear the filter, not quit, while filtering");
        assert!(app.filter.is_empty());
        assert_eq!(app.filtered_adapter_names().len(), 2);
    }

    #[test]
    fn backspace_edits_the_dashboard_filter() {
        let mut app = dashboard_app_with_adapters(&["bulwark", "scripts"]);
        let mut runner = FakeRunner { calls: 0 };
        for c in "bulx".chars() {
            app.on_action(Action::InputChar(c), &mut runner);
        }
        assert_eq!(app.filter, "bulx");
        assert!(app.filtered_adapter_names().is_empty(), "'bulx' matches nothing");
        app.on_action(Action::Backspace, &mut runner);
        assert_eq!(app.filter, "bul");
        assert_eq!(app.filtered_adapter_names(), vec!["bulwark".to_owned()]);
    }

    #[test]
    fn filter_typing_is_inert_on_a_non_filter_screen() {
        // System is not a filter screen, so characters there must NOT mutate the
        // shared filter (they stay available for that screen's own bindings).
        let mut app = dashboard_app_with_adapters(&["bulwark", "scripts"]);
        app.current_screen = Screen::System;
        let mut runner = FakeRunner { calls: 0 };
        for c in "bul".chars() {
            app.on_action(Action::InputChar(c), &mut runner);
        }
        assert!(app.filter.is_empty(), "typing on System must not filter");
    }

    /// Spawn `command` as a real job on `app` and drive `poll_job` like the main
    /// loop until the job finishes (or a timeout). Used to exercise the
    /// completion bookkeeping (history + toast) end to end.
    fn run_job_to_completion(app: &mut App, name: &str, command: &str) {
        app.job = Some(jobs::spawn(name, command).expect("spawn test job"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.job.is_some() {
            app.poll_job();
            assert!(
                std::time::Instant::now() < deadline,
                "job did not finish in time"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn finishing_a_job_records_history_and_flashes_a_toast() {
        use suite_ui::ToastKind;

        let mut app = bare_app();
        assert!(app.job_history.is_empty());

        // A clean exit → one Success history entry + a Success toast.
        run_job_to_completion(&mut app, "true", "true");
        assert_eq!(app.job_history.len(), 1, "a finished job is recorded");
        let rec = &app.job_history[0];
        assert_eq!(rec.name, "true");
        assert!(rec.outcome.ok && !rec.outcome.cancelled);
        assert!(matches!(app.toast, Some((_, ToastKind::Success))));

        // A non-zero exit → a second entry + a Failure toast.
        run_job_to_completion(&mut app, "false", "false");
        assert_eq!(app.job_history.len(), 2, "history accumulates");
        let rec = &app.job_history[1];
        assert!(!rec.outcome.ok && !rec.outcome.cancelled);
        assert!(matches!(app.toast, Some((_, ToastKind::Failure))));
    }

    #[test]
    fn history_is_capped_and_rolls_off_oldest_first() {
        let mut app = bare_app();
        // Pre-fill at the cap with sentinel records, then push one more via a real
        // finished job; the oldest must roll off and the newest land at the end.
        for i in 0..JOB_HISTORY_CAP {
            app.job_history.push(JobRecord {
                name: format!("old-{i}"),
                outcome: LastOutcome {
                    name: format!("old-{i}"),
                    ok: true,
                    cancelled: false,
                },
                summary: format!("old-{i}: finished (exit 0)"),
            });
        }
        run_job_to_completion(&mut app, "newest", "true");
        assert_eq!(app.job_history.len(), JOB_HISTORY_CAP, "history stays capped");
        assert_eq!(
            app.job_history.first().unwrap().name,
            "old-1",
            "the oldest entry rolled off"
        );
        assert_eq!(
            app.job_history.last().unwrap().name,
            "newest",
            "the new entry is appended last"
        );
    }

    #[test]
    fn job_output_is_a_rolling_buffer_capped_at_job_output_cap() {
        let mut app = bare_app();
        // Push well past the cap; the buffer must stay bounded and keep the
        // newest lines (the oldest roll off the front).
        for i in 0..(JOB_OUTPUT_CAP + 250) {
            app.push_job_output(JobOutput::Stdout(format!("line-{i}")));
        }
        assert_eq!(app.job_output.len(), JOB_OUTPUT_CAP, "buffer stays capped");
        assert_eq!(
            app.job_output.front(),
            Some(&JobOutput::Stdout("line-250".to_owned())),
            "the oldest retained line is exactly cap-from-the-end"
        );
        assert_eq!(
            app.job_output.back(),
            Some(&JobOutput::Stdout(format!("line-{}", JOB_OUTPUT_CAP + 249))),
            "the newest line is kept at the back"
        );
    }

    #[test]
    fn any_key_dismisses_a_lingering_toast() {
        let mut app = bare_app();
        let mut runner = FakeRunner { calls: 0 };
        app.toast = Some(("backup — done".to_owned(), suite_ui::ToastKind::Success));
        // A harmless key (refresh) goes through `on_action`, which clears the toast
        // up front regardless of what the action itself does.
        app.on_action(Action::Refresh, &mut runner);
        assert!(app.toast.is_none(), "any key must dismiss the toast");
    }

    #[test]
    fn toast_for_maps_each_outcome_to_its_kind() {
        use suite_ui::ToastKind;
        let ok = LastOutcome { name: "j".into(), ok: true, cancelled: false };
        let fail = LastOutcome { name: "j".into(), ok: false, cancelled: false };
        let cancelled = LastOutcome { name: "j".into(), ok: false, cancelled: true };
        assert!(matches!(toast_for(&ok), (_, ToastKind::Success)));
        assert!(matches!(toast_for(&fail), (_, ToastKind::Failure)));
        assert!(matches!(toast_for(&cancelled), (_, ToastKind::Cancelled)));
        // Cancelled takes precedence over `ok` (a cancel can race a clean exit).
        let cancelled_but_ok = LastOutcome { name: "j".into(), ok: true, cancelled: true };
        assert!(matches!(toast_for(&cancelled_but_ok), (_, ToastKind::Cancelled)));
    }

    #[test]
    fn job_state_maps_outcome_to_the_shared_status_enum() {
        use suite_ui::JobState;

        // Fresh app, no job ever run → Idle.
        let mut app = bare_app();
        assert_eq!(app.job_state(), JobState::Idle);

        // A clean finish → Done { ok: true }.
        app.last_outcome = Some(LastOutcome {
            name: "backup".to_owned(),
            ok: true,
            cancelled: false,
        });
        assert_eq!(app.job_state(), JobState::Done { name: "backup", ok: true });

        // A non-zero exit → Done { ok: false }.
        app.last_outcome = Some(LastOutcome {
            name: "rescan".to_owned(),
            ok: false,
            cancelled: false,
        });
        assert_eq!(app.job_state(), JobState::Done { name: "rescan", ok: false });

        // A cancel/signal → Cancelled, regardless of `ok`.
        app.last_outcome = Some(LastOutcome {
            name: "deploy".to_owned(),
            ok: false,
            cancelled: true,
        });
        assert_eq!(app.job_state(), JobState::Cancelled { name: "deploy" });
    }

    #[test]
    fn a_live_job_outranks_the_last_outcome_in_the_status_bar() {
        use suite_ui::JobState;

        // `job_state` reports Running whenever a job handle is present, regardless
        // of any recorded last outcome. Spawning `sleep` (which won't exit during
        // the test) gives a present handle deterministically; we kill it after the
        // assertion so the test leaves no lingering process behind.
        let mut app = bare_app();
        app.last_outcome = Some(LastOutcome {
            name: "old".to_owned(),
            ok: true,
            cancelled: false,
        });
        app.job = Some(jobs::spawn("live-tool", "sleep").expect("spawn sleep"));
        assert_eq!(app.job_state(), JobState::Running { name: "live-tool" });
        if let Some(job) = app.job.as_mut() {
            job.cancel();
        }
    }

    /// Select a catalog tool by id on a Launcher app (panics if absent).
    fn select_tool(app: &mut App, id: &str) {
        let idx = crate::screens::launchpad::CATALOG
            .iter()
            .position(|t| t.id == id)
            .unwrap_or_else(|| panic!("{id} in catalog"));
        app.selected_tool = idx;
    }

    /// A Launcher app with `proto` selected and pinned to an explicit binary.
    /// `proto` is the INTERACTIVE tool, so arming it yields a foreground
    /// `LaunchTool` that drives the (fake) `ForegroundRunner` on confirm — the
    /// path these runner-based tests exercise.
    fn launcher_app_with_proto() -> App {
        let mut app = launcher_app();
        app.config.adapters.insert(
            "proto".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/tmp/proto".to_owned()),
                timeout_secs: None,
            },
        );
        select_tool(&mut app, "proto");
        app
    }

    #[test]
    fn activate_on_launcher_arms_foreground_tool_without_spawning() {
        // Enter on the Launcher must only *arm* a pending action — never spawn
        // before the user confirms. `proto` is interactive → foreground LaunchTool.
        let mut app = launcher_app_with_proto();
        let mut runner = FakeRunner { calls: 0 };

        let quit = app.on_action(Action::Activate, &mut runner);

        assert!(!quit);
        assert_eq!(
            app.pending_action,
            Some(PendingAction::LaunchTool {
                id: "proto".to_owned(),
                name: "Proto".to_owned(),
            })
        );
        assert_eq!(runner.calls, 0, "arming must not spawn a process");
    }

    #[test]
    fn activate_on_launcher_arms_streamable_tool_as_a_job() {
        // A non-interactive tool (scripts) must arm a RunJob — the background,
        // streamed path — rather than a foreground LaunchTool.
        let mut app = launcher_app();
        select_tool(&mut app, "scripts");
        let mut runner = FakeRunner { calls: 0 };

        app.on_action(Action::Activate, &mut runner);

        assert_eq!(
            app.pending_action,
            Some(PendingAction::RunJob {
                id: "scripts".to_owned(),
                name: "Scripts".to_owned(),
            }),
            "a streamable tool must arm a background job"
        );
        assert_eq!(runner.calls, 0, "arming must not spawn a process");
    }

    #[test]
    fn confirm_runs_foreground_tool_and_clears_it() {
        // With a pending foreground launch, Enter confirms: it runs once via the
        // ForegroundRunner, requests refresh, and clears the pending action.
        let mut app = launcher_app_with_proto();
        let mut runner = FakeRunner { calls: 0 };

        app.on_action(Action::Activate, &mut runner); // arm
        let quit = app.on_action(Action::Activate, &mut runner); // confirm

        assert!(!quit);
        assert_eq!(runner.calls, 1, "confirm must run exactly once");
        assert!(app.pending_action.is_none(), "pending must be cleared");
        assert!(app.refreshing);
        assert!(app
            .recent_events
            .iter()
            .any(|e| e == "Proto exited successfully"));
    }

    #[test]
    fn confirm_streamable_tool_does_not_use_foreground_runner() {
        // Confirming a RunJob goes through the background job path, NOT the
        // foreground runner. The pinned binary isn't a real executable, so the
        // spawn fails and is reported — but the runner must never be touched, and
        // no job handle is left dangling.
        let mut app = launcher_app();
        app.config.adapters.insert(
            "scripts".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/tmp/definitely-not-executable".to_owned()),
                timeout_secs: None,
            },
        );
        select_tool(&mut app, "scripts");
        let mut runner = FakeRunner { calls: 0 };

        app.on_action(Action::Activate, &mut runner); // arm RunJob
        let quit = app.on_action(Action::Activate, &mut runner); // confirm

        assert!(!quit);
        assert_eq!(runner.calls, 0, "a job must not use the foreground runner");
        assert!(app.pending_action.is_none(), "pending must be cleared");
        assert!(app.job.is_none(), "a failed spawn leaves no job handle");
        assert!(app
            .recent_events
            .iter()
            .any(|e| e.contains("failed to start")));
    }

    #[test]
    fn cancel_discards_pending_action_without_spawning() {
        // Esc with a pending action cancels: nothing runs, pending is cleared,
        // and the app does not quit.
        let mut app = launcher_app_with_proto();
        let mut runner = FakeRunner { calls: 0 };

        app.on_action(Action::Activate, &mut runner); // arm
        let quit = app.on_action(Action::Cancel, &mut runner); // cancel

        assert!(!quit, "cancelling a pending action must not quit");
        assert_eq!(runner.calls, 0, "cancel must not spawn a process");
        assert!(app.pending_action.is_none(), "pending must be cleared");
        assert!(app
            .recent_events
            .iter()
            .any(|e| e.contains("cancelled (nothing ran)")));
    }

    #[test]
    fn other_keys_are_swallowed_while_pending() {
        // The modal is modal: any non-confirm/cancel key while pending is
        // ignored. It must not navigate, must not spawn, and must leave the
        // pending action untouched.
        let mut app = launcher_app_with_proto();
        let mut runner = FakeRunner { calls: 0 };

        app.on_action(Action::Activate, &mut runner); // arm
        let before = app.selected_tool;
        let quit = app.on_action(Action::Down, &mut runner); // should be swallowed

        assert!(!quit);
        assert_eq!(runner.calls, 0, "swallowed key must not spawn");
        assert_eq!(app.selected_tool, before, "navigation must be blocked");
        assert!(
            app.pending_action.is_some(),
            "pending must survive a swallowed key"
        );
    }

    #[test]
    fn preview_shows_resolved_command_or_no_command() {
        // The dry-run preview resolves the command without spawning. A pinned
        // binary shows "Will run: <path>"; a feed-only tool shows that nothing
        // would run.
        //
        // We pin an id that is NOT on PATH so the config-binary fallback is what
        // resolves — otherwise a real PATH hit on the dev box
        // would win and make the assertion environment-dependent (same reason
        // the launcher.rs tests use a fake id).
        let mut app = launcher_app();
        app.config.adapters.insert(
            "definitely-not-a-real-tool-xyz".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/tmp/fake-tool".to_owned()),
                timeout_secs: None,
            },
        );

        let launch = PendingAction::LaunchTool {
            id: "definitely-not-a-real-tool-xyz".to_owned(),
            name: "FakeTool".to_owned(),
        };
        assert_eq!(launch.preview(&app.config), "Will run:  /tmp/fake-tool");

        let feed_only = PendingAction::LaunchTool {
            // A different id that is never on PATH and has no config binary.
            id: "another-nonexistent-feed-tool-abc".to_owned(),
            name: "Workstate".to_owned(),
        };
        assert_eq!(
            feed_only.preview(&app.config),
            "No launch command yet (nothing will run)"
        );
    }

    #[test]
    fn launcher_down_and_up_wrap_around_catalog() {
        let mut app = launcher_app();
        let mut runner = FakeRunner { calls: 0 };
        let last = crate::screens::launchpad::CATALOG.len() - 1;

        // Down advances, then wraps from the last entry back to 0.
        app.on_action(Action::Down, &mut runner);
        assert_eq!(app.selected_tool, 1);
        for _ in 1..crate::screens::launchpad::CATALOG.len() {
            app.on_action(Action::Down, &mut runner);
        }
        assert_eq!(app.selected_tool, 0, "Down must wrap past the end");

        // Up from 0 wraps to the last entry.
        app.on_action(Action::Up, &mut runner);
        assert_eq!(app.selected_tool, last, "Up must wrap before the start");
    }

    #[test]
    fn launcher_esc_goes_back_to_dashboard_not_quit() {
        let mut app = launcher_app();
        let mut runner = FakeRunner { calls: 0 };

        let quit = app.on_action(Action::Cancel, &mut runner);

        assert!(!quit, "Esc on Launcher must not quit the app");
        assert_eq!(app.current_screen, Screen::Dashboard);
    }

    #[test]
    fn launcher_enter_arms_the_selected_tool() {
        // Activate on the Launcher must arm a PendingAction for the *selected*
        // catalog tool, carrying that tool's id and name, and must not spawn.
        // `tools` is non-interactive → it arms a RunJob.
        let mut app = launcher_app();
        select_tool(&mut app, "tools");
        let entry = &crate::screens::launchpad::CATALOG[app.selected_tool];
        let mut runner = FakeRunner { calls: 0 };

        app.on_action(Action::Activate, &mut runner);

        assert_eq!(
            app.pending_action,
            Some(PendingAction::RunJob {
                id: entry.id.to_owned(),
                name: entry.name.to_owned(),
            }),
            "Activate must arm the selected tool"
        );
        assert_eq!(runner.calls, 0, "arming must not spawn a process");
    }

    // --- command palette ----------------------------------------------------

    #[test]
    fn palette_opens_filters_and_dispatches_navigation() {
        let mut app = launcher_app();
        let mut runner = FakeRunner { calls: 0 };

        app.on_action(Action::OpenPalette, &mut runner);
        assert!(app.palette_open, "^P must open the palette");

        // Type "system" → the list narrows to the System nav command at top.
        for c in "system".chars() {
            app.on_action(Action::InputChar(c), &mut runner);
        }
        assert!(
            app.palette_commands().iter().any(|c| c.label == "system"),
            "query should surface the system command"
        );

        // Enter dispatches the selected command (nav → switch screen) and closes.
        app.palette_selected = app
            .palette_commands()
            .iter()
            .position(|c| c.label == "system")
            .expect("system present");
        let quit = app.on_action(Action::Activate, &mut runner);

        assert!(!quit);
        assert!(!app.palette_open, "dispatch must close the palette");
        assert_eq!(app.current_screen, Screen::System, "nav command must switch");
    }

    #[test]
    fn palette_run_tool_arms_confirm_without_spawning() {
        // Choosing a `run <tool>` command in the palette must arm the SAME
        // confirm gate as the Launcher — never spawn directly.
        let mut app = launcher_app();
        let mut runner = FakeRunner { calls: 0 };

        app.on_action(Action::OpenPalette, &mut runner);
        for c in "run bulwark".chars() {
            app.on_action(Action::InputChar(c), &mut runner);
        }
        let pos = app
            .palette_commands()
            .iter()
            .position(|c| c.label == "run bulwark")
            .expect("run bulwark present");
        app.palette_selected = pos;
        app.on_action(Action::Activate, &mut runner);

        assert!(!app.palette_open, "dispatch closes the palette");
        assert_eq!(
            app.pending_action,
            Some(PendingAction::RunJob {
                id: "bulwark".to_owned(),
                name: "Bulwark".to_owned(),
            }),
            "run command must arm a job behind the confirm gate"
        );
        assert_eq!(runner.calls, 0, "arming must not spawn");
        assert!(app.job.is_none(), "must not start a job before confirm");
    }

    #[test]
    fn palette_esc_closes_without_dispatching() {
        let mut app = launcher_app();
        let mut runner = FakeRunner { calls: 0 };
        let screen_before = app.current_screen;

        app.on_action(Action::OpenPalette, &mut runner);
        let quit = app.on_action(Action::Cancel, &mut runner);

        assert!(!quit, "Esc in the palette closes it, does not quit");
        assert!(!app.palette_open);
        assert_eq!(app.current_screen, screen_before, "nothing was dispatched");
    }

    #[test]
    fn palette_does_not_open_while_confirm_pending() {
        // The confirm modal is the innermost gate: ^P must not open the palette
        // while an action awaits confirmation.
        let mut app = launcher_app_with_proto();
        let mut runner = FakeRunner { calls: 0 };

        app.on_action(Action::Activate, &mut runner); // arm (confirm pending)
        app.on_action(Action::OpenPalette, &mut runner); // should be swallowed

        assert!(!app.palette_open, "palette must not open over the confirm modal");
        assert!(app.pending_action.is_some(), "pending must be untouched");
    }
}
