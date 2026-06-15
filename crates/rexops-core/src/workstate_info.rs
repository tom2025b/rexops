//! workstate_info.rs — Pure data types for the Workstate v3 snapshot envelope.
//!
//! Moved to core so OpsSnapshot can hold WorkstateInfo without depending on the
//! adapter execution layer. WorkstateAdapter (in rexops-adapters) imports these
//! types from here.

use serde::{Deserialize, Serialize};

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
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub provenance: Provenance,
    #[serde(default = "none")]
    pub data: Option<T>,
}

fn none<T>() -> Option<T> {
    None
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
        "Missing" => Freshness::Missing,
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
