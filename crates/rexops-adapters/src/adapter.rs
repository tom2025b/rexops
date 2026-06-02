//! adapter.rs — The synchronous Adapter trait (the narrow waist of the layer).
//!
//! Every adapter must implement this trait. The three methods give a uniform
//! answer to the questions "is the tool installed?" and "is it healthy right
//! now?" without any domain-specific knowledge.
//!
//! This trait is synchronous by design. The adapters crate in this phase
//! performs no I/O except short-lived blocking calls to external CLIs.
//! Adding async would multiply complexity (and pull in tokio) for zero gain
//! at the scale RexOps currently targets.
//!
//! Additional capabilities (scan, etc.) are provided as inherent methods on
//! the concrete adapter type (BulwarkAdapter::scan), not on the trait. This
//! keeps the common interface tiny and stable while allowing each adapter to
//! grow its own vocabulary.

use crate::error::AdapterError;
use crate::types::AdapterHealth;

/// The minimal contract that all RexOps adapters satisfy.
///
/// Implementors are expected to be cheap to construct (usually unit-struct
/// or a tiny newtype holding the binary name) and safe to use from multiple
/// threads (Send + Sync).
pub trait Adapter {
    /// Fast, best-effort check whether the backing executable can be located.
    ///
    /// Returns `false` for "not on PATH" or "permission denied". Never panics.
    /// This is the method you call first in any "maybe use this adapter" path.
    fn check_available(&self) -> bool;

    /// Attempt to obtain a human-readable version string from the tool.
    ///
    /// Typical implementation: run `<binary> --version`, take first whitespace-
    /// separated token, strip a leading "v".
    ///
    /// Returns:
    /// - Ok(Some("1.4.2")) — success
    /// - Ok(None) — binary existed but output was empty or unparseable
    /// - Err — hard failure (I/O, timeout, permission after spawn, ...)
    fn version(&self) -> Result<Option<String>, AdapterError>;

    /// Composite health derived from presence + version probe.
    ///
    /// Recommended implementation sketch:
    ///   if !check_available() { return Unavailable; }
    ///   match version() {
    ///       Ok(Some(_)) => Healthy,
    ///       Ok(None)    => Degraded,
    ///       Err(_)      => Degraded, // or Unknown for some errors
    ///   }
    fn health(&self) -> AdapterHealth;
}

// Learning Notes:
// - Trait objects are not used here; we prefer concrete types + generics at
//   the call sites (e.g. fn foo<A: Adapter>(a: &A)) so monomorphization keeps
//   the binary small and we avoid vtables in hot paths.
// - No associated types or GATs — we want 1.75 compatibility and simplicity.
// - The trait does *not* own the "run a domain command" surface. That lives on
//   the concrete type so that adding a new adapter never requires touching this
//   file.
