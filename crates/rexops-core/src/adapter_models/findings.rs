//! findings.rs — Workstate findings data types (pure data, no execution).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One scanned item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ScanItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(flatten)]
    pub rest: BTreeMap<String, Value>,
}

impl ScanItem {
    pub fn label(&self) -> &str {
        self.id
            .as_deref()
            .or(self.name.as_deref())
            .unwrap_or("<unnamed>")
    }

    pub fn severity_bucket(&self) -> Option<Severity> {
        self.severity.as_ref().map(|s| Severity::from_str(s))
    }
}

/// Closed bucket set we tally into.
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
    #[serde(default)]
    pub schema_version: i64,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default, alias = "findings")]
    pub items: Vec<ScanItem>,
}

/// A small, self-contained risk tally derived from the items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RiskTally {
    pub critical: u32,
    pub high: u32,
    pub medium: u32,
    pub low: u32,
    pub info: u32,
    pub unknown: u32,
    pub unrated: u32,
}

impl RiskTally {
    pub fn rated_total(&self) -> u32 {
        self.critical + self.high + self.medium + self.low + self.info
    }

    pub fn has_risk_data(&self) -> bool {
        self.rated_total() + self.unknown > 0
    }

    pub fn should_block(&self) -> bool {
        self.critical > 0 || self.high > 0
    }
}

impl FindingsInfo {
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

    pub fn high_risk_items(&self) -> impl Iterator<Item = &ScanItem> {
        self.items.iter().filter(|i| {
            matches!(
                i.severity_bucket(),
                Some(Severity::Critical | Severity::High)
            )
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn item(severity: Option<&str>) -> ScanItem {
        ScanItem {
            severity: severity.map(str::to_owned),
            ..ScanItem::default()
        }
    }

    #[test]
    fn label_prefers_id_then_name_then_placeholder() {
        let mut i = ScanItem {
            id: Some("the-id".into()),
            name: Some("the-name".into()),
            ..ScanItem::default()
        };
        assert_eq!(i.label(), "the-id");
        i.id = None;
        assert_eq!(i.label(), "the-name");
        i.name = None;
        assert_eq!(i.label(), "<unnamed>");
    }

    #[test]
    fn severity_bucket_is_case_insensitive_and_maps_unknowns() {
        assert_eq!(
            item(Some("CRITICAL")).severity_bucket(),
            Some(Severity::Critical)
        );
        assert_eq!(
            item(Some("  high ")).severity_bucket(),
            Some(Severity::High)
        );
        assert_eq!(
            item(Some("medium")).severity_bucket(),
            Some(Severity::Medium)
        );
        assert_eq!(
            item(Some("nonsense")).severity_bucket(),
            Some(Severity::Unknown)
        );
        // No severity field at all is None (distinct from "present but weird").
        assert_eq!(item(None).severity_bucket(), None);
    }

    #[test]
    fn risk_tally_sorts_items_into_the_right_buckets() {
        let info = FindingsInfo {
            items: vec![
                item(Some("critical")),
                item(Some("high")),
                item(Some("high")),
                item(Some("low")),
                item(Some("weird")), // present-but-unrecognized → unknown
                item(None),          // no severity → unrated
            ],
            ..FindingsInfo::default()
        };
        let t = info.risk_tally();
        assert_eq!(t.critical, 1);
        assert_eq!(t.high, 2);
        assert_eq!(t.low, 1);
        assert_eq!(t.unknown, 1, "unrecognized severity string");
        assert_eq!(t.unrated, 1, "no severity field at all");
        assert_eq!(t.rated_total(), 4, "critical+high+high+low");
    }

    #[test]
    fn has_risk_data_distinguishes_no_data_from_zero() {
        // All items unrated → no usable risk data → cockpit shows "unavailable".
        let unrated = FindingsInfo {
            items: vec![item(None), item(None)],
            ..FindingsInfo::default()
        };
        assert!(!unrated.risk_tally().has_risk_data());

        // A single recognized severity flips it to "we have data".
        let rated = FindingsInfo {
            items: vec![item(Some("low"))],
            ..FindingsInfo::default()
        };
        assert!(rated.risk_tally().has_risk_data());
    }

    #[test]
    fn should_block_only_on_critical_or_high() {
        let block = FindingsInfo {
            items: vec![item(Some("high"))],
            ..FindingsInfo::default()
        };
        assert!(block.risk_tally().should_block());

        let no_block = FindingsInfo {
            items: vec![item(Some("medium")), item(Some("low"))],
            ..FindingsInfo::default()
        };
        assert!(!no_block.risk_tally().should_block());
    }

    #[test]
    fn high_risk_items_yields_only_critical_and_high() {
        let info = FindingsInfo {
            items: vec![
                item(Some("critical")),
                item(Some("medium")),
                item(Some("high")),
                item(None),
            ],
            ..FindingsInfo::default()
        };
        let high: Vec<_> = info.high_risk_items().collect();
        assert_eq!(high.len(), 2, "critical + high only");
    }

    #[test]
    fn items_absorb_the_findings_alias() {
        // The `alias = "findings"` on `items` lets a Workstate `findings[]` array
        // deserialize straight into FindingsInfo.items.
        let info: FindingsInfo =
            serde_json::from_str(r#"{"schema_version":3,"findings":[{"severity":"high"}]}"#)
                .unwrap();
        assert_eq!(info.items.len(), 1);
        assert_eq!(info.items[0].severity.as_deref(), Some("high"));
    }

    #[test]
    fn scan_item_preserves_unknown_keys_in_rest() {
        let i: ScanItem = serde_json::from_str(r#"{"severity":"low","extra":123}"#).unwrap();
        assert_eq!(
            i.rest.get("extra").and_then(serde_json::Value::as_i64),
            Some(123)
        );
    }
}
