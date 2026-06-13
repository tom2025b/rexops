//! Owned TUI state, its constructor, and its lifecycle helpers
//! (background snapshot refresh, the activity log, display toggles).

use std::collections::VecDeque;
use std::sync::mpsc;

use rexops_app::{Availability, JobManager, RefreshController};
use rexops_core::{AppConfig, OpsSnapshot};

use super::Screen;
use crate::commands::PendingAction;

pub struct App {
    pub snapshot: OpsSnapshot,
    pub show_help: bool,
    pub current_screen: Screen,
    pub adapter_names: Vec<String>,
    /// Cache of `adapter_names` narrowed by `filter`, read on the render hot
    /// path so it never allocates per frame. Rebuilt by
    /// `recompute_filtered_names` whenever `adapter_names` (in `apply_snapshot`)
    /// or `filter` (via the selection-reconcile helpers) changes. Read it
    /// through `filtered_adapter_names()`.
    pub(crate) filtered_names: Vec<String>,
    pub selected_adapter: Option<String>,
    pub filter: String,
    /// Whether the inline filter on a filter screen is actively capturing
    /// keystrokes. Entered with `/`, exited with Enter/Esc. While set, the
    /// keymap runs in `Text` mode so every character (including bound command
    /// letters like `q`/`r`/digits) types into the filter instead of firing a
    /// command — the same text-input contract the palette has. Only meaningful
    /// on a `filter_screen()`; always cleared when leaving one.
    pub filtering: bool,
    pub selected_tool: usize,
    pub pending_action: Option<PendingAction>,
    pub palette_open: bool,
    pub palette_query: String,
    pub palette_selected: usize,
    /// Background-job state machine (rexops-app): owns the one job slot, its
    /// output buffer + scrollback, the last outcome, and the bounded history. The
    /// pure transitions live in the manager; App keeps only the UI reactions to
    /// its results (screen switch, toast, activity log, refresh). The render path
    /// reads `app.jobs.job` / `.output` / `.history` directly.
    pub jobs: JobManager,
    pub toast: Option<(String, suite_ui::ToastKind)>,
    /// Private so it can only be mutated through `modify_config`, which keeps the
    /// `availability` cache coherent. Read it via `config()`.
    config: AppConfig,
    pub recent_events: VecDeque<String>,
    /// Launch-availability service (rexops-app): owns the per-tool config+PATH
    /// resolvability cache the render path reads instead of shelling out to
    /// `which` every frame. Its coherence with `config` is enforced structurally:
    /// `config` is private and the only write path (`modify_config`) refreshes
    /// this — config can never change without the cache being rebuilt. Live
    /// adapter health is NOT cached here; it is passed in from `snapshot` at each
    /// query (see the delegating helpers below).
    availability: Availability,
    /// Snapshot-refresh controller (rexops-app): owns the send side of the
    /// refresh channel, the once-captured piped stdin, and the in-flight guard.
    /// App keeps the *receiver* in the runtime loop and the snapshot itself here;
    /// the controller only drives spawning and the guard. Reached through
    /// `request_refresh` / `is_refreshing`; config is passed in (it stays owned by
    /// App, bound to the availability cache) rather than duplicated in here.
    refresh: RefreshController,
}

impl App {
    /// Build the app state. `piped_stdin` is the snapshot blob captured once at
    /// startup (or `None`); it is the only place stdin is read, so refresh
    /// threads can clone it instead of re-reading the consume-once pipe.
    pub fn new(
        tx: mpsc::Sender<OpsSnapshot>,
        config: AppConfig,
        piped_stdin: Option<String>,
    ) -> Self {
        // Build the availability service from the config before it is moved in;
        // `Availability::new` populates the resolvability cache, so there is no
        // separate refresh step here. The refresh controller takes the channel
        // sender and the once-captured stdin.
        let availability = Availability::new(&config);
        let refresh = RefreshController::new(tx, piped_stdin);
        Self {
            snapshot: OpsSnapshot::new(),
            show_help: false,
            current_screen: Screen::default(),
            adapter_names: Vec::new(),
            filtered_names: Vec::new(),
            selected_adapter: None,
            filter: String::new(),
            filtering: false,
            selected_tool: 0,
            pending_action: None,
            palette_open: false,
            palette_query: String::new(),
            palette_selected: 0,
            jobs: JobManager::default(),
            toast: None,
            config,
            recent_events: VecDeque::from(["TUI started".to_owned()]),
            availability,
            refresh,
        }
    }

    // --- config access (read via `config`; mutate only via `modify_config`) ---

    /// Read-only view of the live config. All read sites go through here so the
    /// field can stay private and the cache-coherence invariant can be enforced.
    pub(crate) fn config(&self) -> &AppConfig {
        &self.config
    }

