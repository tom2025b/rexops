//! error.rs — Typed error type for core domain operations (config, snapshot, registries).
//!
//! CoreError is the single error type returned by every fallible public API
//! in rexops-core. It gives callers precise, exhaustive matching and excellent
//! actionable messages (e.g. suggestions for fixing a bad config).
//!
//! Design goals (non-negotiable, matching the adapters precedent):
//! - Every public fallible function returns `Result<T, CoreError>`.
//! - thiserror for Display + From + source chaining with zero boilerplate.
//! - Variants are specific: Config, Validation, Registry, Snapshot, etc.
//! - No catch-all "Other" string variant — add a typed case when needed.
//! - Messages include suggested fixes where a human can act (install, edit config).
//!
//! Core never wraps AdapterError directly as a variant; instead callers lift
//! health from AdapterOutput or map specific adapter failures into CoreError
//! variants when they cross the boundary (e.g. during snapshot construction).

use thiserror::Error;

/// The unified error type for all rexops-core operations.
///
/// Use this for config loading/validation, registry lookups, snapshot
/// invariants, and any pure data transformation that can fail.
#[derive(Error, Debug)]
pub enum CoreError {
    /// Configuration could not be loaded or parsed.
    ///
    /// The `source` (if present) contains the underlying serde or I/O error.
    /// The message should already include the attempted path for diagnostics.
    #[error("failed to load config: {message}")]
    ConfigLoad {
        /// Human description (path + what went wrong).
        message: String,
        /// Underlying cause for programmatic inspection (serde, io, etc.).
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Configuration is structurally valid but semantically invalid.
    ///
    /// Example: an adapter listed as enabled but with an empty binary name.
    #[error("invalid config: {0}")]
    ConfigValidation(String),

    /// A lookup in a registry (ToolRegistry, AdapterRegistry) failed.
    ///
    /// Callers should treat this as "not found" rather than a hard error in
    /// most UI paths; the registry APIs return Option for the happy case.
    #[error("registry lookup failed: {0}")]
    RegistryLookup(String),

    /// An invariant was violated while building or updating an OpsSnapshot.
    ///
    /// This is usually a programming error in the caller (e.g. mixing health
    /// from one adapter into another adapter's data). Should be rare.
    #[error("snapshot invariant violated: {0}")]
    SnapshotInvariant(String),

    /// A newtype constructor (ToolId, AdapterId) rejected an invalid value.
    ///
    /// Ids must be non-empty and are normalized (trimmed) on construction.
    #[error("invalid identifier: {0}")]
    InvalidId(String),
}

// Learning Notes:
// - Using struct variants with named fields makes error construction and
//   matching self-documenting at every call site.
// - We deliberately do not implement From<AdapterError> here. Lifting adapter
//   results into snapshots is a *domain decision* done in app/cli layers;
//   core only sees the already-normalized AdapterOutput or health values.
// - ConfigLoad carries an optional source so that the original serde_json or
//   io error is available for `error.chain()` style logging without losing
//   the nice top-level message.
// - Keep this enum small. New failure modes get their own variant so that
//   match arms in CLI/TUI can give tailored UX (banner vs fatal exit).
