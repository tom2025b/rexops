//! toolfoundry.rs — Read-only consumer for the ToolFoundry `rexops-feed`.
//!
//! ToolFoundry is the source of truth for tool lifecycle, ownership, health, and
//! drift. It emits a versioned JSON feed (`toolfoundry rexops-feed --json`) whose
//! shape is fixed by the contract:
//!   ../linux-ops-suite/contracts/toolfoundry.rexops-feed.schema.json
//!
//! This adapter is *purely* a consumer: it reads the feed (from stdin when piped,
//! otherwise from the documented standard path) and parses it into typed structs.
//! It NEVER writes back to ToolFoundry and never spawns the ToolFoundry binary —
//! it only reads bytes that ToolFoundry already produced.
//!
//! Why no JSON-Schema validator? The schema pins `schema_version` to `const: 1`,
//! but the requirement is to treat a missing/unknown *major* version gracefully.
//! A strict validator would hard-reject version 2 — the opposite of graceful.
//! So we parse with serde (like every other adapter) and gate the version
//! ourselves: version 1 → full parse; anything else → graceful skip (Ok(None)).

use std::io::{IsTerminal, Read};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::adapter::Adapter;
use crate::error::AdapterError;
use crate::types::{AdapterHealth, AdapterOutput};

/// The major schema version this consumer understands. Bumped only when we add
/// support for a new breaking ToolFoundry feed shape.
const SUPPORTED_SCHEMA_VERSION: i64 = 1;

/// One tool as reported by the ToolFoundry feed.
///
/// Required-by-contract fields (`id`, `display_name`, `lifecycle_state`,
/// `status`) are plain Strings; the rest use #[serde(default)] so that a feed
/// omitting an optional field still parses. The schema is `additionalProperties:
/// true`, so serde silently ignores any extra keys ToolFoundry may add later.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Tool {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub lifecycle_state: String,
    #[serde(default)]
    pub review_due: bool,
    #[serde(default)]
    pub health_passed: u32,
    #[serde(default)]
    pub health_total: u32,
    #[serde(default)]
    pub drifted: bool,
    /// Aggregate state, e.g. "ok" or "attention".
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub manifest_path: String,
}

impl Tool {
    /// True when this tool's aggregate status is "attention".
    pub fn needs_attention(&self) -> bool {
        self.status == "attention"
    }
}

/// The whole ToolFoundry feed, mirroring the contract exactly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ToolFoundryInfo {
    /// Integer major version. We only fully accept version 1.
    pub schema_version: i64,
    /// Date the feed was generated (YYYY-MM-DD).
    #[serde(default)]
    pub as_of: String,
    /// Total number of tools in the feed.
    #[serde(default)]
    pub tool_count: usize,
    /// Number of tools with status "attention".
    #[serde(default)]
    pub attention_count: usize,
    #[serde(default)]
    pub tools: Vec<Tool>,
}

/// Tiny probe used to read just the version before committing to a full parse.
/// Missing `schema_version` deserializes to `None` (graceful), not an error.
#[derive(Debug, Deserialize)]
struct VersionProbe {
    schema_version: Option<i64>,
}

/// Read-only ToolFoundry feed consumer.
///
/// Holds an optional explicit feed path (used by tests and future `--feed`
/// flags). When `None`, acquisition uses stdin-if-piped, else the standard path.
#[derive(Debug, Clone, Default)]
pub struct ToolFoundryAdapter {
    path_override: Option<PathBuf>,
}

