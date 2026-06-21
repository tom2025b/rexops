//! models.rs — Core domain aggregates: RiskSummary, ReportSummary, OpsSnapshot, etc.
//!
//! These types are the "vocabulary" that the rest of RexOps uses to talk about
//! the observed state of the world. They are pure data
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
use crate::{AdapterHealth, Freshness};

/// High-level risk rollup derived from one or more adapter scans (e.g. Bulwark).
///
/// Adapters can contribute to the same counters so the dashboard has a unified
/// risk view.
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

/// Status of a background or long-running job.
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

/// A single component's RESOLVED status, ready to render. Produced by the app
/// layer's registry walk from a `Component` + a live probe; carries owned
/// strings (no `'static` borrow) so it serializes cleanly into the snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComponentStatus {
    pub id: String,
    pub name: String,
    pub group: String,
    pub maturity: String,
    pub health: AdapterHealth,
    /// Data freshness when the source is a feed; `None` for probe/host/planned.
    /// Populated by the registry walk but not yet rendered anywhere — the cockpit
    /// cards that read it land in Phase B. Until then it only surfaces in `--json`.
    pub freshness: Option<Freshness>,
    /// The one headline number for the card (e.g. "3/3 fresh", "1 crit 1 high").
    pub vital: Option<String>,
    /// Whether this component currently resolves to a launch command.
    pub launchable: bool,
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
    /// Keyed by the typed `AdapterId` so callers cannot accidentally look up a
    /// raw, unvalidated string and silently miss. `AdapterId` is
    /// `#[serde(transparent)]` over its inner string, so the JSON wire form is
    /// unchanged (a plain `{"bulwark": ...}` object).
    #[serde(default)]
    pub adapter_health: std::collections::HashMap<AdapterId, crate::AdapterHealth>,

    /// Current risk picture aggregated across adapters that contribute risk.
    #[serde(default)]
    pub risk: RiskSummary,

    /// Summaries of recent or relevant reports (not the full content).
    #[serde(default)]
    pub reports: Vec<ReportSummary>,

    /// Active or recent jobs such as scans or syncs, keyed by job id.
    ///
    /// NOTE: currently unpopulated — the live TUI job system tracks jobs in its
    /// own manager, not here. Left `String`-keyed deliberately: a job id is not
    /// an adapter id, so it should gain a dedicated `JobId` newtype (not reuse
    /// `AdapterId`) if/when this field is actually wired up.
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

    /// Structured scripts inventory from Workstate. Allows a dedicated Scripts
    /// screen to render favorites/recents without parsing notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripts: Option<crate::ScriptsInfo>,

    /// Structured tools inventory from Workstate. Enables a Tools screen showing
    /// ownership etc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<crate::ToolsInfo>,

    /// Structured findings from Workstate. The risk counts also feed `risk`
    /// above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub findings: Option<crate::FindingsInfo>,

    /// Full structured Workstate snapshot data when a supported-version snapshot
    /// was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workstate: Option<crate::WorkstateInfo>,

    /// Set to `true` when this snapshot was produced by the panic-recovery
    /// fallback (an adapter probe panicked mid-refresh). A typed flag rather than
    /// a note substring so callers can branch on it without string matching.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub panicked: bool,

    /// Resolved per-component statuses for the cockpit. Populated by the app
    /// layer's registry walk; empty until the first build.
    #[serde(default)]
    pub components: Vec<ComponentStatus>,

    /// Latency in milliseconds reported by each StatusCommand probe (keyed by
    /// component id). Only populated when the component's status binary reports
    /// a `latency_ms` field. Empty when no StatusCommand probes have run.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub status_latency: std::collections::HashMap<String, u64>,

    /// One-line detail string reported by each StatusCommand probe (keyed by
    /// component id). Always populated when the probe runs. Used as the card
    /// vital fallback when the heartbeat sparkline has no samples — notably for
    /// a Degraded/Unavailable probe where `latency_ms` is None and the heartbeat
    /// ring-buffer stays empty. Empty when no StatusCommand probes have run.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub status_detail: std::collections::HashMap<String, String>,
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
            scripts: None,
            tools: None,
            findings: None,
            workstate: None,
            panicked: false,
            components: Vec::new(),
            status_latency: std::collections::HashMap::new(),
            status_detail: std::collections::HashMap::new(),
        }
    }

    /// Record (or overwrite) the health for a given adapter.
    ///
    /// Call this while building the snapshot from live adapter probes.
    pub fn set_adapter_health(&mut self, id: &AdapterId, health: crate::AdapterHealth) {
        self.adapter_health.insert(id.clone(), health);
    }

    /// Look up the health of an adapter by id. Accepts anything that borrows as
    /// an `AdapterId` (the map is keyed by the typed id) so call sites read
    /// naturally: `snap.adapter_health_of(&id)`.
    pub fn adapter_health_of(&self, id: &AdapterId) -> Option<crate::AdapterHealth> {
        self.adapter_health.get(id).copied()
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

    /// Append a resolved component status (the registry walk calls this once per
    /// component, in table order).
    pub fn push_component(&mut self, status: ComponentStatus) {
        self.components.push(status);
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

/// Format epoch-millis as a human-readable UTC timestamp: `YYYY-MM-DD HH:MM:SS UTC`.
///
/// A raw epoch-millis integer (what the human `status` header used to print) is
/// unreadable to a person. RexOps deliberately stores timestamps as zero-dep
/// `u64` millis, so this does the civil-date conversion by hand (the standard
/// days-from-epoch algorithm) rather than pulling in `chrono`/`time` just to
/// render one line. The JSON output keeps the raw integer; only human views call
/// this.
#[must_use]
#[allow(clippy::cast_possible_wrap)] // days-since-epoch fits i64 for any real timestamp
pub fn format_unix_millis_utc(ms: u64) -> String {
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let (hour, min, sec) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{min:02}:{sec:02} UTC")
}

/// Convert days-since-Unix-epoch into a proleptic-Gregorian `(year, month, day)`.
/// Howard Hinnant's well-known `civil_from_days` algorithm — exact, branch-light,
/// and correct across all leap-year rules. Used only by [`format_unix_millis_utc`].
///
/// The casts are intrinsic to the algorithm and provably in range: `doe` ∈
/// [0, 146096] and `doy`/`mp`/`d`/`m` are the small bounded intermediates noted
/// inline, so none can wrap, truncate, or lose a sign.
#[allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation
)]
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::ids::AdapterId;
    use crate::AdapterHealth;

    #[test]
    fn new_snapshot_has_now_timestamp_and_is_available_false_when_empty() {
        let snap = OpsSnapshot::new();
        // Timestamp should be initialized to a plausible current epoch value.
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
    fn format_unix_millis_utc_renders_known_instants() {
        // The Unix epoch itself.
        assert_eq!(format_unix_millis_utc(0), "1970-01-01 00:00:00 UTC");
        // A known instant: 2026-06-15 05:16:01 UTC = 1_781_500_561_000 ms.
        // (Cross-checked against `date -u -d @1781500561`.)
        assert_eq!(
            format_unix_millis_utc(1_781_500_561_000),
            "2026-06-15 05:16:01 UTC"
        );
        // Sub-second millis are truncated to the second, not rounded.
        assert_eq!(
            format_unix_millis_utc(1_781_500_561_999),
            "2026-06-15 05:16:01 UTC"
        );
        // A leap-day instant exercises civil_from_days' leap handling:
        // 2024-02-29 12:00:00 UTC = 1_709_208_000_000 ms.
        assert_eq!(
            format_unix_millis_utc(1_709_208_000_000),
            "2024-02-29 12:00:00 UTC"
        );
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

        // Wire-format guarantee: keying adapter_health by the AdapterId newtype
        // (instead of String) must NOT change the JSON shape, because AdapterId
        // is #[serde(transparent)]. The key stays a bare string, so older
        // String-keyed snapshots deserialize unchanged.
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let health = &value["adapter_health"];
        assert!(
            health.get("bulwark").is_some(),
            "adapter_health must serialize with a bare string key, got: {health}"
        );
    }

    #[test]
    fn adapter_health_deserializes_from_legacy_string_keyed_json() {
        // A snapshot written before adapter_health was AdapterId-keyed (plain
        // string keys) must still load: proves the migration is backward
        // compatible on the wire.
        let legacy = r#"{
            "generated_at_ms": 1700000000001,
            "adapter_health": {"bulwark": "healthy", "system": "degraded"}
        }"#;
        let snap: OpsSnapshot = serde_json::from_str(legacy).unwrap();
        let bul = AdapterId::new("bulwark").unwrap();
        assert_eq!(snap.adapter_health_of(&bul), Some(AdapterHealth::Healthy));
        assert_eq!(snap.adapter_health.len(), 2);
    }

    #[test]
    fn ops_snapshot_carries_resolved_component_statuses() {
        use crate::ComponentStatus;
        let mut snap = OpsSnapshot::new();
        assert!(snap.components.is_empty(), "new snapshot has no components");

        snap.push_component(ComponentStatus {
            id: "bulwark".to_owned(),
            name: "Bulwark".to_owned(),
            group: "field tool".to_owned(),
            maturity: "live".to_owned(),
            health: AdapterHealth::Healthy,
            freshness: None,
            vital: Some("1 crit 1 high".to_owned()),
            launchable: true,
        });

        assert_eq!(snap.components.len(), 1);
        assert_eq!(snap.components[0].id, "bulwark");
        assert_eq!(snap.components[0].health, AdapterHealth::Healthy);

        // Round-trips through serde so the CLI `--json` view can emit it.
        let json = serde_json::to_string(&snap).expect("serialize");
        let back: OpsSnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.components.len(), 1);
        assert_eq!(back.components[0].vital.as_deref(), Some("1 crit 1 high"));
    }
}
