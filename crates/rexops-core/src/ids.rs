//! ids.rs — Strongly-typed identifiers (newtypes) for tools and adapters.
//!
//! Using newtypes instead of raw String prevents accidental mixing of
//! adapter names with tool names, makes function signatures self-documenting,
//! and allows us to centralize validation/normalization rules in one place.
//!
//! Rules for all ids in RexOps:
//! - Must be non-empty after trimming whitespace.
//! - Are stored in normalized form (trimmed, original case preserved for display).
//! - Implement the usual traits for use in HashMap keys, sorting, etc.
//! - Serde roundtrips cleanly (serialize as the inner string).
//!
//! Construction is the only way to obtain a valid Id; the inner field is private.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// Stable identifier for an adapter implementation or Workstate section.
///
/// Newtype wrapper gives us type safety and a place to enforce invariants.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AdapterId(String);

impl AdapterId {
    /// Construct a validated AdapterId.
    ///
    /// Returns Err if the input is empty or whitespace-only.
    /// The stored value is trimmed but otherwise preserves case.
    pub fn new(raw: impl Into<String>) -> Result<Self, CoreError> {
        let s = raw.into().trim().to_owned();
        if s.is_empty() {
            return Err(CoreError::InvalidId(
                "adapter id must be non-empty".to_owned(),
            ));
        }
        Ok(Self(s))
    }

    /// Return the inner string slice (for display, serialization, map keys).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AdapterId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Display as the normalized inner value so that "{id}" in logs and
        // error messages is the same string you would put in config.
        f.write_str(&self.0)
    }
}

impl AsRef<str> for AdapterId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Stable identifier for a tool in the inventory (e.g. a script name, binary, or logical tool).
///
/// Follows the same validation and normalization rules as AdapterId so that
/// the two can be used uniformly in registries and snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolId(String);

impl ToolId {
    /// Construct a validated ToolId.
    ///
    /// Returns Err if the input is empty or whitespace-only after trim.
    pub fn new(raw: impl Into<String>) -> Result<Self, CoreError> {
        let s = raw.into().trim().to_owned();
        if s.is_empty() {
            return Err(CoreError::InvalidId("tool id must be non-empty".to_owned()));
        }
        Ok(Self(s))
    }

    /// Return the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ToolId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn adapter_id_new_rejects_empty_and_whitespace() {
        assert!(AdapterId::new("").is_err());
        assert!(AdapterId::new("   ").is_err());
        assert!(AdapterId::new("\t\n").is_err());
    }

    #[test]
    fn adapter_id_new_accepts_and_trims_and_preserves_case() {
        let id = AdapterId::new("  Bulwark  ").unwrap();
        assert_eq!(id.as_str(), "Bulwark");
        assert_eq!(id.to_string(), "Bulwark");
    }

    #[test]
    fn tool_id_roundtrips_through_serde() {
        let id = ToolId::new("my-script-42").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"my-script-42\"");
        let id2: ToolId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, id2);
    }

    #[test]
    fn invalid_id_error_is_actionable() {
        let err = ToolId::new("").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("tool id must be non-empty"));
    }
}
