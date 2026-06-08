//! system.rs — Lightweight read-only system information adapter.
//!
//! Provides basic host info using common commands (hostname, uname, uptime, df).
//! No single external "system" binary required; commands are usually present
//! on Unix-like systems. Degrades gracefully if individual commands fail.
//!
//! Gives the TUI and CLI local host facts for the ops cockpit.

use serde::{Deserialize, Serialize};

use crate::adapter::Adapter;
use crate::error::AdapterError;
use crate::exec::{run_optional, DEFAULT_TIMEOUT};
use crate::types::{AdapterHealth, AdapterOutput};

/// Basic system information collected from standard tools.
/// All fields optional so partial success (some cmds fail) still gives value.
/// Uses #[serde(default)] for robustness.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SystemInfo {
    /// Hostname (from `hostname` or `uname -n`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,

    /// Kernel/OS info (from `uname -sr` or similar).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel: Option<String>,

    /// Uptime string (from `uptime`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime: Option<String>,

    /// Simple disk usage summary lines (from `df -h` head).
    #[serde(default)]
    pub disk: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SystemAdapter;

impl SystemAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Collect system info. Individual command failures are tolerated
    /// (field stays None or vec stays short). Never returns hard error
    /// for missing tools — this adapter is meant to be always "there".
    pub fn info(&self) -> Result<AdapterOutput<SystemInfo>, AdapterError> {
        let mut info = SystemInfo::default();

        // hostname
        if let Ok(Some(h)) = run_optional("hostname", &[], DEFAULT_TIMEOUT) {
            let h = h.trim();
            if !h.is_empty() {
                info.hostname = Some(h.to_owned());
            }
        } else if let Ok(Some(u)) = run_optional("uname", &["-n"], DEFAULT_TIMEOUT) {
            let u = u.trim();
            if !u.is_empty() {
                info.hostname = Some(u.to_owned());
            }
        }

        // kernel
        if let Ok(Some(k)) = run_optional("uname", &["-sr"], DEFAULT_TIMEOUT) {
            let k = k.trim();
            if !k.is_empty() {
                info.kernel = Some(k.to_owned());
            }
        }

        // uptime
        if let Ok(Some(u)) = run_optional("uptime", &[], DEFAULT_TIMEOUT) {
            let u = u.trim();
            if !u.is_empty() {
                info.uptime = Some(u.to_owned());
            }
        }

        // disk (take a few lines)
        if let Ok(Some(d)) = run_optional("df", &["-h"], DEFAULT_TIMEOUT) {
            for line in d.lines().take(6) {
                let t = line.trim();
                if !t.is_empty() {
                    info.disk.push(t.to_owned());
                }
            }
        }

        let health = self.health();
        let version = self.version().ok().flatten();
        let mut out = AdapterOutput::new("system", health, info);
        if let Some(v) = version {
            out = out.with_version(v);
        }
        Ok(out)
    }
}

impl Adapter for SystemAdapter {
    fn check_available(&self) -> bool {
        // System adapter is "always" conceptually available (uses std + common cmds).
        // We still do a cheap probe so the trait contract is satisfied.
        // Use "true" or "echo" which are ubiquitous.
        run_optional("true", &[], std::time::Duration::from_secs(1)).is_ok()
    }

    fn version(&self) -> Result<Option<String>, AdapterError> {
        // Use uname for a "version" of the system info provider.
        let out = run_optional("uname", &["-v"], DEFAULT_TIMEOUT)?;
        Ok(out.and_then(|s| {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.chars().take(40).collect())
            }
        }))
    }

    fn health(&self) -> AdapterHealth {
        if self.check_available() {
            // Individual command failures do not make the adapter unavailable.
            AdapterHealth::Healthy
        } else {
            AdapterHealth::Unavailable
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn system_adapter_is_always_conceptually_available() {
        let a = SystemAdapter::new();
        // On any reasonable Unix, true/echo exists.
        assert!(a.check_available());
        let h = a.health();
        assert!(h == AdapterHealth::Healthy || h == AdapterHealth::Degraded);
    }

    #[test]
    fn system_info_collects_something_or_degrades_gracefully() {
        let a = SystemAdapter::new();
        let out = a.info().expect("info should not hard-fail");
        assert_eq!(out.adapter, "system");
        // At least one field or disk lines should be present on real systems,
        // but we don't assert hard because test envs vary.
        let info = &out.data;
        let has_something = info.hostname.is_some()
            || info.kernel.is_some()
            || info.uptime.is_some()
            || !info.disk.is_empty();
        assert!(has_something || out.health == AdapterHealth::Unavailable);
    }

    #[test]
    fn system_info_roundtrips_via_serde() {
        let info = SystemInfo {
            hostname: Some("testhost".into()),
            kernel: Some("Linux 6.1".into()),
            uptime: Some("up 3 days".into()),
            disk: vec!["/dev/sda1  50G  20G  30G  41% /".into()],
        };

        let json = serde_json::to_string(&info).unwrap();
        let back: SystemInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }
}
