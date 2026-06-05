//! scripts.rs — Workstate scripts data types.
//!
//! These types model the `scripts.data` payload in the Workstate v3 snapshot.
//!
//! Read-only, serde-friendly, no execution logic.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One script entry. Provisional contract: only a few fields are named;
/// everything else is preserved in `rest` so we never lose data and never reject
/// unknown keys.
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

/// The whole scripts payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ScriptsInfo {
    /// `#[serde(default)]` because the Workstate v3 snapshot carries the version
    /// at the snapshot envelope level, not inside this `data` payload.
    #[serde(default)]
    pub schema_version: i64,
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

impl ScriptsInfo {
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

// Learning Notes:
// - `Script` keeps unknown keys in `rest` via `#[serde(flatten)]` so future
//   Workstate additions won't break parsing or lose data.
// - favorites/recents are id arrays, not per-script booleans. is_favorite() is an
//   opportunistic membership check for UI stars only — never relied on for
//   correctness (provisional ids may not line up, yielding no stars).
