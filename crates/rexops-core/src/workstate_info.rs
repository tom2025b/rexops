//! workstate_info.rs — Pure data types for the Workstate v3 snapshot envelope.
//!
//! Moved to core so OpsSnapshot can hold WorkstateInfo without depending on the
//! adapter execution layer. WorkstateAdapter (in rexops-adapters) imports these
//! types from here.

use serde::{Deserialize, Deserializer, Serialize};

use crate::{FindingsInfo, ScriptsInfo, ToolsInfo};

/// Provenance Workstate attaches to each section: who produced it and when.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Provenance {
    #[serde(default)]
    pub feed_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_observed_at: Option<String>,
}

/// One snapshot section: Workstate's freshness `status`, its `provenance`, and
/// the normalized `data` (absent when the section is Missing/UnsupportedVersion).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Section<T> {
    /// Workstate's freshness verdict, kept as a String so `status_to_freshness`
    /// stays the single place that interprets it. Workstate's wire format is
    /// NOT uniform: the simple states (`Fresh`/`Stale`/`Missing`) serialize as
    /// bare JSON strings, but the data-bearing variants serialize as serde's
    /// default externally-tagged OBJECTS — `{"Failed":{"reason":"..."}}` and
    /// `{"UnsupportedVersion":{"found":2,"supported":3}}`. A plain `String`
    /// field rejected those objects, failing the WHOLE envelope and dropping
    /// every section (P1). `status_field` accepts either shape, collapsing a
    /// tagged object to its variant key ("Failed"/"UnsupportedVersion") so the
    /// status reads the same whether the section degraded with or without a
    /// payload — exactly the per-section degradation Workstate intends.
    #[serde(default, deserialize_with = "status_field")]
    pub status: String,
    #[serde(default)]
    pub provenance: Provenance,
    #[serde(default = "none")]
    pub data: Option<T>,
}

fn none<T>() -> Option<T> {
    None
}

/// Deserialize a section `status` from EITHER a bare string (`"Fresh"`) or
/// Workstate's externally-tagged enum object for a data-bearing variant
/// (`{"Failed":{...}}` → `"Failed"`). Any object collapses to its single key,
/// so the payload (`reason`, `found`/`supported`) is intentionally dropped —
/// RexOps surfaces freshness, not Workstate's internal failure detail, and the
/// variant NAME is all `status_to_freshness` needs. A null or unexpected shape
/// degrades to the empty string, which maps to `Freshness::Unknown` rather than
/// erroring — keeping a single odd section from poisoning the whole snapshot.
fn status_field<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    use serde_json::Value;
    let value = Value::deserialize(deserializer)?;
    Ok(match value {
        // Plain string status: `"Fresh"`, `"Stale"`, `"Missing"`, ...
        Value::String(s) => s,
        // Externally-tagged enum object: the single key IS the variant name.
        // `{"Failed":{"reason":...}}` → "Failed"; we keep only the tag.
        Value::Object(map) => map
            .into_iter()
            .next()
            .map(|(tag, _payload)| tag)
            .unwrap_or_default(),
        // null / number / anything else: no usable status → "" → Unknown.
        _ => String::new(),
    })
}

/// The whole Workstate snapshot envelope (schema v3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct WorkstateInfo {
    pub schema_version: i64,
    #[serde(default)]
    pub built_at: String,
    #[serde(default)]
    pub scripts: Section<ScriptsInfo>,
    #[serde(default)]
    pub tools: Section<ToolsInfo>,
    #[serde(default)]
    pub findings: Section<FindingsInfo>,
}

impl WorkstateInfo {
    /// Number of sections that carry usable data (0–3).
    pub fn populated_section_count(&self) -> usize {
        usize::from(self.scripts.data.is_some())
            + usize::from(self.tools.data.is_some())
            + usize::from(self.findings.data.is_some())
    }
}

