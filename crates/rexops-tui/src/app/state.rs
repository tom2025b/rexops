//! Owned TUI state, its constructor, and its lifecycle helpers
//! (background snapshot refresh, the activity log, display toggles).

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc;

use rexops_app::build_snapshot_with_piped;
use rexops_core::{AppConfig, OpsSnapshot};

use super::heartbeat::HeartbeatLog;
use super::Screen;
use crate::commands::PendingAction;
use crate::jobs::{JobHandle, JobOutput, JobRecord, LastOutcome};
use crate::tools;

pub struct App {
    pub snapshot: OpsSnapshot,
    pub refreshing: bool,
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
    /// The cockpit card currently focused, keyed by component `id` (NOT an index,
    /// so focus survives a refresh that reorders/adds components). `None` before
    /// the first snapshot or when no card is visible. Moved by the cockpit nav
    /// helpers; read on the cockpit render path.
    pub selected_component: Option<String>,
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
    pub job: Option<JobHandle>,
    pub job_output: VecDeque<JobOutput>,
    /// Jobs-output scrollback offset, in lines from the BOTTOM. `0` means
    /// "follow the bottom" — newest output stays visible as it streams. A
    /// positive value pins the view that many lines up and pauses auto-follow
    /// until the user scrolls back to the bottom. Clamped to the buffer so it can
    /// never point past either end.
    pub jobs_scroll: usize,
    pub last_job: Option<String>,
    pub last_outcome: Option<LastOutcome>,
    pub job_history: VecDeque<JobRecord>,
    pub toast: Option<(String, suite_ui::ToastKind)>,
    /// Private so it can only be mutated through `set_config` / `modify_config`,
    /// which keep `launch_availability` coherent. Read it via `config()`.
    config: AppConfig,
    pub recent_events: VecDeque<String>,
    /// Per-catalog-tool launch availability, derived from `config` + PATH.
    /// The Launcher redraws every ~100ms; resolving availability there would
    /// shell out to `which` on every frame, so the render path reads this cache
    /// instead. Its coherence with `config` is enforced structurally: `config`
    /// is private and every write path (`set_config`, `modify_config`) refreshes
    /// this — there is no way to change config without recomputing availability.
    launch_availability: HashMap<&'static str, bool>,
    /// Per-component heartbeat ring buffer (latency samples for the sparkline).
    pub(crate) heartbeats: HeartbeatLog,
    /// The piped stdin captured ONCE at startup (a Workstate snapshot fed in via
    /// a pipe), or `None` when stdin was a terminal / empty. Cloned into every
    /// refresh thread so each refresh routes the same bytes. stdin is
    /// consume-once: reading it per refresh would drain it after the first probe
    /// (silent data-source flip) or block forever on a pipe that never closes —
    /// see `rexops_app::build_snapshot`. Capturing it here is what avoids both.
    piped_stdin: Option<String>,
    pub(crate) tx: mpsc::Sender<OpsSnapshot>,
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
        let mut app = Self {
            snapshot: OpsSnapshot::new(),
            refreshing: false,
            show_help: false,
            current_screen: Screen::default(),
            adapter_names: Vec::new(),
            filtered_names: Vec::new(),
            selected_adapter: None,
            selected_component: None,
            filter: String::new(),
            filtering: false,
            selected_tool: 0,
            pending_action: None,
            palette_open: false,
            palette_query: String::new(),
            palette_selected: 0,
            job: None,
            job_output: VecDeque::new(),
            jobs_scroll: 0,
            last_job: None,
            last_outcome: None,
            job_history: VecDeque::new(),
            toast: None,
            config,
            recent_events: VecDeque::from(["TUI started".to_owned()]),
            launch_availability: HashMap::new(),
            heartbeats: HeartbeatLog::with_capacity(16),
            piped_stdin,
            tx,
        };
        app.refresh_launch_availability();
        app
    }

    // --- config access (read via `config`; mutate only via `modify_config`) ---

    /// Read-only view of the live config. All read sites go through here so the
    /// field can stay private and the cache-coherence invariant can be enforced.
    pub(crate) fn config(&self) -> &AppConfig {
        &self.config
    }

    /// Mutate the config in place, then refresh the launch-availability cache.
    /// `config` is private with no other writer, so this is the ONLY way config
    /// changes — `launch_availability` can never drift from it.
    ///
    /// Today config is set once at construction and only read afterwards, so the
    /// sole caller is tests (hence `#[cfg(test)]`). When a production config
    /// reload lands, drop the gate: this is already the coherent mutation path
    /// it must route through, and the field's privacy is what forces it to.
    #[cfg(test)]
    pub(crate) fn modify_config(&mut self, f: impl FnOnce(&mut AppConfig)) {
        f(&mut self.config);
        self.refresh_launch_availability();
    }

    // --- launch availability cache ---

    /// Recompute the cached launch availability for every catalog tool from the
    /// current config (and PATH). Private: config is changed only through
    /// `modify_config`, which calls this — so availability can never drift from
    /// config. The render path reads it via `is_tool_launchable`.
    fn refresh_launch_availability(&mut self) {
        self.launch_availability = tools::launchable()
            .iter()
            .map(|tool| {
                (
                    tool.id,
                    tools::resolve_launch_command(tool.id, self.config()).is_some(),
                )
            })
            .collect();
    }

    /// Whether a catalog tool's command RESOLVES — read from the cached
    /// config+PATH availability. This is the cheap, snapshot-independent half of
    /// launchability (computed once; see the cache docs above). Unknown ids
    /// (not in the catalog) read as not resolvable. Prefer `is_tool_available`
    /// at decision points — it also folds in live adapter health.
    pub(crate) fn is_tool_launchable(&self, tool_id: &str) -> bool {
        self.launch_availability
            .get(tool_id)
            .copied()
            .unwrap_or(false)
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

    /// Whether a tool should be offered for launch RIGHT NOW: its command must
    /// resolve (cached config+PATH) AND its adapter must not be `Unavailable`.
    ///
    /// Health is combined here, at the decision point, rather than baked into the
    /// cache — so the cheap once-computed resolvability cache survives and we just
    /// add a HashMap health lookup (no `which` per frame). `Unknown` and
    /// `Degraded` stay launchable on purpose: `Unknown` is the pre-probe state
    /// (blocking it would make every tool unlaunchable for the first moment after
    /// startup), and a `Degraded` tool is often exactly what you want to launch to
    /// inspect or fix it. Only `Unavailable` — binary gone or administratively
    /// disabled — blocks the launch.
    pub(crate) fn is_tool_available(&self, tool_id: &str) -> bool {
        use rexops_core::AdapterHealth;
        self.is_tool_launchable(tool_id) && self.tool_health(tool_id) != AdapterHealth::Unavailable
    }

    /// The 3-state availability tag for a catalog tool, the single source of
    /// truth shared by every run surface (the Launcher rows and the command
    /// palette) so they can never disagree about what's runnable:
    ///   • available            → "streams" (Background) / "interactive" (Foreground)
    ///   • resolvable but down   → "unavailable" (adapter health == Unavailable)
    ///   • not resolvable at all → "disabled"
    /// Returned without the leading "· " so each caller can frame it to taste.
    pub(crate) fn availability_tag(&self, tool_id: &str) -> &'static str {
        if self.is_tool_available(tool_id) {
            if tools::is_streamable(tool_id) {
                "streams"
            } else {
                "interactive"
            }
        } else if self.is_tool_launchable(tool_id) {
            "unavailable"
        } else {
            "disabled"
        }
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
        // Clone the stdin captured once at startup so this thread routes the same
        // bytes as every other refresh. We deliberately do NOT read stdin here:
        // it is consume-once, so a per-refresh read would drain it after the first
        // probe or block forever on a pipe that never closes (see the field doc).
        let piped = self.piped_stdin.clone();

        std::thread::spawn(move || {
            // `refreshing` is only ever cleared when a snapshot arrives over the
            // channel (apply_snapshot). If build_snapshot panicked, the thread
            // would unwind before sending, no snapshot would arrive, and the flag
            // would stay set forever — silently bricking `r`. Catch the unwind so
            // a panicking probe still delivers a snapshot and the flag always
            // clears. The fallback carries a NOTE (panicked_snapshot) so the crash
            // is visible on the Dashboard/log rather than reading as a normal
            // empty "nothing probed yet" state.
            let snapshot = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                build_snapshot_with_piped(&cfg, piped.as_deref())
            }))
            .unwrap_or_else(|_| Self::panicked_snapshot());
            let _ = tx.send(snapshot);
        });
    }

    /// The fallback snapshot delivered when an adapter probe panics mid-refresh.
    /// It is empty (no probe data survived the unwind) but carries a note so the
    /// failure surfaces in the Dashboard Messages pane / activity log instead of
    /// looking identical to a never-probed state — a silent crash is the worst
    /// outcome for an ops tool. Every other build_snapshot path already reports
    /// via notes; this keeps the panic path consistent.
    pub(crate) fn panicked_snapshot() -> OpsSnapshot {
        let mut snap = OpsSnapshot::new();
        snap.panicked = true;
        snap.add_note("refresh failed: an adapter probe panicked — partial/empty results");
        snap
    }

    pub fn apply_snapshot(&mut self, snapshot: OpsSnapshot) {
        self.snapshot = snapshot;
        self.refreshing = false;
        for (id, ms) in &self.snapshot.status_latency {
            self.heartbeats.record(id, *ms);
        }
        let mut names: Vec<String> = self
            .snapshot
            .adapter_health
            .keys()
            .map(|id| id.as_str().to_owned())
            .collect();
        names.sort();
        self.adapter_names = names;
        self.keep_selected_adapter_visible();
        self.keep_cockpit_selection_visible();
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