impl ToolFoundryAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct an adapter that always reads from an explicit file path.
    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        Self {
            path_override: Some(path.into()),
        }
    }

    /// The documented standard read location for the feed:
    ///   $XDG_DATA_HOME/rexops/feeds/toolfoundry.rexops-feed.json
    /// falling back to ~/.local/share/rexops/feeds/... when XDG is unset.
    /// Returns None only if neither $XDG_DATA_HOME nor $HOME is set.
    pub fn standard_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
        Some(base.join("rexops/feeds/toolfoundry.rexops-feed.json"))
    }

    /// Acquire the raw feed text: stdin when it is piped (not a terminal),
    /// otherwise the explicit/standard path. Returns Ok(None) when no feed is
    /// available — a normal, non-error condition for an optional tool.
    fn read_feed_text(&self) -> Result<Option<String>, AdapterError> {
        // 1) Piped stdin wins, so `toolfoundry rexops-feed --json | rexops` works.
        if self.path_override.is_none() && !std::io::stdin().is_terminal() {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            if !buf.trim().is_empty() {
                return Ok(Some(buf));
            }
            // Empty pipe → fall through to the path.
        }

        // 2) Explicit override or the documented standard path.
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
    ///   version 1            → Ok(Some(info))
    ///   missing/other version → Ok(None)  (graceful skip; caller adds a note)
    /// Malformed JSON is still a hard JsonParse error so real bugs surface.
    pub fn parse_feed(text: &str) -> Result<Option<ToolFoundryInfo>, AdapterError> {
        let probe: VersionProbe = serde_json::from_str(text)?;
        match probe.schema_version {
            Some(v) if v == SUPPORTED_SCHEMA_VERSION => {
                let info: ToolFoundryInfo = serde_json::from_str(text)?;
                Ok(Some(info))
            }
            _ => Ok(None),
        }
    }

    /// Acquire + parse the feed in a **single read**, returning both the health
    /// and (when version 1) the parsed feed. This is the method callers should
    /// use: stdin can only be consumed once, so health and data must come from
    /// the same acquisition. Outcomes:
    ///   (Healthy, Some(out))     → a version-1 feed was read and parsed.
    ///   (Degraded, None)         → feed present but unknown/missing major version.
    ///   (Unavailable, None)      → no feed found (normal for an optional tool).
    ///   (Unknown, None) + Err    → I/O or malformed JSON.
    pub fn read(
        &self,
    ) -> Result<(AdapterHealth, Option<AdapterOutput<ToolFoundryInfo>>), AdapterError> {
        let Some(text) = self.read_feed_text()? else {
            return Ok((AdapterHealth::Unavailable, None));
        };
        match Self::parse_feed(&text)? {
            Some(info) => {
                let out = AdapterOutput::new("toolfoundry", AdapterHealth::Healthy, info);
                Ok((AdapterHealth::Healthy, Some(out)))
            }
            None => Ok((AdapterHealth::Degraded, None)),
        }
    }

    /// Convenience: just the parsed feed (drops the health). Prefer `read()`
    /// when you also need health, since each call re-acquires (and re-drains
    /// stdin).
    pub fn info(&self) -> Result<Option<AdapterOutput<ToolFoundryInfo>>, AdapterError> {
        Ok(self.read()?.1)
    }
}

impl Adapter for ToolFoundryAdapter {
    fn check_available(&self) -> bool {
        // "Available" means we can acquire a parseable v1 feed.
        matches!(self.read(), Ok((AdapterHealth::Healthy, _)))
    }

    fn version(&self) -> Result<Option<String>, AdapterError> {
        // We consume a feed, not a binary, so report the feed's schema version.
        match self.read()?.1 {
            Some(out) => Ok(Some(format!("schema_version={}", out.data.schema_version))),
            None => Ok(None),
        }
    }

    fn health(&self) -> AdapterHealth {
        // Note: this re-acquires the feed. With stdin input it can only be
        // called once; the snapshot builder uses read() to get health + data
        // together from a single acquisition.
        self.read().map_or(AdapterHealth::Unknown, |(h, _)| h)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const FEED_V1: &str = include_str!("../fixtures/toolfoundry/rexops_feed_v1.json");

    #[test]
    fn parses_v1_fixture_with_correct_counts() {
        let info = ToolFoundryAdapter::parse_feed(FEED_V1)
            .expect("v1 fixture must parse")
            .expect("v1 fixture must be an accepted version");
        assert_eq!(info.schema_version, 1);
        assert_eq!(info.as_of, "2026-06-02");
        assert_eq!(info.tool_count, 1);
        assert_eq!(info.attention_count, 1);
        assert_eq!(info.tools.len(), 1);
        let tool = &info.tools[0];
        assert_eq!(tool.id, "backup-home");
        assert_eq!(tool.status, "attention");
        assert!(tool.needs_attention());
        assert!(tool.drifted);
    }

    #[test]
    fn unknown_major_version_is_graceful_skip() {
        // A future breaking version must NOT error — we skip it cleanly.
        let v2 = r#"{"schema_version": 2, "as_of": "2027-01-01", "tool_count": 0,
                     "attention_count": 0, "tools": []}"#;
        let result = ToolFoundryAdapter::parse_feed(v2).expect("must not error on v2");
        assert!(result.is_none(), "v2 feed should be skipped gracefully");
    }

