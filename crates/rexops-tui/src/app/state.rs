//! Owned TUI state, its constructor, and its lifecycle helpers
//! (background snapshot refresh, the activity log, display toggles).

use std::collections::VecDeque;
use std::sync::mpsc;

use rexops_app::build_snapshot;
use rexops_core::{AppConfig, OpsSnapshot};

use super::Screen;
use crate::commands::PendingAction;
use crate::jobs::{JobHandle, JobOutput, JobRecord, LastOutcome};

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
    pub job_history: Vec<JobRecord>,
    pub toast: Option<(String, suite_ui::ToastKind)>,
    pub config: AppConfig,
    pub recent_events: Vec<String>,
    pub(crate) tx: mpsc::Sender<OpsSnapshot>,
}

impl App {
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
        let mut names: Vec<String> = self.snapshot.adapter_health.keys().cloned().collect();
        names.sort();
        self.adapter_names = names;
        self.keep_selected_adapter_visible();
        self.log_event("Snapshot updated from adapter probes");
    }

    // --- activity log and display toggles ---

    pub fn log_event(&mut self, msg: impl Into<String>) {
        self.recent_events.push(msg.into());
        if self.recent_events.len() > 8 {
            self.recent_events.remove(0);
        }
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub(crate) fn clear_toast(&mut self) {
        self.toast = None;
    }
}
