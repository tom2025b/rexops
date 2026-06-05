//! tools.rs — ToolFoundry feed data types.
//!
//! These types are the canonical model for the ToolFoundry `rexops-feed` contract.
//! The `ToolFoundryAdapter` in the old `toolfoundry.rs` was the only consumer of
//! the raw ToolFoundry feed; now that RexOps reads exclusively from the Workstate v3
//! snapshot, the adapter is gone — but these types live on because `WorkstateInfo`
//! embeds them as its `tools.data` payload, and the rest of RexOps (core, app, TUI)
//! renders them directly.
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

/// One tool as reported by the ToolFoundry feed.
///
/// Required-by-contract fields (`id`, `display_name`, `lifecycle_state`, `status`)
/// are plain Strings; the rest use `#[serde(default)]` so a feed omitting an
/// optional field still parses. `additionalProperties: true` means serde silently
/// ignores any extra keys ToolFoundry may add later.
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
    /// "Is a review due?" flag. The raw ToolFoundry feed sends this as
    /// `review_due: bool`. The Workstate v3 snapshot instead sends `review_due`
    /// as a nullable due-DATE (with the real flag in a separate
    /// `review_due_flag`, which serde ignores as an unknown field). We tolerate
    /// the snapshot's explicit `null` here as `false` via `bool_or_null`. This
    /// field is parse-only — nothing in RexOps reads it.
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

/// The whole ToolFoundry feed payload, mirroring the contract exactly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ToolFoundryInfo {
    /// Integer major version. `#[serde(default)]` because the Workstate v3
    /// snapshot carries the version at the envelope level, not inside this
    /// `data` payload.
    #[serde(default)]
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

// Learning Notes:
// - `bool_or_null` is a targeted fix for a Workstate v3 snapshot quirk: the
//   snapshot sends `review_due` as a nullable date string (not a bool), so we
//   deserialize it as Option<bool> and unwrap to false. Marking it pub(crate)
//   keeps it internal to the adapter crate.
