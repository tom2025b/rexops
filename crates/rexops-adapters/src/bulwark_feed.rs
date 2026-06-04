//! bulwark_feed.rs — Read-only consumer for the Bulwark *scan export* feed.
//!
//! This is SEPARATE from bulwark.rs. `bulwark.rs` spawns `bulwark inspect scan`
//! and parses live findings; this module reads an exported, versioned scan JSON
//! from in-memory text (routed piped stdin) or the documented standard path. Two
//! genuinely different data sources, kept apart on purpose.
//!
//! Target shape: ../linux-ops-suite/contracts/bulwark.scan.schema.json — which is
//! explicitly PROVISIONAL. It fixes only the envelope (schema_version,
//! source_tool, generated_at, items[]) and warns "do not treat the item shape as
//! final". So we type the envelope but keep `items` loose: each item exposes a
//! couple of opportunistic optional fields (severity, id/name) and flattens the
//! rest into a free-form map. Severity is read as a *string* and bucketed with an
//! "unknown" fallback — never via a closed enum that would hard-fail on a value
//! we did not anticipate.
//!
//! Read-only: never writes back, never spawns a binary.
//!
//! NOTE on stdin: this adapter never reads `std::io::stdin()` itself. stdin is a
//! process-wide singleton that can be drained only once, and RexOps has several
//! feed consumers — so the snapshot layer reads stdin once, routes the bytes to
//! the right consumer by content, and hands them in via `with_text`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapter::Adapter;
use crate::error::AdapterError;
use crate::types::{AdapterHealth, AdapterOutput};

/// The major schema version this consumer understands.
const SUPPORTED_SCHEMA_VERSION: i64 = 1;

/// One scanned item. Provisional: only a few fields are named; everything else
/// is preserved in `rest` so we never lose data and never reject unknown keys.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ScanItem {
    /// Risk severity as a free string (e.g. "critical", "high"). Optional because
    /// the provisional contract does not require it.
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

/// Closed bucket set we tally into. `Unknown` absorbs anything unrecognized so a
/// provisional/unexpected severity string never breaks parsing or counting.
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

/// The whole scan export envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BulwarkScanInfo {
    pub schema_version: i64,
    /// Lenient: should be "bulwark" but we don't reject a mismatch.
    #[serde(default)]
    pub source_tool: String,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
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

impl BulwarkScanInfo {
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

/// Tiny probe to read just the version before a full parse.
#[derive(Debug, Deserialize)]
struct VersionProbe {
    schema_version: Option<i64>,
}

/// Read-only Bulwark scan-feed consumer.
///
/// Acquisition precedence: in-memory text (from `with_text`) → explicit path
/// (`with_path`) → the documented standard path. The adapter never reads stdin.
#[derive(Debug, Clone, Default)]
pub struct BulwarkFeedAdapter {
    text_override: Option<String>,
    path_override: Option<PathBuf>,
}

impl BulwarkFeedAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct an adapter that reads from in-memory text (e.g. piped stdin the
    /// snapshot layer already captured and routed here).
    pub fn with_text(text: impl Into<String>) -> Self {
        Self {
            text_override: Some(text.into()),
            path_override: None,
        }
    }

    /// Construct an adapter that always reads from an explicit file path.
    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        Self {
            text_override: None,
            path_override: Some(path.into()),
        }
    }

