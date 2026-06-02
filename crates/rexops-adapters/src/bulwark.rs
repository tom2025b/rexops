//! bulwark.rs — Bulwark content-inspection adapter (read-only).
//!
//! Wraps the `bulwark inspect scan --format json` CLI. Returns typed
//! AdapterOutput<BulwarkScanResult>. All vectors have #[serde(default)].
//! Never mutates; purely observational adapter.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::adapter::Adapter;
use crate::error::AdapterError;
use crate::exec::probe_version;
use crate::exec::{run_json, run_optional, DEFAULT_TIMEOUT};
use crate::types::{AdapterHealth, AdapterOutput};

/// Typed mirror of bulwark-inspect::InspectionResult (see upstream for schema).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BulwarkScanResult {
    #[serde(default)]
    pub findings: Vec<BulwarkFinding>,
    pub should_block: bool,
    pub should_redact: bool,
    pub max_severity: Option<BulwarkSeverity>,
    pub inspection_time_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BulwarkFinding {
    pub rule_id: String,
    pub description: String,
    pub severity: BulwarkSeverity,
    pub category: BulwarkCategory,
    pub location: BulwarkLocation,
    pub snippet: Option<String>,
    pub action: BulwarkAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulwarkSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulwarkCategory {
    SecretLeakage,
    Pii,
    PromptInjection,
    SensitiveData,
    #[serde(untagged)]
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum BulwarkLocation {
    JsonPath { path: String },
    ByteRange { start: usize, end: usize },
    Line { line: usize },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulwarkAction {
    Log,
    Redact,
    Block,
}

#[derive(Debug, Clone, Default)]
pub struct BulwarkAdapter {
    binary: String,
}

impl BulwarkAdapter {
    pub fn new() -> Self {
        Self {
            binary: "bulwark".to_owned(),
        }
    }

    pub fn with_binary(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
        }
    }

    pub fn binary(&self) -> &str {
        &self.binary
    }

    /// Invoke `bulwark inspect scan --format json --text <text>` and return envelope.
    pub fn scan(&self, text: &str) -> Result<AdapterOutput<BulwarkScanResult>, AdapterError> {
        let args = ["inspect", "scan", "--format", "json", "--text", text];
        let data: BulwarkScanResult = run_json(&self.binary, &args, DEFAULT_TIMEOUT)?;
        let health = self.health();
        let version = self.version().ok().flatten();
        let mut out = AdapterOutput::new("bulwark", health, data);
        if let Some(v) = version {
            out = out.with_version(v);
        }
        Ok(out)
    }
}

impl Adapter for BulwarkAdapter {
    fn check_available(&self) -> bool {
        matches!(
            run_optional(&self.binary, &["--help"], Duration::from_secs(3)),
            Ok(Some(_))
        )
    }

    fn version(&self) -> Result<Option<String>, AdapterError> {
        probe_version(&self.binary)
    }

    fn health(&self) -> AdapterHealth {
        if !self.check_available() {
            return AdapterHealth::Unavailable;
        }
        match self.version() {
            Ok(Some(_)) => AdapterHealth::Healthy,
            _ => AdapterHealth::Degraded,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../fixtures/bulwark/scan_sample.json");

    #[test]
    fn bulwark_scan_result_roundtrips_and_defaults() {
        let result: BulwarkScanResult = serde_json::from_str(SAMPLE).expect("fixture must parse");
        assert!(result.should_block);
        assert_eq!(result.findings.len(), 2);
        let json = serde_json::to_string(&result).unwrap();
        let result2: BulwarkScanResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result, result2);
    }

    #[test]
    fn bulwark_finding_category_custom_deserializes() {
        let json = r#"{"rule_id":"c","description":"d","severity":"low","category":"corp-x","location":{"type":"unknown"},"snippet":null,"action":"log"}"#;
        let f: BulwarkFinding = serde_json::from_str(json).unwrap();
        assert!(matches!(f.category, BulwarkCategory::Custom(ref s) if s == "corp-x"));
    }

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
    fn scan_missing_binary_is_binary_not_found() {
        let a = BulwarkAdapter::with_binary("rexops-no-bulwark-here-xyz");
        let err = a.scan("text").unwrap_err();
        assert!(matches!(err, AdapterError::BinaryNotFound { .. }));
    }
}
