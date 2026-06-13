//! workstate.rs — Read-only consumer for the Workstate *snapshot* feed (v3).
//!
//! Workstate is the central state-compiler for the Linux Ops suite. It emits one
//! versioned `snapshot.json`, and RexOps reads that snapshot as its single source
//! of truth.
//!
//! Consumer contract:
//! - never reads stdin directly (the snapshot layer reads it once and routes),
//! - single read() returning (health, Option<data>),
//! - a schema_version gate that SKIPS unknown versions instead of erroring.
//!
//! SHAPE (schema v3): an envelope (`schema_version`, `built_at`) wrapping three
//! `Section`s — `scripts`, `tools`, `findings`. Each Section carries a `status`
//! (Workstate's freshness verdict "Fresh"/"Stale"/...), `provenance`, and `data`
//! (the normalized payload, absent when the section is Missing/UnsupportedVersion).
//!
//! The three `data` payloads map directly to RexOps' scripts, tools, and findings
//! section types.
//!
//! Read-only: never writes back, never spawns a binary. Workstate is itself
//! strictly read-only, so there is nothing to mutate on our side either.

use std::path::PathBuf;

use crate::adapter::Adapter;
use crate::error::AdapterError;
use crate::types::{AdapterHealth, AdapterOutput};

// Pure data types now live in rexops-core; re-export so existing pub API is unchanged.
pub use rexops_core::{Provenance, Section, WorkstateInfo, status_to_health};

/// The major schema version this consumer understands. Workstate emits v3.
const SUPPORTED_SCHEMA_VERSION: i64 = 3;

/// Read-only Workstate snapshot consumer.
///
/// Acquisition precedence: in-memory text (`with_text`) → explicit path
/// (`with_path`) → the documented standard path. Never reads stdin itself.
#[derive(Debug, Clone, Default)]
pub struct WorkstateAdapter {
    text_override: Option<String>,
    path_override: Option<PathBuf>,
}

impl WorkstateAdapter {
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
    ///   $XDG_DATA_HOME/rexops/feeds/workstate.snapshot.json
    /// falling back to ~/.local/share/rexops/feeds/... when XDG is unset.
    pub fn standard_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
        Some(base.join("rexops/feeds/workstate.snapshot.json"))
    }

    /// Acquire snapshot text by precedence: in-memory text → explicit path →
    /// standard path. Ok(None) means "no feed available". Never reads stdin.
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
    pub fn parse_feed(text: &str) -> Result<Option<WorkstateInfo>, AdapterError> {
        let value: serde_json::Value = serde_json::from_str(text)?;
        let version = value.get("schema_version").and_then(serde_json::Value::as_i64);
        match version {
            Some(v) if v == SUPPORTED_SCHEMA_VERSION => {
                let info: WorkstateInfo = serde_json::from_value(value)?;
                Ok(Some(info))
            }
            _ => Ok(None),
        }
    }

    /// Acquire + parse in a SINGLE read, returning both the health and (on a
    /// supported version) the parsed feed.
    ///   (Healthy, Some)     → supported-version snapshot parsed.
    ///   (Degraded, None)    → snapshot present, unknown/missing version.
    ///   (Unavailable, None) → no snapshot found.
    pub fn read(
        &self,
    ) -> Result<(AdapterHealth, Option<AdapterOutput<WorkstateInfo>>), AdapterError> {
        let Some(text) = self.read_feed_text()? else {
            return Ok((AdapterHealth::Unavailable, None));
        };
        match Self::parse_feed(&text)? {
            Some(info) => {
                let out = AdapterOutput::new("workstate", AdapterHealth::Healthy, info);
                Ok((AdapterHealth::Healthy, Some(out)))
            }
            None => Ok((AdapterHealth::Degraded, None)),
        }
    }

    /// Convenience: just the parsed feed (drops health). Prefer `read()` when you
    /// also need health, since each call re-acquires.
    pub fn info(&self) -> Result<Option<AdapterOutput<WorkstateInfo>>, AdapterError> {
        Ok(self.read()?.1)
    }
}

impl Adapter for WorkstateAdapter {
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

    const SNAPSHOT_V3: &str = include_str!("../fixtures/workstate/snapshot_v3.json");

