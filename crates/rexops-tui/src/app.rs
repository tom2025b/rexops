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

use std::sync::mpsc;

// The probe logic that used to live here (and was duplicated with CLI) has
// moved to rexops-app. We import the shared builder and the types we still
// need locally (AdapterId is used for health keys in a couple of places,
// AppConfig for the refresh thread).
use rexops_app::build_snapshot;
use rexops_core::{AppConfig, OpsSnapshot};

use crate::launcher::{self, ForegroundRunner};

/// A mutating action that has been *requested* but not yet *confirmed*.
///
/// This is the reusable core of the Phase 8 safety layer. A mutating action
/// never executes the moment the user asks for it: it first becomes a
/// `PendingAction`, which the UI renders as an explicit confirmation modal.
/// Only an explicit confirm (Enter) runs it; cancel (Esc) discards it.
///
/// It is deliberately a small enum, not a boxed trait object. The action set is
/// known and fixed, so adding a future mutating action (e.g. delete/run) means
/// adding one variant here plus arms in `prompt`/`preview` and the confirm
/// handler — no framework, no abstraction tax. For now there is exactly one
/// variant: launching a specialist tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingAction {
    /// Launch a specialist tool by catalog id, shown to the user as `name`.
    LaunchTool { id: String, name: String },
}

impl PendingAction {
    /// The headline question shown in the confirmation modal.
    pub fn prompt(&self) -> String {
        match self {
            PendingAction::LaunchTool { name, .. } => format!("Launch {name}?"),
        }
    }

    /// A dry-run preview line: exactly what *would* happen, computed without
    /// performing it. For a launch this resolves the command (PATH → config)
    /// the same way the real launch does, but never spawns anything. Feed-only
    /// tools (no executable) report that nothing would run.
    pub fn preview(&self, config: &AppConfig) -> String {
        match self {
            PendingAction::LaunchTool { id, .. } => match launcher::resolve_command(id, config) {
                Some(command) => format!("Will run:  {command}"),
                None => "No launch command yet (nothing will run)".to_owned(),
            },
        }
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
    /// This demonstrates the screens/ modularity from the plan.
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

    /// Loaded config (respects which adapters are enabled).
    pub config: AppConfig,

    /// Recent events/logs for the dashboard pane (newest last).
    pub recent_events: Vec<String>,

    /// Sender end of the channel that worker threads use to deliver completed
    /// snapshots. We keep it here so `request_refresh` can clone it.
    tx: mpsc::Sender<OpsSnapshot>,
}

/// Simple screen selector (more can be added later: Tools, Reports, etc.).
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

