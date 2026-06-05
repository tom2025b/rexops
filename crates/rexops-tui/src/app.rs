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
    /// Tools / inventory screen backed by ToolFoundryAdapter data (ownership, symlinks, health).
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

    /// Handle a high-level Action.
    ///
    /// Returns true if the action means "quit now".
    pub fn on_action(
        &mut self,
        action: crate::action::Action,
        launcher: &mut impl ForegroundRunner,
    ) -> bool {
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
            crate::action::Action::Launch => {
                let report =
                    launcher::launch_tool("scriptvault", "ScriptVault", &self.config, launcher);
                self.log_event(report.message());
                if report.should_refresh() {
                    self.request_refresh();
                }
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
                    // Launch the selected tool. launch_tool resolves a command
                    // (which → config); feed-only tools degrade to a message.
                    if let Some(tool) = crate::screens::launchpad::CATALOG.get(self.selected_tool) {
                        let report =
                            launcher::launch_tool(tool.id, tool.name, &self.config, launcher);
                        self.log_event(report.message());
                        if report.should_refresh() {
                            self.request_refresh();
                        }
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

    #[test]
    fn launch_action_runs_scriptvault_and_requests_refresh() {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default());
        app.config.adapters.insert(
            "scriptvault".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/tmp/scriptvault".to_owned()),
                timeout_secs: None,
            },
        );
        let mut runner = FakeRunner { calls: 0 };

        assert!(!app.on_action(Action::Launch, &mut runner));

        assert_eq!(runner.calls, 1);
        assert!(app.refreshing);
        assert!(app
            .recent_events
            .iter()
            .any(|event| event == "ScriptVault exited successfully"));
    }

    /// Build an App already on the Launcher screen for navigation tests.
    fn launcher_app() -> App {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default());
        app.current_screen = Screen::Launcher;
        app
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
    fn launcher_enter_routes_selected_tool_to_launch() {
        // Activate on the Launcher must route the *selected* catalog tool through
        // launch_tool and log its report. We assert routing (a report mentioning
        // the selected tool's name was logged) rather than launch outcome, since
        // whether a real binary resolves depends on the host PATH. The graceful
        // no-command behavior itself is covered deterministically in
        // launcher.rs::launch_tool_without_command_reports_gracefully_and_skips_runner.
        let mut app = launcher_app();
        let idx = crate::screens::launchpad::CATALOG
            .iter()
            .position(|t| t.id == "toolfoundry")
            .expect("toolfoundry in catalog");
        app.selected_tool = idx;
        let name = crate::screens::launchpad::CATALOG[idx].name;
        let mut runner = FakeRunner { calls: 0 };

        app.on_action(Action::Activate, &mut runner);

        assert!(
            app.recent_events.iter().any(|e| e.contains(name)),
            "Activate must log a launch report for the selected tool ({name})"
        );
        // toolfoundry is a feed-only tool (never on PATH, no config binary), so
        // Activate must degrade gracefully without spawning a process.
        assert_eq!(runner.calls, 0, "feed-only tool must not spawn a process");
    }
}

// The local build_snapshot was removed in this increment. We now call the
// shared rexops_app::build_snapshot (re-exported/used at top of file) from
// request_refresh. This eliminates the previous duplication with the CLI.
//
// The Learning Notes about "keeping logic in TUI for now" are obsolete — the
// plan has been followed and the shared rexops-app layer is in place.
