//! scriptvault.rs — Stub read-only adapter for ScriptVault (metadata/favorites/recents).
//!
//! Per the architecture plan, this is the next adapter after System (lightweight,
//! read-only). Since no real `scriptvault` binary is present, it always provides
//! sample data for demo purposes and reports Healthy (or Unavailable if we
//! wanted to simulate missing, but we keep it available to show data).
//!
//! Follows the exact same patterns as system.rs and bulwark.rs:
//! - Small (<200 LOC).
//! - Adapter trait impl.
//! - info() returning AdapterOutput<ScriptVaultInfo>.
//! - Uses exec helpers only if we probe (here we don't need, for pure demo).
//! - thiserror via AdapterError, serde, etc.
//! - Tests for roundtrip, health, etc.

use serde::{Deserialize, Serialize};

use crate::adapter::Adapter;
use crate::error::AdapterError;
use crate::exec::run_optional;
use crate::exec::DEFAULT_TIMEOUT;
use crate::types::{AdapterHealth, AdapterOutput};

/// Sample script metadata (read-only view from ScriptVault).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Script {
    pub name: String,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub recent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Envelope of "ScriptVault" data for the demo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ScriptVaultInfo {
    #[serde(default)]
    pub scripts: Vec<Script>,
    pub total: usize,
    pub favorites: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ScriptVaultAdapter;

impl ScriptVaultAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Always return sample data for demo (no real binary).
    /// In a real impl this would run the external tool and parse.
    pub fn info(&self) -> Result<AdapterOutput<ScriptVaultInfo>, AdapterError> {
        // Hard-coded demo data (3 scripts, 1 favorite, 2 recent).
        let scripts = vec![
            Script {
                name: "deploy-prod.sh".to_owned(),
                favorite: true,
                recent: true,
                description: Some("Deploy to production with safety checks".to_owned()),
            },
            Script {
                name: "backup-db.sh".to_owned(),
                favorite: false,
                recent: true,
                description: None,
            },
            Script {
                name: "cleanup-logs.py".to_owned(),
                favorite: false,
                recent: false,
                description: Some("Rotate and compress old logs".to_owned()),
            },
        ];
        let info = ScriptVaultInfo {
            scripts: scripts.clone(),
            total: scripts.len(),
            favorites: scripts.iter().filter(|s| s.favorite).count(),
        };

        let health = self.health();
        let version = self.version().ok().flatten();
        let mut out = AdapterOutput::new("scriptvault", health, info);
        if let Some(v) = version {
            out = out.with_version(v);
        }
        Ok(out)
    }
}

impl Adapter for ScriptVaultAdapter {
    fn check_available(&self) -> bool {
        // Demo: always "available" (in real life we'd probe a binary).
        // To make it realistic we could check for a common binary, but we keep
        // it simple and always succeed so the TUI/CLI shows interesting data.
        true
    }

    fn version(&self) -> Result<Option<String>, AdapterError> {
        // Fake version via a harmless command (or return a constant).
        let out = run_optional("echo", &["scriptvault-demo-v0.1"], DEFAULT_TIMEOUT)?;
        Ok(out.map(|s| s.trim().to_owned()))
    }

    fn health(&self) -> AdapterHealth {
        if self.check_available() {
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
    fn scriptvault_adapter_is_healthy_for_demo() {
        let a = ScriptVaultAdapter::new();
        assert!(a.check_available());
        assert_eq!(a.health(), AdapterHealth::Healthy);
    }

    #[test]
    fn scriptvault_info_returns_sample_data() {
        let a = ScriptVaultAdapter::new();
        let out = a.info().expect("info must succeed in demo");
        assert_eq!(out.adapter, "scriptvault");
        let info = &out.data;
        assert_eq!(info.total, 3);
        assert_eq!(info.favorites, 1);
        assert!(info.scripts.iter().any(|s| s.favorite));
    }

    #[test]
    fn scriptvault_info_roundtrips_via_serde() {
        let info = ScriptVaultInfo {
            scripts: vec![Script {
                name: "test.sh".into(),
                favorite: true,
                recent: false,
                description: Some("demo".into()),
            }],
            total: 1,
            favorites: 1,
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: ScriptVaultInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }
}

// Learning Notes:
// - This is a pure demo adapter (no external binary required) so the rest of
//   the system can show a third data source in the TUI/CLI without needing
//   the real ScriptVault tool installed.
// - We still implement the full Adapter contract and return AdapterOutput<T>
//   exactly like the real adapters — this proves the "thin integration layer"
//   design works for any number of future tools.
// - Sample data is intentionally small and realistic (favorites + recents)
//   so the adapters screen and notes look useful when you press '2' or '3'.
// - If we later want to make it "real", we would replace the hard-coded
//   vec![] with a call to the external binary + parsing, exactly like Bulwark.
