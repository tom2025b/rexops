//! workstate.rs — Read-only consumer for the Workstate *snapshot* feed.
//!
//! Reads an exported, versioned Workstate JSON (per-project/repo health) from
//! in-memory text (routed piped stdin) or the documented standard path. Mirrors
//! scriptvault.rs, toolfoundry.rs and bulwark_feed.rs exactly:
//! - never reads stdin directly (the snapshot layer reads it once and routes),
//! - single read() returning (health, Option<data>),
//! - a schema_version gate that SKIPS unknown versions instead of erroring.
//!
//! Target shape: ../linux-ops-suite/contracts/workstate.snapshot.schema.json,
//! which is explicitly PROVISIONAL. Workstate emits nothing yet; the contract
//! fixes only the envelope (schema_version, source_tool, generated_at) plus a
//! free-form `projects[]` array (additionalProperties: true). So we type the
//! envelope and keep each `Project` loose (a couple of opportunistic optionals +
//! flattened rest) — a future change to Workstate's item shape will not break us.
//!
//! Read-only: never writes back, never spawns a binary. Workstate is itself
//! strictly read-only and must never mutate repos, so there is nothing to mutate
//! on our side either.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapter::Adapter;
use crate::error::AdapterError;
use crate::types::{AdapterHealth, AdapterOutput};

/// The major schema version this consumer understands.
const SUPPORTED_SCHEMA_VERSION: i64 = 1;

/// One observed project/repository entry. Provisional: only a couple of fields
/// are named; everything else is preserved in `rest` so we never lose data and
/// never reject unknown keys.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Project {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Everything else the snapshot carried, kept verbatim (additionalProperties).
    #[serde(flatten)]
    pub rest: BTreeMap<String, Value>,
}

impl Project {
    /// Best display label: path, then name, then "<unknown>". Path wins because a
    /// snapshot of repos is most usefully identified by where it lives on disk.
    pub fn label(&self) -> &str {
        self.path
            .as_deref()
            .or(self.name.as_deref())
            .unwrap_or("<unknown>")
    }
}

/// The whole Workstate snapshot envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct WorkstateInfo {
    pub schema_version: i64,
    /// Lenient: should be "workstate" but we don't reject a mismatch.
    #[serde(default)]
    pub source_tool: String,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub projects: Vec<Project>,
}

impl WorkstateInfo {
    /// Total projects in the snapshot.
    pub fn project_count(&self) -> usize {
        self.projects.len()
    }
}

