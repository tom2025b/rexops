//! Background snapshot-refresh: the low-level worker driver plus the
//! refresh-lifecycle controller.
//!
//! Long-lived front-ends build snapshots on a worker thread without re-reading
//! stdin and without letting panicking adapter probes strand the caller in a
//! permanent "refreshing" state. This module is the raw driver — [`spawn_refresh`]
//! plus the [`panicked_snapshot`] fallback; the [`RefreshController`] in the
//! `controller` submodule wraps it with the per-front-end state (channel sender,
//! captured stdin, in-flight guard) needed to drive repeated refreshes.

mod controller;

pub use controller::RefreshController;

use std::sync::mpsc::Sender;
use std::thread;

use rexops_core::{AppConfig, OpsSnapshot};

use crate::build_snapshot_with_piped;

/// The fallback snapshot delivered when an adapter probe panics mid-refresh.
///
/// It is empty because no probe data survived the unwind, but it carries a
/// typed flag and a note so the failure is visible to the caller.
pub fn panicked_snapshot() -> OpsSnapshot {
    let mut snap = OpsSnapshot::new();
    snap.panicked = true;
    snap.add_note("refresh failed: an adapter probe panicked — partial/empty results");
    snap
}

/// Spawn a background refresh from a captured config and optional piped input.
///
/// `piped` is the stdin captured once by the caller at startup. This function
/// never reads stdin directly, so repeat refreshes see the same input and cannot
/// block on a consume-once stream.
pub fn spawn_refresh(tx: Sender<OpsSnapshot>, config: AppConfig, piped: Option<String>) {
    thread::spawn(move || {
        let snapshot = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            build_snapshot_with_piped(&config, piped.as_deref())
        }))
        .unwrap_or_else(|_| panicked_snapshot());
        let _ = tx.send(snapshot);
    });
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use rexops_core::AdapterConfig;

    use super::*;

    #[test]
    fn panicked_snapshot_is_flagged_and_noted() {
        let snap = panicked_snapshot();
        assert!(snap.adapter_health.is_empty());
        assert!(snap.panicked, "fallback must set the panicked flag");
        assert!(
            snap.notes
                .iter()
                .any(|note| note.contains("an adapter probe panicked")),
            "fallback must carry a visible note, got: {:?}",
            snap.notes
        );
    }

    #[test]
    fn spawn_refresh_delivers_a_snapshot_over_the_channel() {
        let (tx, rx) = mpsc::channel();
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

        spawn_refresh(tx, config, None);

        let snapshot = match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(snapshot) => snapshot,
            Err(err) => panic!("a snapshot must arrive: {err}"),
        };
        assert!(!snapshot.panicked, "a clean build must not be flagged");
    }
}