    /// Returns the current filtered view of adapter names (live search in Adapters screen).
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
        }
    }

    /// Discard the pending action (the user cancelled). Logs that nothing ran.
    fn cancel_pending(&mut self) {
        if let Some(action) = self.pending_action.take() {
            match action {
                PendingAction::LaunchTool { name, .. } => {
                    self.log_event(format!("{name}: launch cancelled (nothing ran)"));
                }
            }
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
        // Confirmation gate: while an action is pending, the modal is modal.
        // Enter confirms (runs it), Esc cancels (discards it), and EVERY other
        // key is swallowed so nothing leaks through to the underlying screen.
        // We return early before normal dispatch — and never quit from here.
        if self.pending_action.is_some() {
            match action {
                crate::action::Action::Activate => self.confirm_pending(launcher),
                crate::action::Action::Cancel => self.cancel_pending(),
                _ => {} // ignored: a modal swallows all other input
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
                        // Demo: surface selection in notes (real detail pane in render).
                        self.snapshot.add_note(format!(
                            "selected adapter detail: {name} (press r to refresh for live)"
                        ));
                    }
                } else if self.current_screen == Screen::Launcher {
                    // Do NOT launch yet. Requesting a launch only *arms* a
                    // pending action; the confirmation modal then requires an
                    // explicit Enter to actually run it. This is the single
                    // gated entry point for the (only) mutating action.
                    if let Some(tool) = crate::screens::launchpad::CATALOG.get(self.selected_tool) {
                        self.pending_action = Some(PendingAction::LaunchTool {
                            id: tool.id.to_owned(),
                            name: tool.name.to_owned(),
                        });
                        self.log_event(format!(
                            "{}: confirm launch (Enter) or cancel (Esc)",
                            tool.name
                        ));
                    }
                }
                false
            }
            crate::action::Action::Cancel => {
                if self.current_screen == Screen::Adapters && !self.filter.is_empty() {
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
                if self.current_screen == Screen::Adapters && c.is_ascii_graphic() {
                    self.filter.push(c);
                    let visible = self.filtered_adapter_names();
                    self.selected_adapter = visible.first().cloned();
                }
                false
            }
            crate::action::Action::Backspace => {
                if self.current_screen == Screen::Adapters && !self.filter.is_empty() {
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

    /// Pin a catalog entry to an explicit binary so the launch path resolves
    /// deterministically regardless of the host PATH.
    fn launcher_app_with_scripts() -> App {
        let mut app = launcher_app();
        app.config.adapters.insert(
            "scripts".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/tmp/scripts".to_owned()),
                timeout_secs: None,
            },
        );
        let idx = crate::screens::launchpad::CATALOG
            .iter()
            .position(|t| t.id == "scripts")
            .expect("scripts in catalog");
        app.selected_tool = idx;
        app
    }

    #[test]
    fn activate_on_launcher_arms_pending_without_spawning() {
        // Enter on the Launcher must only *arm* a pending action — it must never
        // spawn a process before the user confirms.
        let mut app = launcher_app_with_scripts();
        let mut runner = FakeRunner { calls: 0 };

        let quit = app.on_action(Action::Activate, &mut runner);

        assert!(!quit);
        assert_eq!(
            app.pending_action,
            Some(PendingAction::LaunchTool {
                id: "scripts".to_owned(),
                name: "Scripts".to_owned(),
            })
        );
        assert_eq!(runner.calls, 0, "arming must not spawn a process");
    }

    #[test]
    fn confirm_runs_pending_action_and_clears_it() {
        // With a pending launch, Enter confirms: it runs once, requests refresh,
        // and clears the pending action.
        let mut app = launcher_app_with_scripts();
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
            .any(|e| e == "Scripts exited successfully"));
    }

    #[test]
    fn cancel_discards_pending_action_without_spawning() {
        // Esc with a pending launch cancels: nothing runs, pending is cleared,
        // and the app does not quit.
        let mut app = launcher_app_with_scripts();
        let mut runner = FakeRunner { calls: 0 };

        app.on_action(Action::Activate, &mut runner); // arm
        let quit = app.on_action(Action::Cancel, &mut runner); // cancel

        assert!(!quit, "cancelling a pending action must not quit");
        assert_eq!(runner.calls, 0, "cancel must not spawn a process");
        assert!(app.pending_action.is_none(), "pending must be cleared");
        assert!(app
            .recent_events
            .iter()
            .any(|e| e.contains("launch cancelled")));
    }

    #[test]
    fn other_keys_are_swallowed_while_pending() {
        // The modal is modal: any non-confirm/cancel key while pending is
        // ignored. It must not navigate, must not spawn, and must leave the
        // pending action untouched.
        let mut app = launcher_app_with_scripts();
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
        // catalog tool — carrying that tool's id and name — and must not spawn.
        let mut app = launcher_app();
        let idx = crate::screens::launchpad::CATALOG
            .iter()
            .position(|t| t.id == "tools")
            .expect("tools in catalog");
        app.selected_tool = idx;
        let entry = &crate::screens::launchpad::CATALOG[idx];
        let mut runner = FakeRunner { calls: 0 };

        app.on_action(Action::Activate, &mut runner);

        assert_eq!(
            app.pending_action,
            Some(PendingAction::LaunchTool {
                id: entry.id.to_owned(),
                name: entry.name.to_owned(),
            }),
            "Activate must arm the selected tool"
        );
        assert_eq!(runner.calls, 0, "arming must not spawn a process");
    }
}

// The local build_snapshot was removed in this increment. We now call the
// shared rexops_app::build_snapshot (re-exported/used at top of file) from
// request_refresh. This eliminates the previous duplication with the CLI.
//
// The Learning Notes about "keeping logic in TUI for now" are obsolete — the
// plan has been followed and the shared rexops-app layer is in place.
//
// Learning Notes (Phase 8 — confirmation layer):
// - The confirmation flow is a tiny state machine in App (pending_action:
//   Option<PendingAction>), not a terminal concern, so it is fully unit-testable
//   with FakeRunner — no real TTY needed. Logic lives here; only rendering lives
//   in ui.rs.
// - The gate at the TOP of on_action makes the modal *modal*: while something is
//   pending, Enter confirms, Esc cancels, every other key is swallowed. Nothing
//   leaks to the underlying screen.
// - Safety invariant: launch_tool has exactly one caller (confirm_pending),
//   reachable only via the gate. So a mutation needs two keypresses (arm, then
//   confirm) and the modal always renders between them — there is no
//   single-key arm-and-fire path.
// - PendingAction is a small enum, not a boxed trait. Adding a future mutating
//   action is one variant + arms in prompt/preview/confirm_pending — reusable
//   without an abstraction framework (KISS/YAGNI).
