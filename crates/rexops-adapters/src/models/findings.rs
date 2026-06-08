//! findings.rs — Workstate findings data types.
//!
//! These types model the `findings.data` payload in the Workstate v3 snapshot.
//!
//! Read-only, serde-friendly, no execution logic.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One scanned item. Known fields are named; everything else is preserved in
/// `rest` so RexOps never loses data or rejects unknown keys.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ScanItem {
    /// Risk severity as a free string (e.g. "critical", "high").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    /// Stable id-like label, if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Human name, used as a display fallback when `id` is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Everything else the export carried, kept verbatim (additionalProperties).
    #[serde(flatten)]
    pub rest: BTreeMap<String, Value>,
}

impl ScanItem {
    /// Best display label: id, then name, then "<unnamed>".
    pub fn label(&self) -> &str {
        self.id
            .as_deref()
            .or(self.name.as_deref())
            .unwrap_or("<unnamed>")
    }

    /// Normalized severity bucket: lowercased, mapped to a known set or "unknown".
    /// Returns None when the item carries no severity field at all (so callers can
    /// distinguish "no risk data" from "severity present but unrecognized").
    pub fn severity_bucket(&self) -> Option<Severity> {
        self.severity.as_ref().map(|s| Severity::from_str(s))
    }
}

/// Closed bucket set we tally into. `Unknown` absorbs anything unrecognized so
/// an unexpected severity string never breaks parsing or counting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Info,
    Unknown,
}

impl Severity {
    fn from_str(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "critical" => Self::Critical,
            "high" => Self::High,
            "medium" => Self::Medium,
            "low" => Self::Low,
            "info" => Self::Info,
            _ => Self::Unknown,
        }
    }
}

/// The whole findings payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct FindingsInfo {
    /// `#[serde(default)]` because the Workstate v3 snapshot carries the version
    /// at the envelope level, not inside this `data` payload.
    #[serde(default)]
    pub schema_version: i64,
    #[serde(default)]
    pub generated_at: String,
    /// Scanned findings. `alias = "findings"` lets this type absorb the
    /// Workstate v3 snapshot's `findings[]` array.
    #[serde(default, alias = "findings")]
    pub items: Vec<ScanItem>,
}

/// A small, self-contained risk tally derived from the items. Mirrors the
/// fields of rexops-core's RiskSummary without depending on core (adapters must
/// not depend on core). The app layer copies these into the snapshot's RiskSummary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RiskTally {
    pub critical: u32,
    pub high: u32,
    pub medium: u32,
    pub low: u32,
    pub info: u32,
    /// Items whose severity field was present but unrecognized.
    pub unknown: u32,
    /// Items with NO severity field at all (drives "breakdown unavailable").
    pub unrated: u32,
}

impl RiskTally {
    /// Total items that carried a recognized severity bucket.
    pub fn rated_total(&self) -> u32 {
        self.critical + self.high + self.medium + self.low + self.info
    }

    /// True when at least one item carried a usable severity. When false the
    /// cockpit should say "risk breakdown unavailable" rather than show zeros.
    pub fn has_risk_data(&self) -> bool {
        self.rated_total() + self.unknown > 0
    }

    /// Any critical or high item → recommend blocking.
    pub fn should_block(&self) -> bool {
        self.critical > 0 || self.high > 0
    }
}

impl FindingsInfo {
    /// Roll item severities into a RiskTally.
    pub fn risk_tally(&self) -> RiskTally {
        let mut t = RiskTally::default();
        for item in &self.items {
            match item.severity_bucket() {
                Some(Severity::Critical) => t.critical += 1,
                Some(Severity::High) => t.high += 1,
                Some(Severity::Medium) => t.medium += 1,
                Some(Severity::Low) => t.low += 1,
                Some(Severity::Info) => t.info += 1,
                Some(Severity::Unknown) => t.unknown += 1,
                None => t.unrated += 1,
            }
        }
        t
    }

    /// Items whose severity is critical or high, for the "high-risk items" view.
    pub fn high_risk_items(&self) -> impl Iterator<Item = &ScanItem> {
        self.items.iter().filter(|i| {
            matches!(
                i.severity_bucket(),
                Some(Severity::Critical | Severity::High)
            )
        })
    }
}
