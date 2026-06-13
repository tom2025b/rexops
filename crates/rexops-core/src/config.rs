//! config.rs — AppConfig and supporting configuration types.
//!
//! AppConfig is the deserialized form of the user's rexops configuration
//! (examples/config.yaml or user overrides). It is pure data — loading from
//! disk (file discovery, yaml/json parsing) lives in the caller (cli or app).
//!
//! Core responsibilities here:
//! - Define the exact shape that matches examples/config.yaml.
//! - Provide sensible defaults via serde and manual Default.
//! - Offer a validate() method that returns CoreError on semantic problems.
//! - Keep all fields optional where possible so that partial configs work.
//!
//! No side effects, no I/O, no environment variable reading in this module.
//! That keeps it trivial to unit-test with inline yaml strings.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// Top-level RexOps configuration.
///
/// This struct is the single source of truth for which adapters are enabled and
/// how RexOps should talk to them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    /// Schema version. Start at 1; bump only on breaking changes.
    /// Version 0 is rejected during validation.
    pub version: u32,

    /// Per-adapter configuration. Key is the adapter id ("bulwark").
    /// Unknown adapters in config are ignored (forward compat).
    #[serde(default)]
    pub adapters: HashMap<String, AdapterConfig>,

    /// Global defaults that adapters and other subsystems fall back to.
    #[serde(default)]
    pub defaults: Defaults,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: 1,
            adapters: HashMap::new(),
            defaults: Defaults::default(),
        }
    }
}

impl AppConfig {
    /// Whether a named adapter is enabled. An adapter absent from config is
    /// enabled by default; one present with `enabled: false` is disabled.
    pub fn adapter_enabled(&self, name: &str) -> bool {
        self.adapters.get(name).map_or(true, |c| c.enabled)
    }

    /// Validate the config for semantic correctness.
    ///
    /// Returns Ok(()) for a usable config. Returns CoreError::ConfigValidation
    /// for problems that a human can fix (e.g. enabled adapter with no binary).
    ///
    /// This is *not* I/O validation; call this after you have a parsed struct.
    pub fn validate(&self) -> Result<(), CoreError> {
        for (name, cfg) in &self.adapters {
            if cfg.enabled {
                // If enabled, we prefer a non-empty binary, but an empty/None
                // is acceptable — the adapter layer will just use its default
                // name and report Unavailable if the binary is missing.
                if let Some(b) = &cfg.binary {
                    if b.trim().is_empty() {
                        return Err(CoreError::ConfigValidation(format!(
                            "adapters.{name}.binary is present but empty; remove the key or provide a name"
                        )));
                    }
                }
            }
            // Adapter-specific validation belongs here.
        }

        // Version sanity: zero is never valid.
        if self.version == 0 {
            return Err(CoreError::ConfigValidation(
                "version must be >= 1".to_owned(),
            ));
        }

        Ok(())
    }
}

/// Configuration for a single adapter (e.g. the "bulwark" section).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AdapterConfig {
    /// Whether this adapter should be considered at all.
    /// Default true when the section exists; set false to administratively
    /// disable an adapter even if its binary is on PATH.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Explicit binary name or path. None means "use the adapter's built-in default".
    /// Example: "bulwark" or "/usr/local/bin/bulwark-dev".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>,

    /// Optional per-adapter timeout in seconds.
    /// Overrides the global default when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

/// Global defaults applied when an adapter (or other subsystem) does not
/// specify its own value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Defaults {
    /// Default timeout for any adapter operation (probe, scan, list, ...).
    /// Adapters may still hard-cap this for safety.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Whether to surface full stderr in error messages and JSON output.
    /// Useful for debugging; turn off in very sensitive environments.
    #[serde(default = "default_true")]
    pub include_stderr_in_errors: bool,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            timeout_secs: default_timeout(),
            include_stderr_in_errors: default_true(),
        }
    }
}

fn default_enabled() -> bool {
    true
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u64 {
    30
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid_and_has_sensible_defaults() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.version, 1);
        assert!(cfg.adapters.is_empty());
        assert_eq!(cfg.defaults.timeout_secs, 30);
        assert!(cfg.defaults.include_stderr_in_errors);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn config_with_enabled_adapter_but_empty_binary_is_invalid() {
        let mut cfg = AppConfig::default();
        cfg.adapters.insert(
            "bulwark".to_owned(),
            AdapterConfig {
                enabled: true,
                binary: Some("   ".to_owned()),
                timeout_secs: None,
            },
        );
        let err = cfg.validate().unwrap_err();
        assert!(matches!(err, CoreError::ConfigValidation(_)));
        assert!(err.to_string().contains("binary is present but empty"));
    }

    #[test]
    fn config_roundtrips_as_yaml_like_structure() {
        // This test proves that the struct shape matches the spirit of examples/config.yaml
        let mut cfg = AppConfig {
            version: 1,
            adapters: HashMap::new(),
            defaults: Defaults {
                timeout_secs: 45,
                include_stderr_in_errors: false,
            },
        };
        cfg.adapters.insert(
            "bulwark".to_owned(),
            AdapterConfig {
                enabled: true,
                binary: Some("bulwark".to_owned()),
                timeout_secs: Some(30),
            },
        );

        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let cfg2: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, cfg2);
        assert!(cfg2.validate().is_ok());
    }

    #[test]
    fn adapter_enabled_absent_key_defaults_to_true() {
        let cfg = AppConfig::default();
        assert!(cfg.adapter_enabled("bulwark"), "absent key → enabled");
        assert!(cfg.adapter_enabled("system"), "absent key → enabled");
    }

    #[test]
    fn adapter_enabled_respects_explicit_false() {
        let mut cfg = AppConfig::default();
        cfg.adapters.insert(
            "bulwark".to_owned(),
            AdapterConfig {
                enabled: false,
                binary: None,
                timeout_secs: None,
            },
        );
        assert!(!cfg.adapter_enabled("bulwark"), "explicit false → disabled");
        assert!(cfg.adapter_enabled("system"), "other adapters still default-on");
    }

    #[test]
    fn adapter_config_defaults_enabled_to_true() {
        // When a section exists in yaml without "enabled:", we want enabled=true.
        // Our default fn + #[serde(default = "...")] achieves this.
        let json = r#"{"enabled": false}"#; // explicit false
        let ac: AdapterConfig = serde_json::from_str(json).unwrap();
        assert!(!ac.enabled);

        let json2 = r"{}"; // omitted -> should be true via our default fn
        let ac2: AdapterConfig = serde_json::from_str(json2).unwrap();
        assert!(ac2.enabled);
    }
}
