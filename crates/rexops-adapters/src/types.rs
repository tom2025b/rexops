//! types.rs — Common data types shared by all adapters.
//!
//! This module contains the vocabulary types used across the adapter layer:
//! - AdapterHealth: a small closed enum describing the runtime state of an
//!   external tool adapter.
//! - AdapterOutput<T>: a generic envelope that wraps every "successful data
//!   fetch" result so that callers always receive context (which adapter,
//!   which version, what health) together with the payload.
//!
//! Why an envelope instead of just returning the inner T?
//! - The adapter layer is intentionally *read-only* and *thin*. The caller
//!   (rexops-core, CLI, TUI) often wants to know "was bulwark healthy when we
//!   asked it for the scan?" without making a second call.
//! - It makes logging/telemetry uniform: one place to attach adapter name +
//!   version to any result.
//! - It stays serializable for cached or transported results.
//!
//! Constraints:
//! - All types here are Sync + Send + 'static by default (no lifetimes).
//! - No execution logic; pure data.
//! - Keep this file tiny (< 120 LOC including comments) — it is imported by
//!   almost everything.

use serde::{Deserialize, Serialize};

/// Runtime health classification for an adapter (and its backing binary).
///
/// The health value is *observed*, not declared. It is computed by probing the
/// external binary (presence + successful version query + successful no-op
/// command). Adapters may return Degraded for "works but old version" etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterHealth {
    /// Binary present, responds to --version / --help, and basic commands succeed.
    Healthy,

    /// Binary present and basically works, but some capability is reduced
    /// (example: very old version that lacks a scan flag we rely on).
    Degraded,

    /// Binary not found on PATH, or the adapter has been administratively
    /// disabled. This is a normal, non-error condition for optional tools.
    Unavailable,

    /// Probe itself failed in an unexpected way (I/O error, permission, etc.).
    /// Higher layers should treat Unknown similarly to Degraded but log loudly.
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
///
/// Every adapter method that can succeed with a payload returns
/// `Result<AdapterOutput<ConcreteType>, AdapterError>`.
///
/// The envelope is deliberately small and stable so additive fields can remain
/// non-breaking for most callers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterOutput<T> {
    /// Stable identifier for the adapter implementation ("bulwark", ...).
    pub adapter: String,

    /// Version string returned by the external binary's --version (or
    /// equivalent). None if the binary could not report a version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Health snapshot captured at the time the data was produced.
    pub health: AdapterHealth,

    /// The actual typed result from the external tool.
    pub data: T,
}

impl<T> AdapterOutput<T> {
    /// Construct a minimal envelope. Version can be attached afterwards
    /// with `with_version` when it is known.
    pub fn new(adapter: impl Into<String>, health: AdapterHealth, data: T) -> Self {
        Self {
            adapter: adapter.into(),
            version: None,
            health,
            data,
        }
    }

    /// Builder-style setter for the detected version string.
    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }
}
