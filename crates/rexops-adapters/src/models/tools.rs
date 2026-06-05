//! tools.rs — Workstate tools data types.
//!
//! These types model the `tools.data` payload in the Workstate v3 snapshot.
//!
//! Read-only, serde-friendly, no execution logic.

use serde::{Deserialize, Serialize};

/// Deserialize a bool that may arrive as JSON `null` (or be absent) as `false`.
/// Needed because the Workstate v3 snapshot sends `review_due: null` (a nullable
/// date, with the real flag in `review_due_flag`), and plain `#[serde(default)]`
/// rejects an explicit `null` for a non-Option bool.
pub(crate) fn bool_or_null<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<bool>::deserialize(deserializer)?.unwrap_or(false))
}

/// One tool as reported by Workstate.
///
/// Required-by-contract fields (`id`, `display_name`, `lifecycle_state`, `status`)
/// are plain Strings; the rest use `#[serde(default)]` so a snapshot omitting an
/// optional field still parses. Serde silently ignores any extra keys Workstate
/// may add later.
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
    /// "Is a review due?" flag. Workstate may send `review_due` as null; treat
    /// that as false. This field is parse-only — nothing in RexOps reads it.
    #[serde(default, deserialize_with = "bool_or_null")]
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

/// The whole tools payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ToolsInfo {
    /// Integer major version. `#[serde(default)]` because the Workstate v3
    /// snapshot carries the version at the envelope level, not inside this
    /// `data` payload.
    #[serde(default)]
    pub schema_version: i64,
    /// Date the feed was generated (YYYY-MM-DD).
    #[serde(default)]
    pub as_of: String,
    /// Total number of tools.
    #[serde(default)]
    pub tool_count: usize,
    /// Number of tools with status "attention".
    #[serde(default)]
    pub attention_count: usize,
    #[serde(default)]
    pub tools: Vec<Tool>,
}

// Learning Notes:
// - `bool_or_null` tolerates an explicit null from Workstate while keeping the
//   public type simple.
