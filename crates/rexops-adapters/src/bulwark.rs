//! bulwark.rs — Bulwark presence/health probe (read-only).
//!
//! RexOps consumes Bulwark's actual *findings* through the Workstate snapshot
//! (the `findings` section), not by invoking Bulwark directly — Workstate is
//! the single source of truth. This adapter therefore only PROBES Bulwark for
//! presence + version to report its [`AdapterHealth`]; it does not parse scan
//! output.
//!
//! It deliberately does not wrap a scan subcommand. An earlier version invoked
//! `bulwark inspect scan --format json --text <text>`, but current Bulwark has
//! no `inspect scan` command (it exposes `scan`/`scan --json` over configured
//! directories and `workstate-feed`), and nothing in RexOps ever called it —
//! findings arrive via the Workstate snapshot. That dead, stale-API path was
//! removed rather than rewritten to a contract no caller needs. Never mutates;
//! purely observational.

use std::time::Duration;

use crate::adapter::Adapter;
use crate::error::AdapterError;
use crate::exec::probe_version;
use crate::exec::{run_optional, DEFAULT_TIMEOUT};
use crate::types::AdapterHealth;

#[derive(Debug, Clone)]
pub struct BulwarkAdapter {
    binary: String,
    /// Hard timeout applied to every probe/scan spawn for this adapter. Set from
    /// config (`adapters.bulwark.timeout_secs`, else the global default) by the
    /// snapshot builder; defaults to [`DEFAULT_TIMEOUT`] when unset.
    timeout: Duration,
}

impl Default for BulwarkAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl BulwarkAdapter {
    pub fn new() -> Self {
        Self {
            binary: "bulwark".to_owned(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_binary(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Override the per-spawn timeout (chainable). Used by the snapshot builder to
    /// honour the configured `timeout_secs`.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn binary(&self) -> &str {
        &self.binary
    }

    /// Probe presence + version in ONE pass and derive health, so callers that
    /// want both (the snapshot builder) don't spawn the binary three times
    /// (`check_available` + `version` + `version` again). A single
    /// `<binary> --version` decides everything: a missing binary yields
    /// `(Unavailable, None)`; a present binary with a parseable version yields
    /// `(Healthy, Some(ver))`; present-but-unparseable yields `(Degraded, None)`.
    pub fn probe(&self) -> (AdapterHealth, Option<String>) {
        match probe_version(&self.binary, self.timeout) {
            Ok(Some(ver)) => (AdapterHealth::Healthy, Some(ver)),
            // Binary present but version unparseable/empty → Degraded.
            Ok(None) if self.binary_present() => (AdapterHealth::Degraded, None),
            // Binary genuinely absent (probe_version returns Ok(None) on ENOENT too,
            // so confirm absence) → Unavailable.
            Ok(None) => (AdapterHealth::Unavailable, None),
            Err(_) => (AdapterHealth::Degraded, None),
        }
    }

    /// Cheap presence check used only to disambiguate the `Ok(None)` version case
    /// (absent vs present-but-no-version). Kept separate so `probe` stays a single
    /// spawn on the common (healthy) path.
    fn binary_present(&self) -> bool {
        matches!(
            run_optional(&self.binary, &["--help"], self.timeout),
            Ok(Some(_))
        )
    }
}

impl Adapter for BulwarkAdapter {
    fn check_available(&self) -> bool {
        self.binary_present()
    }

    fn version(&self) -> Result<Option<String>, AdapterError> {
        probe_version(&self.binary, self.timeout)
    }

    fn health(&self) -> AdapterHealth {
        self.probe().0
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn adapter_reports_unavailable_when_binary_missing() {
        let a = BulwarkAdapter::with_binary("rexops-no-bulwark-here-xyz");
        assert!(!a.check_available());
        assert_eq!(a.health(), AdapterHealth::Unavailable);
        assert!(a.version().unwrap().is_none());
    }

    #[test]
    fn adapter_health_uses_real_binary() {
        let a = BulwarkAdapter::with_binary("echo");
        assert!(a.check_available());
        let h = a.health();
        assert!(h == AdapterHealth::Healthy || h == AdapterHealth::Degraded);
    }

    #[test]
    fn probe_missing_binary_is_unavailable_with_no_version() {
        // The only path RexOps actually exercises: a presence/version probe.
        // A missing binary must report Unavailable and yield no version — never
        // an error that would brick the snapshot build.
        let a = BulwarkAdapter::with_binary("rexops-no-bulwark-here-xyz");
        let (health, version) = a.probe();
        assert_eq!(health, AdapterHealth::Unavailable);
        assert!(version.is_none());
    }
}