    #[test]
    fn missing_version_is_graceful_skip() {
        let no_version = r#"{"tool_count": 0, "attention_count": 0, "tools": []}"#;
        let result =
            ToolFoundryAdapter::parse_feed(no_version).expect("must not error on missing version");
        assert!(result.is_none(), "version-less feed should be skipped");
    }

    #[test]
    fn malformed_json_is_a_parse_error() {
        let err = ToolFoundryAdapter::parse_feed("{not json").unwrap_err();
        assert!(matches!(err, AdapterError::JsonParse(_)));
    }

    #[test]
    fn unknown_extra_fields_are_ignored() {
        // additionalProperties: true — forward-compatible by design.
        let extra = r#"{"schema_version": 1, "as_of": "2026-06-02", "tool_count": 0,
                        "attention_count": 0, "tools": [], "future_field": "x"}"#;
        let info = ToolFoundryAdapter::parse_feed(extra).unwrap().unwrap();
        assert_eq!(info.tool_count, 0);
    }

    #[test]
    fn info_roundtrips_via_serde() {
        let info = ToolFoundryInfo {
            schema_version: 1,
            as_of: "2026-06-02".into(),
            tool_count: 1,
            attention_count: 1,
            tools: vec![Tool {
                id: "t".into(),
                display_name: "T".into(),
                status: "attention".into(),
                ..Default::default()
            }],
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: ToolFoundryInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }

    #[test]
    fn explicit_path_reads_and_parses() {
        // with_path bypasses stdin entirely, so this is deterministic in CI.
        let dir = std::env::temp_dir().join("rexops-tf-test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("feed.json");
        std::fs::write(&p, FEED_V1).unwrap();

        let a = ToolFoundryAdapter::with_path(&p);
        let out = a.info().expect("read ok").expect("v1 feed present");
        assert_eq!(out.adapter, "toolfoundry");
        assert_eq!(out.health, AdapterHealth::Healthy);
        assert_eq!(out.data.tool_count, 1);

        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn missing_path_is_unavailable_not_error() {
        let a = ToolFoundryAdapter::with_path("/no/such/rexops/feed/xyz123.json");
        assert!(a.info().expect("must not error").is_none());
        assert_eq!(a.health(), AdapterHealth::Unavailable);
    }

    #[test]
    fn read_returns_health_and_info_from_one_acquisition() {
        // Critical for the stdin path: a single read() must yield BOTH health and
        // the parsed feed, because stdin can only be drained once.
        let dir = std::env::temp_dir().join("rexops-tf-read-test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("feed.json");
        std::fs::write(&p, FEED_V1).unwrap();

        let a = ToolFoundryAdapter::with_path(&p);
        let (health, out) = a.read().expect("read ok");
        assert_eq!(health, AdapterHealth::Healthy);
        let out = out.expect("v1 feed must produce data alongside Healthy");
        assert_eq!(out.data.tool_count, 1);

        std::fs::remove_file(&p).ok();
    }
}

// Learning Notes:
// - This adapter is the project's first *real* consumer of a sibling tool's
//   contract feed. Compare to bulwark.rs (spawns a binary) — here we only read
//   bytes ToolFoundry already wrote, honoring the "do not write back" rule and
//   the "RexOps observes, never reimplements" boundary.
// - The version gate (parse_feed) is the heart of "treat unknown versions
//   gracefully": we read `schema_version` with a tiny probe struct first, so a
//   future v2 feed is *skipped*, not rejected with an error.
// - stdin-if-piped (IsTerminal) lets `toolfoundry rexops-feed --json | rexops`
//   work with no new subcommand, while interactive runs fall back to the
//   documented XDG path. The path_override keeps tests hermetic.
// - #[serde(default)] on optional fields + additionalProperties:true (serde's
//   default ignore-unknown) make the type forward-compatible within a major
//   version: ToolFoundry can add fields without breaking us.
// - info() returns Result<Option<..>> not Result<..>: the Option encodes
//   "no usable feed" as a first-class, non-error outcome the cockpit can note.
