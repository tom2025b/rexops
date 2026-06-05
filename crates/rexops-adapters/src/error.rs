//! error.rs — Typed error type for all adapter operations.
//!
//! This module defines AdapterError, the single error type returned by every
//! fallible function in the rexops-adapters crate. Using a dedicated error type
//! (instead of anyhow or Box<dyn Error>) gives callers precise, exhaustive
//! matching on failure modes without losing context.
//!
//! Design goals (non-negotiable):
//! - Every public fallible API returns Result<T, AdapterError>.
//! - No unwrap()/expect() in library code (enforced by #![deny] in lib.rs).
//! - thiserror for ergonomic Display + From impls and source chaining.
//! - Variants cover the realistic failure modes of external CLI adapters:
//!   missing binary, non-zero exit, JSON parse failure, timeout, and I/O.
//!
//! Why a single enum?
//! - Makes error handling uniform across adapters.
//! - Enables callers (executors, TUI) to decide policy per variant (e.g. treat
//!   BinaryNotFound as "adapter unavailable" rather than hard failure).
//! - Keeps the surface small; one import, one match.

use std::time::Duration;
use thiserror::Error;

/// The unified error type for adapter-layer operations.
///
/// All functions that talk to external binaries (check version, run scans, etc.)
/// must return `Result<..., AdapterError>`. This allows typed handling at the
/// call site and prevents silent failures or stringly-typed errors.
#[derive(Error, Debug)]
pub enum AdapterError {
    /// The required external binary was not found on PATH (or the explicit path).
    ///
    /// This is a *graceful* condition for optional adapters. Callers should
    /// typically map this to AdapterHealth::Unavailable rather than propagating
    /// as a hard error.
    #[error("binary not found: {binary}")]
    BinaryNotFound {
        /// Name of the missing executable (e.g. "bulwark").
        binary: String,
    },

    /// The external command ran but exited with a non-success status.
    ///
    /// We capture the exit code (if available on Unix) and the full stderr so
    /// that higher layers can log actionable diagnostics without re-running.
    #[error("command '{command}' failed with exit code {exit_code:?}: {stderr}")]
    CommandFailed {
        /// The executable that was invoked.
        command: String,
        /// Exit status if the OS provided one.
        exit_code: Option<i32>,
        /// Captured stderr (truncated by caller if extremely long).
        stderr: String,
    },

    /// Failed to deserialize JSON produced by the external tool.
    ///
    /// This usually indicates either a version skew (tool output changed) or
    /// a bug in our type definitions. The underlying serde_json error is
    /// preserved via #[from].
    #[error("JSON parse error from adapter output: {0}")]
    JsonParse(#[from] serde_json::Error),

    /// The command exceeded the configured timeout and was killed.
    ///
    /// Timeouts are implemented with a background thread + channel so that we
    /// remain synchronous and do not pull in tokio/async for the adapter layer.
    #[error("command timed out after {0:?}")]
    Timeout(Duration),

    /// An underlying I/O error occurred (spawning, reading pipes, kill, etc.).
    ///
    /// Converted automatically via #[from] so that `?` works cleanly inside
    /// the exec helpers.
    #[error("I/O error while running adapter command: {0}")]
    Io(#[from] std::io::Error),
}

// Learning Notes (for future maintainers / learners):
// - thiserror::Error gives us std::error::Error impl + Display for free.
// - Using struct variants (not tuple) makes pattern matching self-documenting.
// - BinaryNotFound is intentionally not "fatal" — the adapter layer is read-only
//   and adapters are expected to be optional at runtime.
// - We do *not* include a catch-all "Other(String)" variant. If a new mode
//   appears, add a typed variant so callers can react specifically.
// - From<serde_json::Error> and From<io::Error> let the exec helpers stay small
//   and still surface precise root causes.
