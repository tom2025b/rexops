//! Owned TUI state, its constructor, and its lifecycle helpers
//! (background snapshot refresh, the activity log, display toggles).

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc;

use rexops_app::build_snapshot;
use rexops_core::{AppConfig, OpsSnapshot};

use super::Screen;
use crate::commands::PendingAction;
use crate::jobs::{JobHandle, JobOutput, JobRecord, LastOutcome};
use crate::tools::{self, CATALOG};

pub struct App {
    pub snapshot: OpsSnapshot,
    pub refreshing: bool,
    pub show_help: bool,
    pub current_screen: Screen,
    pub adapter_names: Vec<String>,
    pub selected_adapter: Option<String>,
    pub filter: String,
    pub selected_tool: usize,
    pub pending_action: Option<PendingAction>,
    pub palette_open: bool,
    pub palette_query: String,
    pub palette_selected: usize,
    pub job: Option<JobHandle>,
    pub job_output: VecDeque<JobOutput>,
    pub last_job: Option<String>,
    pub last_outcome: Option<LastOutcome>,
    pub job_history: VecDeque<JobRecord>,
    pub toast: Option<(String, suite_ui::ToastKind)>,
    pub config: AppConfig,
    pub recent_events: VecDeque<String>,
    /// Per-catalog-tool launch availability, computed once from config + PATH.
    /// The Launcher redraws every ~100ms; resolving availability there would
    /// shell out to `which` on every frame. We cache it here instead and only
    /// recompute when the config changes (see `refresh_launch_availability`).
    launch_availability: HashMap<&'static str, bool>,
    pub(crate) tx: mpsc::Sender<OpsSnapshot>,
}

impl App {
    pub fn new(tx: mpsc::Sender<OpsSnapshot>, config: AppConfig) -> Self {
        let mut app = Self {
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
            job_history: VecDeque::new(),
            toast: None,
            config,
            recent_events: VecDeque::from(["TUI started".to_owned()]),
            launch_availability: HashMap::new(),
            tx,
        };
        app.refresh_launch_availability();
        app
    }

    // --- launch availability cache ---

    /// Recompute the cached launch availability for every catalog tool from the
    /// current config (and PATH). Call once at construction and again whenever
    /// `config` changes. This is the only place `resolve_launch_command` runs for
    /// availability — the render path reads the cache via `is_tool_launchable`.
    pub(crate) fn refresh_launch_availability(&mut self) {
        self.launch_availability = CATALOG
            .iter()
            .map(|tool| {
                (
                    tool.id,
                    tools::resolve_launch_command(tool.id, &self.config).is_some(),
                )
            })
            .collect();
    }

    /// Whether a catalog tool can be launched, read from the cached availability.
    /// Unknown ids (not in the catalog) read as not launchable.
    pub(crate) fn is_tool_launchable(&self, tool_id: &str) -> bool {
        self.launch_availability
            .get(tool_id)
            .copied()
            .unwrap_or(false)
    }

    /// Test-only: override a single tool's cached availability so render-path
    /// tests can prove they read the cache rather than resolving live.
    #[cfg(test)]
    pub(crate) fn set_tool_launchable(&mut self, tool_id: &'static str, launchable: bool) {
        self.launch_availability.insert(tool_id, launchable);
    }

    // --- snapshot refresh lifecycle ---

    pub fn request_refresh(&mut self) {
        if self.refreshing {
            return;
        }
        self.refreshing = true;
        self.log_event("Refresh requested (background thread)");

        let tx = self.tx.clone();
        let cfg = self.config.clone();

        std::thread::spawn(move || {
            let snapshot = build_snapshot(&cfg);
            let _ = tx.send(snapshot);
        });
    }

    pub fn apply_snapshot(&mut self, snapshot: OpsSnapshot) {
        self.snapshot = snapshot;
        self.refreshing = false;
        let mut names: Vec<String> = self.snapshot.adapter_health.keys().cloned().collect();
        names.sort();
        self.adapter_names = names;
        self.keep_selected_adapter_visible();
        self.log_event("Snapshot updated from adapter probes");
    }

    // --- activity log and display toggles ---

    pub fn log_event(&mut self, msg: impl Into<String>) {
        self.recent_events.push_back(msg.into());
        if self.recent_events.len() > 8 {
            self.recent_events.pop_front();
        }
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub(crate) fn clear_toast(&mut self) {
        self.toast = None;
    }
}
