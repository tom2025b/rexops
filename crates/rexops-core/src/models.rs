//! models.rs — Core domain aggregates: RiskSummary, ReportSummary, OpsSnapshot, etc.
//!
//! These types are the "vocabulary" that the rest of RexOps (CLI, TUI, future
//! app) uses to talk about the observed state of the world. They are pure data
//! — no methods that perform I/O or call adapters.
//!
//! OpsSnapshot is the central object: a point-in-time, serializable picture
//! built by lifting one or more AdapterOutput<T> values plus any local state.
//!
//! Design constraints:
//! - All fields are serializable (serde).
//! - All vectors use #[serde(default)] so that missing keys in JSON/YAML
//!   become empty vecs instead of parse failures (defensive).
//! - Timestamps are u64 millis since Unix epoch for zero-dependency simplicity.
//! - Keep the module small; split if it exceeds ~200 LOC.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::ids::AdapterId;
use crate::AdapterHealth;

/// High-level risk rollup derived from one or more adapter scans (e.g. Bulwark).
///
/// In Phase 1 this is populated from BulwarkScanResult. Later adapters can
/// contribute to the same counters so the dashboard has a unified "risk" view.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RiskSummary {
    #[serde(default)]
    pub critical: u32,
    #[serde(default)]
    pub high: u32,
    #[serde(default)]
    pub medium: u32,
    #[serde(default)]
    pub low: u32,
    #[serde(default)]
    pub info: u32,

    /// Total findings across all severities.
    #[serde(default)]
    pub total_findings: u32,

    /// Whether any adapter indicated that the content/action should be blocked.
    #[serde(default)]
    pub should_block: bool,

    /// Highest severity observed (if any). Serialized as snake_case string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_severity: Option<String>,
}

impl RiskSummary {
    /// Create an empty summary (all zeros, no block).
    pub fn new() -> Self {
        Self::default()
    }

    /// Sum another summary into self (used when merging multiple adapter results).
    pub fn merge(&mut self, other: &Self) {
        self.critical += other.critical;
        self.high += other.high;
        self.medium += other.medium;
        self.low += other.low;
        self.info += other.info;
        self.total_findings += other.total_findings;
        self.should_block = self.should_block || other.should_block;

        // Naive max-severity: prefer any non-None; caller can refine.
        if self.max_severity.is_none() {
            self.max_severity.clone_from(&other.max_severity);
        }
    }
}

/// Status of a background or long-running job (placeholder for future use).
///
/// Included now so that snapshots and UIs have a stable place to surface
/// "something is still computing" without inventing ad-hoc strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Idle,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

/// A lightweight summary of a persisted or in-memory report.
///
/// The full report payload lives elsewhere (on disk, in Bulwark logs, etc.).
/// This is what appears in inventory lists and snapshot indexes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReportSummary {
    /// Stable identifier for the report (e.g. uuid or timestamped name).
    pub id: String,

    /// When the report was produced (millis since epoch).
    pub generated_at_ms: u64,

    /// Short human title or description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Risk rollup that was current when the report was taken.
    #[serde(default)]
    pub risk: RiskSummary,

    /// Which adapter produced the underlying data (if applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_adapter: Option<String>,
}

/// The single point-in-time aggregate that higher layers query.
///
/// OpsSnapshot is deliberately "wide but shallow": it contains enough to render
/// a dashboard, answer "is everything healthy?", and drive detail panes,
/// without embedding every raw finding or full adapter output (those can be
/// fetched on demand or stored in a detail cache).
///
/// Build it by calling constructors / merge helpers from adapter results.
/// Never mutate after construction in normal flow — treat as immutable view.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpsSnapshot {
    /// When this snapshot was assembled (millis since Unix epoch).
    pub generated_at_ms: u64,

    /// Health of every adapter we know about at snapshot time.
    /// Keyed by adapter id for easy lookup in TUI/CLI.
    #[serde(default)]
    pub adapter_health: std::collections::HashMap<String, crate::AdapterHealth>,

    /// Current risk picture aggregated across adapters that contribute risk.
    #[serde(default)]
    pub risk: RiskSummary,

    /// Summaries of recent or relevant reports (not the full content).
    #[serde(default)]
    pub reports: Vec<ReportSummary>,

    /// Active or recent jobs (future: scans, syncs, etc.).
    #[serde(default)]
    pub jobs: std::collections::HashMap<String, JobStatus>,

    /// Optional free-form notes or degraded-state explanations.
    /// TUI renders these as banners.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,

    /// Structured system information from the SystemAdapter (if successfully
    /// probed). Populated in build_snapshot so screens can render nicely
    /// without re-parsing notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<crate::SystemInfo>,

    /// Structured scriptvault information from the ScriptVaultAdapter (if
    /// successfully probed). Allows a dedicated Scripts screen to render
    /// favorites/recents without parsing notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scriptvault: Option<crate::ScriptVaultInfo>,

    /// Structured toolfoundry information from the ToolFoundryAdapter (if
    /// successfully probed). Enables a Tools screen showing ownership etc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolfoundry: Option<crate::ToolFoundryInfo>,

    /// Structured Bulwark scan-export data from the BulwarkFeedAdapter (if a
    /// supported-version feed was read). The risk counts also feed `risk` above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bulwark: Option<crate::BulwarkScanInfo>,

    /// Structured Workstate snapshot data from the WorkstateAdapter (if a
    /// supported-version feed was read). Per-project repo health; contributes
    /// notes and a structured field only — no risk (the contract has none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workstate: Option<crate::WorkstateInfo>,
}

