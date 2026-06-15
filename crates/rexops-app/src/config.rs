//! config.rs — Shared loader for AppConfig.
//!
//! This is the single implementation of "find a config file in the documented
//! search order, deserialize, validate, or fall back to defaults".
//!
//! Shared by rexops-cli and rexops-tui so both front-ends stay in sync.

use std::path::PathBuf;

use rexops_core::AppConfig;

/// Build the config search path, highest precedence first.
///
/// Stable, location-independent paths come FIRST so RexOps behaves the same no
/// matter which directory it's run from (the old loader led with
/// `./examples/config.yaml`, so `rexops` picked up the dev sample inside the repo
/// and nothing outside it — same binary, different behaviour by CWD). A
/// project-local `./rexops.yaml` is still honoured, but only after the stable
/// per-user/system locations, and `examples/config.yaml` is no longer searched —
/// it's a sample to copy, not a live config.
///
///   1. `$XDG_CONFIG_HOME/rexops/config.yaml`  (or `~/.config/rexops/config.yaml`)
///   2. `/etc/rexops/config.yaml`              (system-wide)
///   3. `./rexops.yaml`, `./rexops.yml`        (project-local override)
fn config_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Per-user: prefer XDG_CONFIG_HOME, else ~/.config.
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()) {
        paths.push(PathBuf::from(dir).join("rexops/config.yaml"));
    } else if let Some(home) = std::env::var_os("HOME").filter(|s| !s.is_empty()) {
        paths.push(PathBuf::from(home).join(".config/rexops/config.yaml"));
    }

    // System-wide.
    paths.push(PathBuf::from("/etc/rexops/config.yaml"));

    // Project-local override (lowest precedence).
    paths.push(PathBuf::from("./rexops.yaml"));
    paths.push(PathBuf::from("./rexops.yml"));

    paths
}

/// Load AppConfig from the first usable file in [`config_search_paths`].
///
/// On success, the chosen path is announced to **stderr** so the user knows which
/// config is in effect (stdout stays clean for `--json`). If nothing is found we
/// fall back to `AppConfig::default()` — which enables every optional adapter —
/// and say so, matching the "adapters are optional by design" rule: a missing
/// config must never break the tool.
///
/// Callers (CLI, TUI) should call this once at startup and clone the AppConfig
/// into worker threads as needed (it is cheap: a small HashMap).
pub fn load_config() -> AppConfig {
    for path in config_search_paths() {
        if !path.exists() {
            continue;
        }
        let p = path.display();
        // A config file is PRESENT here. Falling back to defaults silently when
        // it can't be used would hide a real misconfiguration (the user thinks
        // their settings apply; they don't), so each failure mode warns to
        // stderr before falling through. A merely-absent file stays silent —
        // that's the normal "adapters are optional" case.
        match std::fs::read_to_string(&path) {
            Err(e) => eprintln!("rexops: config {p} could not be read ({e}); using defaults"),
            Ok(contents) => match serde_yaml::from_str::<AppConfig>(&contents) {
                Err(e) => eprintln!("rexops: config {p} is not valid YAML ({e}); using defaults"),
                Ok(cfg) => match cfg.validate() {
                    Ok(()) => {
                        eprintln!("rexops: using config {p}");
                        return cfg;
                    }
                    Err(e) => {
                        eprintln!("rexops: config {p} failed validation ({e}); using defaults");
                    }
                },
            },
        }
    }
    eprintln!("rexops: no config file found; using built-in defaults (all adapters enabled)");
    AppConfig::default()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn search_paths_lead_with_stable_locations_not_cwd() {
        // The CR-3 fix: stable, CWD-independent paths must come BEFORE the
        // project-local ./rexops.yaml, and ./examples must not be searched at all.
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg-test");
        let paths = config_search_paths();
        std::env::remove_var("XDG_CONFIG_HOME");

        let strs: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();

        assert_eq!(
            strs.first().map(String::as_str),
            Some("/tmp/xdg-test/rexops/config.yaml"),
            "XDG config must be the highest-precedence path, got: {strs:?}"
        );
        assert!(
            strs.iter().any(|p| p == "/etc/rexops/config.yaml"),
            "system-wide path must be present: {strs:?}"
        );

        // ./rexops.yaml is still honoured, but only after the stable paths.
        let local = strs
            .iter()
            .position(|p| p == "./rexops.yaml")
            .expect("local path present");
        let etc = strs
            .iter()
            .position(|p| p == "/etc/rexops/config.yaml")
            .expect("etc present");
        assert!(
            local > etc,
            "project-local config must rank below stable paths: {strs:?}"
        );

        // The old CWD footgun must be gone entirely.
        assert!(
            !strs.iter().any(|p| p.contains("examples/config.yaml")),
            "examples/config.yaml must no longer be a search path: {strs:?}"
        );
    }
}
