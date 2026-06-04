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

    /// Loaded config (respects which adapters are enabled).
    pub config: AppConfig,

    /// Recent events/logs for the dashboard pane (newest last).
    pub recent_events: Vec<String>,

    /// One-shot foreground launch request for the terminal-owning main loop.
    pending_launch: Option<LaunchRequest>,

    /// Sender end of the channel that worker threads use to deliver completed
    /// snapshots. We keep it here so `request_refresh` can clone it.
    tx: mpsc::Sender<OpsSnapshot>,
}

/// Foreground launch requests that require the real terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchRequest {
    /// Stage 2 hardcodes ScriptVault to prove suspend/run/restore.
    ScriptVault,
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
            config,
            recent_events: vec!["TUI started".to_owned()],
            pending_launch: None,
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

    /// Take the pending launch request, if an action queued one.
    pub fn take_launch_request(&mut self) -> Option<LaunchRequest> {
        self.pending_launch.take()
    }

    /// Toggle the help text overlay / hint area.
    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    /// Handle a high-level Action.
    ///
    /// Returns true if the action means "quit now".
    pub fn on_action(&mut self, action: crate::action::Action) -> bool {
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
                self.pending_launch = Some(LaunchRequest::ScriptVault);
                self.log_event("Launch requested: ScriptVault");
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
                }
                false
            }
            crate::action::Action::Cancel => {
                if self.current_screen == Screen::Adapters && !self.filter.is_empty() {
                    self.filter.clear();
                    let visible = self.filtered_adapter_names();
                    self.selected_adapter = visible.first().cloned();
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

    #[test]
    fn launch_action_queues_scriptvault_request_once() {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default());

        assert!(!app.on_action(Action::Launch));
        assert_eq!(app.take_launch_request(), Some(LaunchRequest::ScriptVault));
        assert_eq!(app.take_launch_request(), None);
    }
}

// The local build_snapshot was removed in this increment. We now call the
// shared rexops_app::build_snapshot (re-exported/used at top of file) from
// request_refresh. This eliminates the previous duplication with the CLI.
//
// The Learning Notes about "keeping logic in TUI for now" are obsolete — the
// plan has been followed and the shared rexops-app layer is in place.