    #[test]
    fn parses_v3_fixture_with_all_three_sections() {
        let info = WorkstateAdapter::parse_feed(SNAPSHOT_V3)
            .expect("v3 fixture must parse")
            .expect("v3 fixture must be an accepted version");
        assert_eq!(info.schema_version, 3);
        assert_eq!(info.populated_section_count(), 3);

        // scripts.data reuses ScriptsInfo verbatim.
        let scripts = info.scripts.data.as_ref().expect("scripts data present");
        assert_eq!(scripts.total(), 3);
        assert_eq!(scripts.favorites_count(), 1);
        assert_eq!(scripts.recents_count(), 2);

        // tools.data reuses ToolsInfo verbatim.
        let tools = info.tools.data.as_ref().expect("tools data present");
        assert_eq!(tools.tool_count, 2);
        assert_eq!(tools.attention_count, 1);
        assert_eq!(tools.tools.len(), 2);
        assert_eq!(tools.tools[0].review_after.as_deref(), Some("2026-09-01"));
        assert!(!tools.tools[0].review_due_flag);
        assert_eq!(tools.tools[1].review_after.as_deref(), Some("2026-09-01"));
        assert!(tools.tools[1].review_due_flag);

        // findings.data reuses FindingsInfo — its `items` aliases `findings[]`.
        let findings = info.findings.data.as_ref().expect("findings data present");
        assert_eq!(findings.items.len(), 4);
        let t = findings.risk_tally();
        assert_eq!(t.critical, 1);
        assert_eq!(t.high, 1);
        assert!(t.should_block());
    }

    #[test]
    fn section_status_maps_to_health() {
        let info = WorkstateAdapter::parse_feed(SNAPSHOT_V3).unwrap().unwrap();
        // The fixture's sections are all Stale → Degraded.
        assert_eq!(info.scripts.status, "Stale");
        assert_eq!(
            status_to_health(&info.scripts.status),
            AdapterHealth::Degraded
        );
        assert_eq!(
            status_to_health(&info.tools.status),
            AdapterHealth::Degraded
        );
        assert_eq!(
            status_to_health(&info.findings.status),
            AdapterHealth::Degraded
        );
    }

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
    fn provenance_carries_source_observed_at() {
        let info = WorkstateAdapter::parse_feed(SNAPSHOT_V3).unwrap().unwrap();
        assert_eq!(info.tools.provenance.feed_id, "tools");
        assert_eq!(
            info.tools.provenance.source_observed_at.as_deref(),
            Some("2026-06-02T00:00:00Z")
        );
    }

    #[test]
    fn unknown_major_version_is_graceful_skip() {
        let v99 = r#"{"schema_version": 99, "built_at": "x"}"#;
        assert!(WorkstateAdapter::parse_feed(v99)
            .expect("must not error")
            .is_none());
    }

    #[test]
    fn old_v1_version_is_graceful_skip() {
        // Unsupported v1 projects[] snapshots are skipped rather than treated as errors.
        let v1 = r#"{"schema_version": 1, "source_tool": "workstate", "projects": []}"#;
        assert!(WorkstateAdapter::parse_feed(v1)
            .expect("must not error")
            .is_none());
    }

    #[test]
    fn missing_version_is_graceful_skip() {
        let no_ver = r#"{"built_at": "x"}"#;
        assert!(WorkstateAdapter::parse_feed(no_ver)
            .expect("must not error")
            .is_none());
    }

    #[test]
    fn missing_section_data_is_none_not_error() {
        // A section reported Missing carries no `data` — must parse to None.
        let feed = r#"{"schema_version": 3, "built_at": "x",
            "scripts": {"status": "Missing", "provenance": {"feed_id": "scripts"}},
            "tools":   {"status": "Missing", "provenance": {"feed_id": "tools"}},
            "findings":{"status": "Missing", "provenance": {"feed_id": "findings"}}}"#;
        let info = WorkstateAdapter::parse_feed(feed).unwrap().unwrap();
        assert_eq!(info.populated_section_count(), 0);
        assert!(info.scripts.data.is_none());
        assert_eq!(
            status_to_health(&info.scripts.status),
            AdapterHealth::Unavailable
        );
    }

    #[test]
    fn malformed_json_is_a_parse_error() {
        let err = WorkstateAdapter::parse_feed("{not json").unwrap_err();
        assert!(matches!(err, AdapterError::JsonParse(_)));
    }

    #[test]
    fn info_roundtrips_via_serde() {
        let info = WorkstateAdapter::parse_feed(SNAPSHOT_V3).unwrap().unwrap();
        let json = serde_json::to_string(&info).unwrap();
        let back: WorkstateInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }

    #[test]
    fn with_text_reads_from_memory_without_touching_disk_or_stdin() {
        let a = WorkstateAdapter::with_text(SNAPSHOT_V3);
        let (health, out) = a.read().expect("read ok");
        assert_eq!(health, AdapterHealth::Healthy);
        let out = out.expect("v3 feed present");
        assert_eq!(out.adapter, "workstate");
        assert_eq!(out.data.populated_section_count(), 3);
    }

    #[test]
    fn missing_path_is_unavailable_not_error() {
        let a = WorkstateAdapter::with_path("/no/such/rexops/workstate/xyz123.json");
        assert!(a.info().expect("must not error").is_none());
        assert_eq!(a.health(), AdapterHealth::Unavailable);
    }
}
