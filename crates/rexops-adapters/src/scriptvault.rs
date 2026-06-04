//! scriptvault.rs — Read-only consumer for the ScriptVault *export* feed.
//!
//! Reads an exported, versioned ScriptVault JSON (script inventory + favorites +
//! recents) from in-memory text (routed piped stdin) or the documented standard
//! path. Mirrors toolfoundry.rs and bulwark_feed.rs exactly:
//! - never reads stdin directly (the snapshot layer reads it once and routes),
//! - single read() returning (health, Option<data>),
//! - a schema_version gate that SKIPS unknown versions instead of erroring.
//!
//! Target shape: ../linux-ops-suite/contracts/scriptvault.export.schema.json,
//! which is explicitly PROVISIONAL. It fixes the envelope (schema_version,
//! source_tool, generated_at) plus `scripts[]` (free-form), `favorites[]` and
//! `recents[]` (string id arrays). So we type the envelope and the id arrays, but
//! keep each `Script` loose (a couple of opportunistic optionals + flattened
//! rest) — a future change to ScriptVault's item shape will not break us.
//!
//! Read-only: never writes back, never spawns a binary.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapter::Adapter;
use crate::error::AdapterError;
use crate::types::{AdapterHealth, AdapterOutput};

/// The major schema version this consumer understands.
const SUPPORTED_SCHEMA_VERSION: i64 = 1;

/// One script entry. Provisional: only a few fields are named; everything else is
/// preserved in `rest` so we never lose data and never reject unknown keys.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Script {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Everything else the export carried, kept verbatim (additionalProperties).
    #[serde(flatten)]
    pub rest: BTreeMap<String, Value>,
}

impl Script {
    /// Best display label: id, then name, then "<unnamed>".
    pub fn label(&self) -> &str {
        self.id
            .as_deref()
            .or(self.name.as_deref())
            .unwrap_or("<unnamed>")
    }
}

/// The whole ScriptVault export envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ScriptVaultInfo {
    pub schema_version: i64,
    /// Lenient: should be "scriptvault" but we don't reject a mismatch.
    #[serde(default)]
    pub source_tool: String,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub scripts: Vec<Script>,
    /// Favorite script ids.
    #[serde(default)]
    pub favorites: Vec<String>,
    /// Recently launched script ids.
    #[serde(default)]
    pub recents: Vec<String>,
}

impl ScriptVaultInfo {
    /// Total scripts in the inventory.
    pub fn total(&self) -> usize {
        self.scripts.len()
    }

    /// Number of favorite ids.
    pub fn favorites_count(&self) -> usize {
        self.favorites.len()
    }

    /// Number of recent ids.
    pub fn recents_count(&self) -> usize {
        self.recents.len()
    }

    /// Opportunistic membership check: is this script flagged as a favorite?
    /// Matches by id, falling back to name. Never a correctness dependency — a
    /// provisional feed without matching ids simply yields no stars.
    pub fn is_favorite(&self, script: &Script) -> bool {
        self.favorites.iter().any(|f| {
            Some(f.as_str()) == script.id.as_deref() || Some(f.as_str()) == script.name.as_deref()
        })
    }
}

/// Tiny probe to read just the version before a full parse.
#[derive(Debug, Deserialize)]
struct VersionProbe {
    schema_version: Option<i64>,
}

/// Read-only ScriptVault export consumer.
///
/// Acquisition precedence: in-memory text (`with_text`) → explicit path
/// (`with_path`) → the documented standard path. Never reads stdin itself.
#[derive(Debug, Clone, Default)]
pub struct ScriptVaultAdapter {
    text_override: Option<String>,
    path_override: Option<PathBuf>,
}