/// Freshness of a Workstate **section** — how current its data is.
///
/// This is deliberately NOT `AdapterHealth`. A stale section is not a *fault*:
/// the data simply hasn't been recompiled recently. Conflating the two made a
/// correctly-working install render all-yellow on first launch (every section
/// is a few days old → Stale → was mapped to `Degraded`). Freshness gets its own
/// neutral vocabulary so the UI can show it dim/informational instead of as a
/// health alarm. Adapters have health; sections have freshness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    /// Recompiled recently — current.
    Fresh,
    /// Data is older than Workstate's freshness window, or the snapshot's schema
    /// version is newer than we fully support. Usable, just not current. Neutral,
    /// not an error.
    Stale,
    /// The section is absent from the snapshot (no data compiled for it).
    Missing,
    /// An unrecognized status string — surfaced rather than silently assumed Fresh.
    Unknown,
}

impl Freshness {
    /// A short, lowercase tag for rendering next to a section ("fresh", "stale").
    pub fn label(&self) -> &'static str {
        match self {
            Freshness::Fresh => "fresh",
            Freshness::Stale => "stale",
            Freshness::Missing => "missing",
            Freshness::Unknown => "unknown",
        }
    }
}

/// Translate a Workstate section `status` string into a [`Freshness`].
///
/// Positive matches only; an unanticipated status is `Unknown`, never silently
/// treated as Fresh. This replaces the old `status_to_health`: a section's
/// status describes *currency*, not adapter health, so it must not be coerced
/// into the health vocabulary.
pub fn status_to_freshness(status: &str) -> Freshness {
    match status.trim() {
        "Fresh" => Freshness::Fresh,
        "Stale" | "UnsupportedVersion" => Freshness::Stale,
        // A Failed section is present-but-unusable: Workstate could not read or
        // parse the feed, so there is no data, the same effective state as a
        // section that never arrived. Map it to Missing (no usable data) rather
        // than Stale (which implies usable, merely old). `UnsupportedVersion`
        // stays Stale: its data is intentionally dropped but the section is
        // otherwise healthy and a fresher RexOps could read it.
        "Missing" | "Failed" => Freshness::Missing,
        _ => Freshness::Unknown,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn status_to_freshness_covers_known_and_unknown_values() {
        assert_eq!(status_to_freshness("Fresh"), Freshness::Fresh);
        assert_eq!(status_to_freshness("Stale"), Freshness::Stale);
        // A newer-than-supported snapshot is still usable, just not current.
        assert_eq!(status_to_freshness("UnsupportedVersion"), Freshness::Stale);
        assert_eq!(status_to_freshness("Missing"), Freshness::Missing);
        // An unanticipated status is Unknown, never silently treated as Fresh.
        assert_eq!(status_to_freshness("WeirdNewStatus"), Freshness::Unknown);
        // A Failed section has no usable data — the same effective state as a
        // section that never arrived, so it maps to Missing (not Stale/Unknown).
        assert_eq!(status_to_freshness("Failed"), Freshness::Missing);
    }

    #[test]
    fn section_status_accepts_a_bare_string() {
        // The common case: a simple freshness verdict is a plain JSON string.
        let s: Section<ScriptsInfo> = serde_json::from_str(r#"{"status":"Stale"}"#).unwrap();
        assert_eq!(s.status, "Stale");
        assert_eq!(status_to_freshness(&s.status), Freshness::Stale);
    }

    #[test]
    fn section_status_accepts_the_failed_tagged_object() {
        // THE P1 REGRESSION: Workstate serializes the data-bearing `Failed`
        // variant as an externally-tagged OBJECT, not a string. A plain-String
        // field rejected it and failed the whole envelope. We must accept it and
        // collapse it to its variant key so the section degrades on its own.
        let s: Section<ScriptsInfo> =
            serde_json::from_str(r#"{"status":{"Failed":{"reason":"feed unreadable"}}}"#).unwrap();
        assert_eq!(s.status, "Failed", "tagged object collapses to its tag");
        assert_eq!(status_to_freshness(&s.status), Freshness::Missing);
        assert!(s.data.is_none(), "a Failed section carries no data");
    }

    #[test]
    fn section_status_accepts_the_unsupported_version_tagged_object() {
        // The other data-bearing variant Workstate emits, also as an object.
        let s: Section<ScriptsInfo> =
            serde_json::from_str(r#"{"status":{"UnsupportedVersion":{"found":2,"supported":3}}}"#)
                .unwrap();
        assert_eq!(s.status, "UnsupportedVersion");
        // Newer-than-supported is still a healthy section — Stale, not Missing.
        assert_eq!(status_to_freshness(&s.status), Freshness::Stale);
    }

    #[test]
    fn section_status_degrades_an_odd_shape_to_unknown_not_an_error() {
        // A status that is neither a string nor a tagged object (here null) must
        // NOT fail the parse — it degrades to "" → Unknown so one weird section
        // never poisons the whole snapshot.
        let s: Section<ScriptsInfo> = serde_json::from_str(r#"{"status":null}"#).unwrap();
        assert_eq!(s.status, "");
        assert_eq!(status_to_freshness(&s.status), Freshness::Unknown);
    }

    #[test]
    fn whole_envelope_survives_one_failed_section() {
        // The end-to-end guarantee: a snapshot with ONE Failed/UnsupportedVersion
        // section must still parse, keeping the healthy sections — the exact
        // per-section degradation that the plain-String field used to destroy by
        // failing the entire envelope.
        let feed = r#"{"schema_version":3,"built_at":"x",
            "scripts":{"status":"Fresh","provenance":{"feed_id":"scripts"}},
            "tools":{"status":{"Failed":{"reason":"toolfoundry feed unreadable"}},"provenance":{"feed_id":"tools"}},
            "findings":{"status":{"UnsupportedVersion":{"found":2,"supported":3}},"provenance":{"feed_id":"findings"}}}"#;
        let info: WorkstateInfo = serde_json::from_str(feed).unwrap();
        assert_eq!(info.scripts.status, "Fresh");
        assert_eq!(info.tools.status, "Failed");
        assert_eq!(info.findings.status, "UnsupportedVersion");
        assert_eq!(status_to_freshness(&info.tools.status), Freshness::Missing);
        assert_eq!(status_to_freshness(&info.findings.status), Freshness::Stale);
    }

    #[test]
    fn status_to_freshness_trims_surrounding_whitespace() {
        // The core impl trims, so a status with stray padding still maps correctly.
        assert_eq!(status_to_freshness("  Fresh  "), Freshness::Fresh);
    }

    #[test]
    fn freshness_is_not_health_stale_is_neutral() {
        // The whole point of the split: Stale must NOT read as a health fault.
        // It carries its own neutral label, distinct from AdapterHealth::Degraded.
        assert_eq!(Freshness::Stale.label(), "stale");
        assert_eq!(Freshness::Fresh.label(), "fresh");
    }

    #[test]
    fn populated_section_count_counts_only_sections_with_data() {
        let mut info = WorkstateInfo::default();
        assert_eq!(info.populated_section_count(), 0, "empty envelope → 0");

        info.scripts.data = Some(ScriptsInfo::default());
        assert_eq!(info.populated_section_count(), 1);

        info.tools.data = Some(ToolsInfo::default());
        info.findings.data = Some(FindingsInfo::default());
        assert_eq!(info.populated_section_count(), 3, "all three populated");
    }

    #[test]
    fn section_data_defaults_to_none() {
        // The serde `default = "none"` on Section::data: an omitted data key
        // parses to None, which is what "Missing" sections rely on.
        let section: Section<ScriptsInfo> =
            serde_json::from_str(r#"{"status":"Missing"}"#).unwrap();
        assert_eq!(section.status, "Missing");
        assert!(section.data.is_none());
        assert_eq!(section.provenance, Provenance::default());
    }

    #[test]
    fn envelope_roundtrips_via_serde() {
        let mut info = WorkstateInfo {
            schema_version: 3,
            built_at: "2026-06-13".into(),
            ..WorkstateInfo::default()
        };
        info.scripts.status = "Fresh".into();
        info.scripts.data = Some(ScriptsInfo::default());
        info.scripts.provenance.feed_id = "scripts".into();

        let json = serde_json::to_string(&info).unwrap();
        let back: WorkstateInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }

    #[test]
    fn provenance_optional_fields_omitted_when_none() {
        let json = serde_json::to_string(&Provenance::default()).unwrap();
        assert!(!json.contains("fetched_at"), "None omitted, got: {json}");
        assert!(
            !json.contains("source_observed_at"),
            "None omitted, got: {json}"
        );
    }
}
