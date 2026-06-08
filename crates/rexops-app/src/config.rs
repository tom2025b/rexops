//! config.rs — Shared loader for AppConfig.
//!
//! This is the single implementation of "find a config file in the documented
//! search order, deserialize, validate, or fall back to defaults".
//!
//! Shared by rexops-cli and rexops-tui so both front-ends stay in sync.

use std::path::Path;

use rexops_core::AppConfig;

/// Load AppConfig, searching common locations as documented in examples/config.yaml.
///
/// Search order (first match wins):
///   1. ./examples/config.yaml   (handy during development)
///   2. ./rexops.yaml
///   3. ./rexops.yml
///
/// If nothing is found, or any step fails (read, parse, validate), we return
/// AppConfig::default() which enables every optional adapter. This matches the
/// "adapters are optional by design" rule: missing config must never break the
/// tools.
///
/// Callers (CLI, TUI) should usually call this once at startup and then clone
/// the AppConfig into worker threads when needed (it is cheap: small HashMap).
pub fn load_config() -> AppConfig {
    let candidate_paths = ["./examples/config.yaml", "./rexops.yaml", "./rexops.yml"];
    for p in &candidate_paths {
        if Path::new(p).exists() {
            if let Ok(contents) = std::fs::read_to_string(p) {
                if let Ok(cfg) = serde_yaml::from_str::<AppConfig>(&contents) {
                    if cfg.validate().is_ok() {
                        return cfg;
                    }
                    // If validate fails we fall through to default (silent is ok
                    // for a dev tool; a real app might log a warning here).
                }
            }
        }
    }
    AppConfig::default()
}