    /// Mutate the config in place, then refresh the availability cache.
    /// `config` is private with no other writer, so this is the ONLY way config
    /// changes — the cache can never drift from it.
    ///
    /// Today config is set once at construction and only read afterwards, so the
    /// sole caller is tests (hence `#[cfg(test)]`). When a production config
    /// reload lands, drop the gate: this is already the coherent mutation path
    /// it must route through, and the field's privacy is what forces it to.
    #[cfg(test)]
    pub(crate) fn modify_config(&mut self, f: impl FnOnce(&mut AppConfig)) {
        f(&mut self.config);
        self.availability.refresh(&self.config);
    }

    // --- launch availability (delegated to the rexops-app service) ---
    //
    // The resolvability cache and the launchable/available/tag logic live in
    // `rexops_app::Availability`. These thin wrappers feed it the one input it
    // does not hold — live adapter health from `self.snapshot` — and keep the
    // `pub(crate)` names the TUI call sites already use.

    /// Whether a catalog tool's command RESOLVES — the cheap, snapshot-independent
    /// half of launchability. Prefer `is_tool_available` at decision points; it
    /// also folds in live adapter health.
    pub(crate) fn is_tool_launchable(&self, tool_id: &str) -> bool {
        self.availability.is_launchable(tool_id)
    }

    /// Live adapter health for a catalog tool from the current snapshot, or
    /// `Unknown` if it hasn't been probed yet (e.g. before the first refresh).
    pub(crate) fn tool_health(&self, tool_id: &str) -> rexops_core::AdapterHealth {
        self.snapshot
            .adapter_health
            .get(tool_id)
            .copied()
            .unwrap_or(rexops_core::AdapterHealth::Unknown)
    }

    /// Whether a tool should be offered for launch RIGHT NOW: command resolves
    /// AND the adapter is not `Unavailable`. The health rule (Unknown/Degraded
    /// stay launchable, only Unavailable blocks) lives in the service.
    pub(crate) fn is_tool_available(&self, tool_id: &str) -> bool {
        self.availability
            .is_available(tool_id, self.tool_health(tool_id))
    }

    /// The 3-state availability verdict for a catalog tool — the single source of
    /// truth shared by every run surface (Launcher rows + command palette) so
    /// they can never disagree. Returns the domain enum; front-end wording lives
    /// in `crate::tools::availability_label`.
    pub(crate) fn availability_tag(&self, tool_id: &str) -> rexops_app::AvailabilityTag {
        self.availability.tag(tool_id, self.tool_health(tool_id))
    }

    /// Test-only: override a single tool's cached resolvability so render-path
    /// tests can prove they read the cache rather than resolving live.
    #[cfg(test)]
    pub(crate) fn set_tool_launchable(&mut self, tool_id: &'static str, launchable: bool) {
        self.availability.set_launchable(tool_id, launchable);
    }

    // --- snapshot refresh lifecycle (controller owns the channel + guard) ---

    /// Whether a background refresh is in flight. Read on the render path to show
    /// the "refreshing…" indicator. Delegates to the controller's guard.
    pub fn is_refreshing(&self) -> bool {
        self.refresh.is_refreshing()
    }

    /// Kick off a background refresh (no-op if one is already running). The
    /// controller owns the channel + in-flight guard and does the spawn; App only
    /// passes its config (kept here, bound to the availability cache) and logs the
    /// request when one was actually started.
    pub fn request_refresh(&mut self) {
        if self.refresh.request(&self.config) {
            self.log_event("Refresh requested (background thread)");
        }
    }

    pub fn apply_snapshot(&mut self, snapshot: OpsSnapshot) {
        self.snapshot = snapshot;
        // Clear the controller's in-flight guard now the snapshot has landed.
        self.refresh.mark_applied();
        let mut names: Vec<String> = self
            .snapshot
            .adapter_health
            .keys()
            .map(|id| id.as_str().to_owned())
            .collect();
        names.sort();
        self.adapter_names = names;
        self.keep_selected_adapter_visible();
        if self.snapshot.panicked {
            self.log_event("Refresh failed: an adapter probe panicked (results may be empty)");
        } else {
            self.log_event("Snapshot updated from adapter probes");
        }
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

    /// How the keymap should interpret the next keypress. `Text` when a text
    /// field is focused (the command palette today) so printable keys — including
    /// bound command letters like `q`/`r`/digits — become literal input; else
    /// `Navigation`, where the global command bindings apply. The runtime passes
    /// this into `keymap::handle_key`. (A pending confirmation is its own modal
    /// gate in `on_action` and is not a text field, so it stays `Navigation`.)
    pub(crate) fn input_mode(&self) -> crate::input::keymap::InputMode {
        use crate::input::keymap::InputMode;
        if self.palette_open || self.filtering {
            InputMode::Text
        } else {
            InputMode::Navigation
        }
    }

    pub(crate) fn clear_toast(&mut self) {
        self.toast = None;
    }
}