    /// Documented standard read location:
    ///   $XDG_DATA_HOME/rexops/feeds/bulwark.scan.json
    /// falling back to ~/.local/share/rexops/feeds/... when XDG is unset.
    pub fn standard_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
        Some(base.join("rexops/feeds/bulwark.scan.json"))
    }

    /// Acquire raw feed text by precedence: in-memory text → explicit path →
    /// standard path. Ok(None) means "no feed available" — a normal condition.
    /// Never reads stdin (see module note).
    fn read_feed_text(&self) -> Result<Option<String>, AdapterError> {
        if let Some(text) = &self.text_override {
            return Ok(Some(text.clone()));
        }
        let path = match &self.path_override {
            Some(p) => Some(p.clone()),
            None => Self::standard_path(),
        };
        let Some(path) = path else {
            return Ok(None);
        };
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(std::fs::read_to_string(&path)?))
    }

    /// Parse feed text, gating on schema version.
    ///   supported version → Ok(Some(info))
    ///   missing/other     → Ok(None)  (graceful skip)
    /// Malformed JSON stays a hard JsonParse error so real bugs surface.
    pub fn parse_feed(text: &str) -> Result<Option<BulwarkScanInfo>, AdapterError> {
        let probe: VersionProbe = serde_json::from_str(text)?;
        match probe.schema_version {
            Some(v) if v == SUPPORTED_SCHEMA_VERSION => {
                let info: BulwarkScanInfo = serde_json::from_str(text)?;
                Ok(Some(info))
            }
            _ => Ok(None),
        }
    }

    /// Acquire + parse in a SINGLE read (stdin is consume-once), returning both
    /// the health and (on a supported version) the parsed feed.
    ///   (Healthy, Some)     → supported-version feed parsed.
    ///   (Degraded, None)    → feed present, unknown/missing version.
    ///   (Unavailable, None) → no feed found.
    pub fn read(
        &self,
    ) -> Result<(AdapterHealth, Option<AdapterOutput<BulwarkScanInfo>>), AdapterError> {
        let Some(text) = self.read_feed_text()? else {
            return Ok((AdapterHealth::Unavailable, None));
        };
        match Self::parse_feed(&text)? {
            Some(info) => {
                let out = AdapterOutput::new("bulwark-feed", AdapterHealth::Healthy, info);
                Ok((AdapterHealth::Healthy, Some(out)))
            }
            None => Ok((AdapterHealth::Degraded, None)),
        }
    }

    /// Convenience: just the parsed feed (drops health). Prefer `read()` when you
    /// also need health, since each call re-acquires (and re-drains stdin).
    pub fn info(&self) -> Result<Option<AdapterOutput<BulwarkScanInfo>>, AdapterError> {
        Ok(self.read()?.1)
    }
}

impl Adapter for BulwarkFeedAdapter {
    fn check_available(&self) -> bool {
        matches!(self.read(), Ok((AdapterHealth::Healthy, _)))
    }

    fn version(&self) -> Result<Option<String>, AdapterError> {
        match self.read()?.1 {
            Some(out) => Ok(Some(format!("schema_version={}", out.data.schema_version))),
            None => Ok(None),
        }
    }

    fn health(&self) -> AdapterHealth {
        self.read().map_or(AdapterHealth::Unknown, |(h, _)| h)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const FEED_V1: &str = include_str!("../fixtures/bulwark/scan_feed_v1.json");

    #[test]
    fn parses_v1_fixture_with_risk_breakdown() {
        let info = BulwarkFeedAdapter::parse_feed(FEED_V1)
            .expect("v1 fixture must parse")
            .expect("v1 fixture must be an accepted version");
        assert_eq!(info.schema_version, 1);
        assert_eq!(info.source_tool, "bulwark");
        assert_eq!(info.items.len(), 4);

        let t = info.risk_tally();
        assert_eq!(t.critical, 1);
        assert_eq!(t.high, 1);
        assert_eq!(t.medium, 1);
        assert_eq!(t.low, 1);
        assert_eq!(t.unrated, 0);
        assert!(t.has_risk_data());
        assert!(t.should_block(), "a critical item must force should_block");

        let high: Vec<&str> = info.high_risk_items().map(ScanItem::label).collect();
        assert_eq!(high.len(), 2);
        assert!(high.contains(&"deploy-prod.sh"));
        assert!(high.contains(&"healthcheck.sh"));
    }

    #[test]
    fn unknown_major_version_is_graceful_skip() {
        let v99 = r#"{"schema_version": 99, "source_tool": "bulwark", "items": []}"#;
        assert!(BulwarkFeedAdapter::parse_feed(v99)
            .expect("must not error")
            .is_none());
    }

    #[test]
    fn missing_version_is_graceful_skip() {
        let no_ver = r#"{"source_tool": "bulwark", "items": []}"#;
        assert!(BulwarkFeedAdapter::parse_feed(no_ver)
            .expect("must not error")
            .is_none());
    }

    #[test]
    fn items_without_severity_report_no_risk_data() {
        // "risk breakdown if available" — when nothing is rated, say so, don't
        // pretend everything is zero-risk.
        let feed = r#"{"schema_version": 1, "source_tool": "bulwark",
                       "items": [{"id": "a"}, {"id": "b", "rule_id": "x"}]}"#;
        let info = BulwarkFeedAdapter::parse_feed(feed).unwrap().unwrap();
        let t = info.risk_tally();
        assert_eq!(t.unrated, 2);
        assert_eq!(t.rated_total(), 0);
        assert!(!t.has_risk_data());
        assert!(!t.should_block());
    }

