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

/// Translate a Workstate section `status` string into RexOps' AdapterHealth.
pub fn status_to_health(status: &str) -> crate::AdapterHealth {
    match status.trim() {
        "Fresh" => crate::AdapterHealth::Healthy,
        "Stale" | "UnsupportedVersion" => crate::AdapterHealth::Degraded,
        "Missing" => crate::AdapterHealth::Unavailable,
        _ => crate::AdapterHealth::Unknown,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::AdapterHealth;

    #[test]
    fn status_to_health_covers_known_and_unknown_values() {
        assert_eq!(status_to_health("Fresh"), AdapterHealth::Healthy);
        assert_eq!(status_to_health("Stale"), AdapterHealth::Degraded);
        assert_eq!(
            status_to_health("UnsupportedVersion"),
            AdapterHealth::Degraded
        );
        assert_eq!(status_to_health("Missing"), AdapterHealth::Unavailable);
        // An unanticipated status is Unknown, never silently treated as healthy.
        assert_eq!(status_to_health("WeirdNewStatus"), AdapterHealth::Unknown);
    }

    #[test]
    fn status_to_health_trims_surrounding_whitespace() {
        // The core impl trims, so a status with stray padding still maps correctly.
        assert_eq!(status_to_health("  Fresh  "), AdapterHealth::Healthy);
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