impl ScriptVaultAdapter {
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
    ///   $XDG_DATA_HOME/rexops/feeds/scriptvault.export.json
    /// falling back to ~/.local/share/rexops/feeds/... when XDG is unset.
    pub fn standard_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
        Some(base.join("rexops/feeds/scriptvault.export.json"))
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
    pub fn parse_feed(text: &str) -> Result<Option<ScriptVaultInfo>, AdapterError> {
        let probe: VersionProbe = serde_json::from_str(text)?;
        match probe.schema_version {
            Some(v) if v == SUPPORTED_SCHEMA_VERSION => {
                let info: ScriptVaultInfo = serde_json::from_str(text)?;
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
    ) -> Result<(AdapterHealth, Option<AdapterOutput<ScriptVaultInfo>>), AdapterError> {
        let Some(text) = self.read_feed_text()? else {
            return Ok((AdapterHealth::Unavailable, None));
        };
        match Self::parse_feed(&text)? {
            Some(info) => {
                let out = AdapterOutput::new("scriptvault", AdapterHealth::Healthy, info);
                Ok((AdapterHealth::Healthy, Some(out)))
            }
            None => Ok((AdapterHealth::Degraded, None)),
        }
    }

    /// Convenience: just the parsed feed (drops health). Prefer `read()` when you
    /// also need health, since each call re-acquires.
    pub fn info(&self) -> Result<Option<AdapterOutput<ScriptVaultInfo>>, AdapterError> {
        Ok(self.read()?.1)
    }
}

impl Adapter for ScriptVaultAdapter {
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

    const EXPORT_V1: &str = include_str!("../fixtures/scriptvault/export_v1.json");

    #[test]
    fn parses_v1_fixture_with_counts() {
        let info = ScriptVaultAdapter::parse_feed(EXPORT_V1)
            .expect("v1 fixture must parse")
            .expect("v1 fixture must be an accepted version");
        assert_eq!(info.schema_version, 1);
        assert_eq!(info.source_tool, "scriptvault");
        assert_eq!(info.total(), 3);
        assert_eq!(info.favorites_count(), 1);
        assert_eq!(info.recents_count(), 2);

        // Opportunistic favorite membership by id.
        let deploy = info
            .scripts
            .iter()
            .find(|s| s.label() == "deploy-prod")
            .unwrap();
        assert!(info.is_favorite(deploy));
        let cleanup = info
            .scripts
            .iter()
            .find(|s| s.label() == "cleanup-logs")
            .unwrap();
        assert!(!info.is_favorite(cleanup));
    }

    #[test]
    fn unknown_major_version_is_graceful_skip() {
        let v99 = r#"{"schema_version": 99, "source_tool": "scriptvault", "scripts": []}"#;
        assert!(ScriptVaultAdapter::parse_feed(v99)
            .expect("must not error")
            .is_none());
    }

    #[test]
    fn missing_version_is_graceful_skip() {
        let no_ver = r#"{"source_tool": "scriptvault", "scripts": []}"#;
        assert!(ScriptVaultAdapter::parse_feed(no_ver)
            .expect("must not error")
            .is_none());
    }

    #[test]
    fn lenient_source_tool_does_not_reject() {
        let feed = r#"{"schema_version": 1, "source_tool": "not-sv", "scripts": []}"#;
        let info = ScriptVaultAdapter::parse_feed(feed).unwrap().unwrap();
        assert_eq!(info.source_tool, "not-sv");
    }

    #[test]
    fn malformed_json_is_a_parse_error() {
        let err = ScriptVaultAdapter::parse_feed("{not json").unwrap_err();
        assert!(matches!(err, AdapterError::JsonParse(_)));
    }

    #[test]
    fn unknown_script_fields_are_preserved_in_rest() {
        let feed = r#"{"schema_version": 1, "source_tool": "scriptvault",
                       "scripts": [{"id": "a", "future": 42}]}"#;
        let info = ScriptVaultAdapter::parse_feed(feed).unwrap().unwrap();
        assert_eq!(
            info.scripts[0].rest.get("future").unwrap(),
            &Value::from(42)
        );
    }

    #[test]
    fn info_roundtrips_via_serde() {
        let info = ScriptVaultInfo {
            schema_version: 1,
            source_tool: "scriptvault".into(),
            generated_at: "2026-06-04".into(),
            scripts: vec![Script {
                id: Some("a".into()),
                name: Some("a.sh".into()),
                ..Default::default()
            }],
            favorites: vec!["a".into()],
            recents: vec!["a".into()],
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: ScriptVaultInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }

    #[test]
    fn with_text_reads_from_memory_without_touching_disk_or_stdin() {
        let a = ScriptVaultAdapter::with_text(EXPORT_V1);
        let (health, out) = a.read().expect("read ok");
        assert_eq!(health, AdapterHealth::Healthy);
        let out = out.expect("v1 feed present");
        assert_eq!(out.adapter, "scriptvault");
        assert_eq!(out.data.total(), 3);
    }

    #[test]
    fn missing_path_is_unavailable_not_error() {
        let a = ScriptVaultAdapter::with_path("/no/such/rexops/scriptvault/xyz123.json");
        assert!(a.info().expect("must not error").is_none());
        assert_eq!(a.health(), AdapterHealth::Unavailable);
    }
}

// Learning Notes:
// - Third contract-feed consumer, identical pattern to toolfoundry.rs and
//   bulwark_feed.rs: in-memory-text → path → standard-path acquisition (NEVER
//   stdin directly), a single read() returning health + data, and a version gate
//   that skips unknown versions gracefully.
// - favorites/recents are id arrays per the contract, not per-script booleans
//   (the old stub's invention). The summary needs only counts (.len()), so we
//   avoid a fragile join over provisional, free-form script items.
// - is_favorite() is an OPPORTUNISTIC membership check for UI stars only — it is
//   never relied on for correctness, so a feed whose ids don't line up simply
//   shows no stars rather than misbehaving.
// - This adapter used to serve hardcoded demo data. It now reports Unavailable
//   when no feed is present — the same transition ToolFoundry made in Phase 3.