    #[test]
    fn unrecognized_severity_buckets_as_unknown_not_error() {
        let feed = r#"{"schema_version": 1, "source_tool": "bulwark",
                       "items": [{"id": "a", "severity": "spicy"}]}"#;
        let info = BulwarkFeedAdapter::parse_feed(feed).unwrap().unwrap();
        let t = info.risk_tally();
        assert_eq!(t.unknown, 1);
        assert!(
            t.has_risk_data(),
            "an unknown-but-present severity is risk data"
        );
    }

    #[test]
    fn lenient_source_tool_does_not_reject() {
        // A wrong/missing source_tool must not crash parsing (provisional handling).
        let feed = r#"{"schema_version": 1, "source_tool": "not-bulwark", "items": []}"#;
        let info = BulwarkFeedAdapter::parse_feed(feed).unwrap().unwrap();
        assert_eq!(info.source_tool, "not-bulwark");
    }

    #[test]
    fn malformed_json_is_a_parse_error() {
        let err = BulwarkFeedAdapter::parse_feed("{not json").unwrap_err();
        assert!(matches!(err, AdapterError::JsonParse(_)));
    }

    #[test]
    fn unknown_item_fields_are_preserved_in_rest() {
        let feed = r#"{"schema_version": 1, "source_tool": "bulwark",
                       "items": [{"id": "a", "severity": "low", "future": 42}]}"#;
        let info = BulwarkFeedAdapter::parse_feed(feed).unwrap().unwrap();
        assert_eq!(info.items[0].rest.get("future").unwrap(), &Value::from(42));
    }

    #[test]
    fn info_roundtrips_via_serde() {
        let info = BulwarkScanInfo {
            schema_version: 1,
            source_tool: "bulwark".into(),
            generated_at: "2026-06-04".into(),
            items: vec![ScanItem {
                severity: Some("high".into()),
                id: Some("x".into()),
                ..Default::default()
            }],
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: BulwarkScanInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }

    #[test]
    fn read_returns_health_and_info_from_one_acquisition() {
        let dir = std::env::temp_dir().join("rexops-bwf-test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("scan.json");
        std::fs::write(&p, FEED_V1).unwrap();

        let a = BulwarkFeedAdapter::with_path(&p);
        let (health, out) = a.read().expect("read ok");
        assert_eq!(health, AdapterHealth::Healthy);
        let out = out.expect("v1 feed must produce data alongside Healthy");
        assert_eq!(out.adapter, "bulwark-feed");
        assert_eq!(out.data.items.len(), 4);

        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn missing_path_is_unavailable_not_error() {
        let a = BulwarkFeedAdapter::with_path("/no/such/rexops/bulwark/xyz123.json");
        assert!(a.info().expect("must not error").is_none());
        assert_eq!(a.health(), AdapterHealth::Unavailable);
    }

    #[test]
    fn with_text_reads_from_memory_without_touching_disk_or_stdin() {
        // This is how the snapshot layer hands routed stdin bytes to the adapter.
        let a = BulwarkFeedAdapter::with_text(FEED_V1);
        let (health, out) = a.read().expect("read ok");
        assert_eq!(health, AdapterHealth::Healthy);
        assert_eq!(out.expect("v1 feed present").data.items.len(), 4);
    }
}

// Learning Notes:
// - This is the second contract-feed consumer (after toolfoundry.rs) and mirrors
//   its structure exactly: acquisition precedence in-memory text → explicit path
//   → standard path (NEVER stdin directly), a single read() that returns health +
//   data together (stdin can only be drained once, so the snapshot layer reads it
//   once and routes), and a schema_version gate that SKIPS unknown versions.
// - The big difference from toolfoundry is the PROVISIONAL contract: items are
//   loosely typed (a few optionals + #[serde(flatten)] rest), and severity is a
//   string bucketed with an Unknown/None fallback. This means a future change to
//   Bulwark's item shape will not break us — exactly what "refine the schema
//   later" requires.
// - RiskTally is intentionally local to the adapter crate (which must not depend
//   on rexops-core). The app layer translates it into core's RiskSummary. This
//   keeps the dependency arrows pointing the right way.
// - has_risk_data() vs zero counts is the heart of "risk breakdown if available":
//   we never claim "0 critical" when the truth is "we have no severity data".
