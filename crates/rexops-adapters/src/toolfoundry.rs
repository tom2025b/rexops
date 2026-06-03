//! toolfoundry.rs — Stub adapter for ToolFoundry (ownership/lifecycle/health/symlinks).
//!
//! Per the architecture plan, this is the next adapter after ScriptVault.
//! Lightweight stub providing sample data for demo (no real binary needed).
//! Follows exact same patterns as scriptvault.rs and system.rs for consistency.
//! Reports Healthy with mock tools data (name, owner, health, symlink info).

use serde::{Deserialize, Serialize};

use crate::adapter::Adapter;
use crate::error::AdapterError;
use crate::exec::{run_optional, DEFAULT_TIMEOUT};
use crate::types::{AdapterHealth, AdapterOutput};

/// Sample tool metadata (read-only view from ToolFoundry).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Tool {
    pub name: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub health: String, // e.g. "healthy", "degraded"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symlink: Option<String>,
}

/// Envelope of "ToolFoundry" data for the demo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ToolFoundryInfo {
    #[serde(default)]
    pub tools: Vec<Tool>,
    pub total: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ToolFoundryAdapter;

impl ToolFoundryAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Always return sample data for demo.
    pub fn info(&self) -> Result<AdapterOutput<ToolFoundryInfo>, AdapterError> {
        let tools = vec![
            Tool {
                name: "bulwark".to_owned(),
                owner: "ops-team".to_owned(),
                health: "healthy".to_owned(),
                symlink: Some("/usr/local/bin/bulwark".to_owned()),
            },
            Tool {
                name: "rexops".to_owned(),
                owner: "devs".to_owned(),
                health: "healthy".to_owned(),
                symlink: None,
            },
            Tool {
                name: "old-tool".to_owned(),
                owner: "legacy".to_owned(),
                health: "degraded".to_owned(),
                symlink: Some("/opt/bin/old-tool".to_owned()),
            },
        ];
        let info = ToolFoundryInfo {
            tools: tools.clone(),
            total: tools.len(),
        };

        let health = self.health();
        let version = self.version().ok().flatten();
        let mut out = AdapterOutput::new("toolfoundry", health, info);
        if let Some(v) = version {
            out = out.with_version(v);
        }
        Ok(out)
    }
}

impl Adapter for ToolFoundryAdapter {
    fn check_available(&self) -> bool {
        // Demo: always available.
        true
    }

    fn version(&self) -> Result<Option<String>, AdapterError> {
        let out = run_optional("echo", &["toolfoundry-demo-v0.1"], DEFAULT_TIMEOUT)?;
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
    fn toolfoundry_adapter_is_healthy_for_demo() {
        let a = ToolFoundryAdapter::new();
        assert!(a.check_available());
        assert_eq!(a.health(), AdapterHealth::Healthy);
    }

    #[test]
    fn toolfoundry_info_returns_sample_data() {
        let a = ToolFoundryAdapter::new();
        let out = a.info().expect("info must succeed in demo");
        assert_eq!(out.adapter, "toolfoundry");
        let info = &out.data;
        assert_eq!(info.total, 3);
        assert!(info.tools.iter().any(|t| t.owner == "ops-team"));
    }

    #[test]
    fn toolfoundry_info_roundtrips_via_serde() {
        let info = ToolFoundryInfo {
            tools: vec![Tool {
                name: "test-tool".into(),
                owner: "test".into(),
                health: "healthy".into(),
                symlink: Some("/bin/test".into()),
            }],
            total: 1,
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: ToolFoundryInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }
}

// Learning Notes:
// - This stub mirrors ScriptVault exactly in structure for demo purposes.
// - Sample data includes symlink and health to showcase ToolFoundry's purpose
//   (ownership/lifecycle/health/symlinks) in the TUI/CLI without needing the
//   real tool.
// - Always Healthy to ensure it appears nicely in the Adapters screen and new
//   Tools screen.
// - If made real later, replace mock vec with actual command execution + parse,
//   exactly like the Bulwark path.
// - Reuses the same AdapterError, AdapterOutput, etc., proving the thin layer
//   design scales.
