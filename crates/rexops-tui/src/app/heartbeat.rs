//! A bounded, transient log of per-component liveness samples — the data behind
//! the cockpit's heartbeat sparkline. In-memory only (cleared on exit), keyed by
//! component id so any StatusCommand tool can have a heartbeat, not just Pulse.

use std::collections::{HashMap, VecDeque};

/// Recent latency samples per component id, capped per id.
#[derive(Debug, Default)]
pub struct HeartbeatLog {
    cap: usize,
    by_id: HashMap<String, VecDeque<u64>>,
}

impl HeartbeatLog {
    /// A log holding up to `cap` samples per component.
    pub fn with_capacity(cap: usize) -> Self {
        HeartbeatLog {
            cap: cap.max(1),
            by_id: HashMap::new(),
        }
    }

    /// Append one sample for `id`, dropping the oldest past capacity.
    pub fn record(&mut self, id: &str, latency_ms: u64) {
        let q = self.by_id.entry(id.to_owned()).or_default();
        q.push_back(latency_ms);
        while q.len() > self.cap {
            q.pop_front();
        }
    }

    /// Samples for `id`, oldest→newest (empty if none).
    pub fn samples(&self, id: &str) -> Vec<u64> {
        self.by_id
            .get(id)
            .map(|q| q.iter().copied().collect())
            .unwrap_or_default()
    }

    /// The most recent sample for `id`, if any.
    pub fn latest(&self, id: &str) -> Option<u64> {
        self.by_id.get(id).and_then(|q| q.back().copied())
    }
}

// Learning Notes
// - Per-id `VecDeque` with a hard cap: O(1) push, bounded memory, no persistence.
// - Default capacity (16) is chosen in `App`; this type stays policy-free.
// - Phase E: `samples` and `latest` are consumed by the cockpit card vital (cockpit.rs)
//   and the drill-down heartbeat section (cockpit_detail.rs). The previous
//   `#[allow(dead_code)]` guard was removed when those call sites landed.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_in_order_and_reports_latest() {
        let mut log = HeartbeatLog::with_capacity(16);
        log.record("pulse", 5);
        log.record("pulse", 8);
        assert_eq!(log.samples("pulse"), vec![5, 8]);
        assert_eq!(log.latest("pulse"), Some(8));
    }

    #[test]
    fn caps_at_capacity_dropping_oldest() {
        let mut log = HeartbeatLog::with_capacity(3);
        for s in [1, 2, 3, 4, 5] {
            log.record("pulse", s);
        }
        assert_eq!(log.samples("pulse"), vec![3, 4, 5]);
    }

    #[test]
    fn unknown_id_is_empty() {
        let log = HeartbeatLog::with_capacity(4);
        assert!(log.samples("nope").is_empty());
        assert_eq!(log.latest("nope"), None);
    }
}