impl OpsSnapshot {
    /// Create a fresh empty snapshot with current timestamp.
    pub fn new() -> Self {
        Self {
            generated_at_ms: current_unix_millis(),
            adapter_health: std::collections::HashMap::new(),
            risk: RiskSummary::new(),
            reports: Vec::new(),
            jobs: std::collections::HashMap::new(),
            notes: Vec::new(),
            system: None,
            scriptvault: None,
            toolfoundry: None,
            bulwark: None,
            workstate: None,
        }
    }

    /// Record (or overwrite) the health for a given adapter.
    ///
    /// Call this while building the snapshot from live adapter probes.
    pub fn set_adapter_health(&mut self, id: &AdapterId, health: crate::AdapterHealth) {
        self.adapter_health.insert(id.as_str().to_owned(), health);
    }

    /// Merge a risk contribution into the snapshot (e.g. from a Bulwark scan).
    pub fn merge_risk(&mut self, other: &RiskSummary) {
        self.risk.merge(other);
    }

    /// Add a note (e.g. "bulwark adapter degraded: old version").
    /// Duplicates are not deduped — caller should avoid spamming.
    pub fn add_note(&mut self, note: impl Into<String>) {
        self.notes.push(note.into());
    }

    /// Return true if any adapter is in a state that allows work.
    pub fn any_adapter_available(&self) -> bool {
        self.adapter_health
            .values()
            .any(AdapterHealth::is_available)
    }
}

impl Default for OpsSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

/// Return current time as milliseconds since Unix epoch.
/// Uses SystemTime; on the very rare clock-before-epoch system we clamp to 0.
///
/// We intentionally truncate u128 millis to u64. This is safe for all
/// practical timestamps (u64 millis lasts until year ~584 billion).
#[allow(clippy::cast_possible_truncation)]
fn current_unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

// Learning Notes:
// - Using HashMap<String, Health> (with String keys) is pragmatic for serde
//   and for TUI that wants to iterate "all adapters" without knowing the
//   full set of AdapterId values at compile time. We still use AdapterId for
//   the *set* API to keep call sites type-safe.
// - RiskSummary::merge is a tiny pure function — easy to test and to extend
//   when new adapters contribute new risk dimensions.
// - We deliberately avoid embedding full AdapterOutput<T> or raw findings
//   inside OpsSnapshot. The snapshot is for "at a glance"; detail panes ask
//   the adapter again or read a cached full result. This keeps memory and
//   wire size small.
// - current_unix_millis is a private helper so that tests can later inject
//   deterministic times if needed (by adding a builder that takes a ts).
// - JobStatus and ReportSummary are present even in Phase 1 so that the
//   shape of snapshots is stable; empty collections serialize cleanly.

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::ids::AdapterId;
    use crate::AdapterHealth;

    #[test]
    fn new_snapshot_has_now_timestamp_and_is_available_false_when_empty() {
        let snap = OpsSnapshot::new();
        // timestamp should be "recent" (not zero, not in future by much)
        assert!(snap.generated_at_ms > 1_700_000_000_000); // after 2023
        assert!(!snap.any_adapter_available());
        assert!(snap.notes.is_empty());
    }

    #[test]
    fn set_adapter_health_and_query_available() {
        let mut snap = OpsSnapshot::new();
        let bul = AdapterId::new("bulwark").unwrap();
        snap.set_adapter_health(&bul, AdapterHealth::Healthy);
        assert!(snap.any_adapter_available());
        assert_eq!(snap.adapter_health.len(), 1);
    }

    #[test]
    fn risk_merge_adds_counts_and_or_s_block() {
        let mut a = RiskSummary {
            critical: 1,
            total_findings: 1,
            should_block: false,
            ..Default::default()
        };
        let b = RiskSummary {
            high: 2,
            total_findings: 2,
            should_block: true,
            ..Default::default()
        };
        a.merge(&b);
        assert_eq!(a.critical, 1);
        assert_eq!(a.high, 2);
        assert_eq!(a.total_findings, 3);
        assert!(a.should_block);
    }

    #[test]
    fn snapshot_and_risk_are_serde_roundtrippable() {
        let mut snap = OpsSnapshot::new();
        let id = AdapterId::new("bulwark").unwrap();
        snap.set_adapter_health(&id, AdapterHealth::Degraded);
        snap.merge_risk(&RiskSummary {
            medium: 1,
            total_findings: 1,
            ..Default::default()
        });
        snap.add_note("degraded because version probe returned empty".to_owned());

        let json = serde_json::to_string(&snap).unwrap();
        let snap2: OpsSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap, snap2);
    }
}
