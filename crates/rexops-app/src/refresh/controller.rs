//! Refresh-lifecycle controller.
//!
//! Wraps the low-level [`spawn_refresh`](super::spawn_refresh) driver with the
//! small amount of state a long-lived front-end needs to drive *repeated*
//! refreshes: the send side of the snapshot channel, the once-captured piped
//! stdin, and the single-flight guard. The driver itself (building a snapshot on
//! a worker thread, the panic fallback) stays in the parent module.

use std::sync::mpsc::Sender;

use rexops_core::{AppConfig, OpsSnapshot};

use super::spawn_refresh;

/// Drives repeated background refreshes for a long-lived front-end.
///
/// Owns the send side of the snapshot channel, the stdin captured once at
/// startup, and the single-flight guard. The front-end keeps the *receiver* and
/// drains it on its own loop, calling [`mark_applied`](Self::mark_applied) when a
/// snapshot lands — this controller deliberately does not touch the receiver, the
/// resulting snapshot, or any UI state. Config is NOT held here: it is passed into
/// [`request`](Self::request) so it can stay single-owned by the caller (where, in
/// the TUI, it is also bound to the launch-availability cache).
pub struct RefreshController {
    tx: Sender<OpsSnapshot>,
    /// stdin captured once at startup (a Workstate snapshot fed via a pipe), or
    /// `None`. Cloned into each worker so refreshes route the same bytes — stdin
    /// is consume-once and must not be re-read per refresh.
    piped_stdin: Option<String>,
    /// True between a spawned refresh and the snapshot landing. Guards against
    /// stacking overlapping refreshes (and drives any "refreshing…" UI).
    refreshing: bool,
}

impl RefreshController {
    /// Build the controller from the channel sender and the once-captured stdin.
    pub fn new(tx: Sender<OpsSnapshot>, piped_stdin: Option<String>) -> Self {
        Self {
            tx,
            piped_stdin,
            refreshing: false,
        }
    }

    /// Whether a refresh is currently in flight (spawned but its snapshot not yet
    /// applied). Front-ends read this to render a "refreshing…" indicator.
    pub fn is_refreshing(&self) -> bool {
        self.refreshing
    }

    /// Spawn a background refresh for `config`, unless one is already in flight.
    /// Returns `true` if a worker was spawned (so the caller can log it), `false`
    /// if a refresh was already running and this call was a no-op. The in-flight
    /// guard is set here and cleared by [`mark_applied`](Self::mark_applied) when
    /// the snapshot arrives — the panic-catch in [`spawn_refresh`] guarantees a
    /// snapshot always arrives, so the guard can never wedge.
    pub fn request(&mut self, config: &AppConfig) -> bool {
        if self.refreshing {
            return false;
        }
        self.refreshing = true;
        spawn_refresh(self.tx.clone(), config.clone(), self.piped_stdin.clone());
        true
    }

    /// Clear the in-flight guard: a snapshot has been received and applied. Safe
    /// to call unconditionally on every snapshot (idempotent when already clear).
    pub fn mark_applied(&mut self) {
        self.refreshing = false;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use rexops_core::AdapterConfig;

    use super::*;

    /// A config whose adapters are all disabled, so `request` spawns a worker
    /// that builds an empty snapshot fast (no real probes) and delivers it.
    fn all_disabled_config() -> AppConfig {
        let mut config = AppConfig::default();
        for name in ["bulwark", "system", "workstate"] {
            config.adapters.insert(
                name.to_owned(),
                AdapterConfig {
                    enabled: false,
                    ..Default::default()
                },
            );
        }
        config
    }

    #[test]
    fn controller_starts_idle() {
        let (tx, _rx) = mpsc::channel();
        let ctrl = RefreshController::new(tx, None);
        assert!(
            !ctrl.is_refreshing(),
            "a fresh controller is not refreshing"
        );
    }

    #[test]
    fn request_sets_in_flight_and_collapses_overlapping_requests() {
        let (tx, rx) = mpsc::channel();
        let mut ctrl = RefreshController::new(tx, None);
        let config = all_disabled_config();

        assert!(ctrl.request(&config), "first request must spawn a refresh");
        assert!(ctrl.is_refreshing(), "request must set the in-flight guard");
        // A second request while one is in flight must be a no-op (single-flight).
        assert!(
            !ctrl.request(&config),
            "an overlapping request must not spawn a second worker"
        );

        // The one spawned worker still delivers exactly one snapshot.
        let snap = match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(snap) => snap,
            Err(err) => panic!("the spawned refresh must deliver a snapshot: {err}"),
        };
        assert!(!snap.panicked);
    }

    #[test]
    fn mark_applied_clears_the_guard_and_re_enables_request() {
        let (tx, rx) = mpsc::channel();
        let mut ctrl = RefreshController::new(tx, None);
        let config = all_disabled_config();

        assert!(ctrl.request(&config));
        ctrl.mark_applied();
        assert!(
            !ctrl.is_refreshing(),
            "mark_applied must clear the in-flight guard"
        );
        // With the guard cleared, a fresh request spawns again.
        assert!(
            ctrl.request(&config),
            "after mark_applied a new request must spawn"
        );
        // Drain both workers' snapshots so neither thread outlives the test on a
        // live channel.
        for _ in 0..2 {
            let _ = rx.recv_timeout(Duration::from_secs(5));
        }
    }
}
