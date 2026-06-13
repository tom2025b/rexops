//! adapter_types.rs — Vocabulary types shared by all adapters and the core domain.
//!
//! Moved to core so that OpsSnapshot (a core type) can own AdapterHealth fields
//! without requiring core to depend on the adapter execution layer. Adapters
//! import these types from here.

use serde::{Deserialize, Serialize};

/// Runtime health classification for an adapter (and its backing binary).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterHealth {
    /// Binary present, responds to probes, and basic commands succeed.
    Healthy,

    /// Binary present and basically works, but some capability is reduced.
    Degraded,

    /// Binary not found on PATH, or the adapter has been administratively
    /// disabled. This is a normal, non-error condition for optional tools.
    Unavailable,

    /// Probe itself failed in an unexpected way (I/O error, permission, etc.).
    Unknown,
}

impl AdapterHealth {
    /// Convenience predicate: is the adapter usable for real work right now?
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Healthy | Self::Degraded)
    }
}

/// Generic envelope produced by any call that returns structured data from
/// an adapter (scan, list, inspect, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterOutput<T> {
    /// Stable identifier for the adapter implementation ("bulwark", ...).
    pub adapter: String,

    /// Version string returned by the external binary's --version (or equivalent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Health snapshot captured at the time the data was produced.
    pub health: AdapterHealth,

    /// The actual typed result from the external tool.
    pub data: T,
}

impl<T> AdapterOutput<T> {
    pub fn new(adapter: impl Into<String>, health: AdapterHealth, data: T) -> Self {
        Self {
            adapter: adapter.into(),
            version: None,
            health,
            data,
        }
    }

    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn is_available_is_true_only_for_healthy_and_degraded() {
        assert!(AdapterHealth::Healthy.is_available());
        assert!(AdapterHealth::Degraded.is_available());
        assert!(!AdapterHealth::Unavailable.is_available());
        assert!(!AdapterHealth::Unknown.is_available());
    }

    #[test]
    fn adapter_health_serializes_snake_case() {
        // The #[serde(rename_all = "snake_case")] contract: variants are lowercase
        // strings, which is what cross-tool snapshots on disk depend on.
        assert_eq!(
            serde_json::to_string(&AdapterHealth::Healthy).unwrap(),
            "\"healthy\""
        );
        assert_eq!(
            serde_json::to_string(&AdapterHealth::Unavailable).unwrap(),
            "\"unavailable\""
        );
        let back: AdapterHealth = serde_json::from_str("\"degraded\"").unwrap();
        assert_eq!(back, AdapterHealth::Degraded);
    }

    #[test]
    fn output_new_has_no_version_until_set() {
        let out = AdapterOutput::new("bulwark", AdapterHealth::Healthy, 42u32);
        assert_eq!(out.adapter, "bulwark");
        assert_eq!(out.health, AdapterHealth::Healthy);
        assert_eq!(out.data, 42);
        assert!(out.version.is_none(), "version is None until with_version");
    }

    #[test]
    fn with_version_attaches_and_is_chainable() {
        let out =
            AdapterOutput::new("system", AdapterHealth::Degraded, "data").with_version("1.2.3");
        assert_eq!(out.version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn output_skips_none_version_in_serialization() {
        // version uses skip_serializing_if = "Option::is_none" — a None version
        // must not emit the key at all, keeping snapshots lean.
        let out = AdapterOutput::new("x", AdapterHealth::Healthy, 1u8);
        let json = serde_json::to_string(&out).unwrap();
        assert!(
            !json.contains("version"),
            "absent version omitted, got: {json}"
        );

        let out = out.with_version("9");
        let json = serde_json::to_string(&out).unwrap();
        assert!(
            json.contains("\"version\":\"9\""),
            "present version emitted, got: {json}"
        );
    }
}