/// Tiny probe to read just the version before a full parse.
#[derive(Debug, Deserialize)]
struct VersionProbe {
    schema_version: Option<i64>,
}

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

    /// Acquire raw feed text by precedence: in-memory text → explicit path →
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
        let probe: VersionProbe = serde_json::from_str(text)?;
        match probe.schema_version {
            Some(v) if v == SUPPORTED_SCHEMA_VERSION => {
                let info: WorkstateInfo = serde_json::from_str(text)?;
                Ok(Some(info))
            }
            _ => Ok(None),
        }
    }

    /// Acquire + parse in a SINGLE read, returning both the health and (on a
    /// supported version) the parsed feed.
    ///   (Healthy, Some)     → supported-version feed parsed.
    ///   (Degraded, None)    → feed present, unknown/missing version.
    ///   (Unavailable, None) → no feed found.
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

    const SNAPSHOT_V1: &str = include_str!("../fixtures/workstate/snapshot_v1.json");

    #[test]
    fn parses_v1_fixture_with_counts() {
        let info = WorkstateAdapter::parse_feed(SNAPSHOT_V1)
            .expect("v1 fixture must parse")
            .expect("v1 fixture must be an accepted version");
        assert_eq!(info.schema_version, 1);
        assert_eq!(info.source_tool, "workstate");
        assert_eq!(info.project_count(), 3);

        // Path-first labelling.
        let rexops = info
            .projects
            .iter()
            .find(|p| p.name.as_deref() == Some("rexops"))
            .unwrap();
        assert_eq!(rexops.label(), "/home/tom/projects/rexops");
    }

    #[test]
    fn unknown_major_version_is_graceful_skip() {
        let v99 = r#"{"schema_version": 99, "source_tool": "workstate", "projects": []}"#;
        assert!(WorkstateAdapter::parse_feed(v99)
            .expect("must not error")
            .is_none());
    }

    #[test]
    fn missing_version_is_graceful_skip() {
        let no_ver = r#"{"source_tool": "workstate", "projects": []}"#;
        assert!(WorkstateAdapter::parse_feed(no_ver)
            .expect("must not error")
            .is_none());
    }

    #[test]
    fn lenient_source_tool_does_not_reject() {
        let feed = r#"{"schema_version": 1, "source_tool": "not-ws", "projects": []}"#;
        let info = WorkstateAdapter::parse_feed(feed).unwrap().unwrap();
        assert_eq!(info.source_tool, "not-ws");
    }

    #[test]
    fn malformed_json_is_a_parse_error() {
        let err = WorkstateAdapter::parse_feed("{not json").unwrap_err();
        assert!(matches!(err, AdapterError::JsonParse(_)));
    }

    #[test]
    fn unknown_project_fields_are_preserved_in_rest() {
        let feed = r#"{"schema_version": 1, "source_tool": "workstate",
                       "projects": [{"name": "a", "ahead": 3}]}"#;
        let info = WorkstateAdapter::parse_feed(feed).unwrap().unwrap();
        assert_eq!(info.projects[0].rest.get("ahead").unwrap(), &Value::from(3));
    }

    #[test]
    fn label_falls_back_to_name_then_unknown() {
        let name_only = Project {
            name: Some("proj".into()),
            ..Default::default()
        };
        assert_eq!(name_only.label(), "proj");
        assert_eq!(Project::default().label(), "<unknown>");
    }

    #[test]
    fn info_roundtrips_via_serde() {
        let info = WorkstateInfo {
            schema_version: 1,
            source_tool: "workstate".into(),
            generated_at: "2026-06-04".into(),
            projects: vec![Project {
                name: Some("rexops".into()),
                path: Some("/home/tom/projects/rexops".into()),
                ..Default::default()
            }],
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: WorkstateInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }

    #[test]
    fn with_text_reads_from_memory_without_touching_disk_or_stdin() {
        let a = WorkstateAdapter::with_text(SNAPSHOT_V1);
        let (health, out) = a.read().expect("read ok");
        assert_eq!(health, AdapterHealth::Healthy);
        let out = out.expect("v1 feed present");
        assert_eq!(out.adapter, "workstate");
        assert_eq!(out.data.project_count(), 3);
    }

    #[test]
    fn missing_path_is_unavailable_not_error() {
        let a = WorkstateAdapter::with_path("/no/such/rexops/workstate/xyz123.json");
        assert!(a.info().expect("must not error").is_none());
        assert_eq!(a.health(), AdapterHealth::Unavailable);
    }
}

// Learning Notes:
// - Sixth contract-feed consumer, identical pattern to toolfoundry.rs,
//   bulwark_feed.rs and scriptvault.rs: in-memory-text → path → standard-path
//   acquisition (NEVER stdin directly), a single read() returning health + data,
//   and a version gate that skips unknown versions gracefully.
// - The Workstate contract is a PROVISIONAL stub: it has NO risk/severity fields,
//   only a free-form projects[] array. So this adapter intentionally contributes
//   NO risk to the dashboard — it just exposes a structured field + summary notes.
//   Only Bulwark feeds the risk pane.
// - Project.label() prefers `path` over `name` because a snapshot of repositories
//   is most usefully identified by its on-disk location. label() never panics on
//   a project missing both (returns "<unknown>").
// - As with every feed consumer, unknown project keys land in `rest` so a future
//   richer Workstate shape costs us nothing and breaks nothing.
