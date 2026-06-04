//! launcher.rs — TUI launch orchestration for specialist tools.
//!
//! This module decides *what* to launch and how to report the result. It does
//! not own terminal state; the caller supplies a ForegroundRunner that knows
//! how to suspend/restore the TUI around a child process.

use std::io;
use std::process::Command;

use rexops_core::AppConfig;

const SCRIPTVAULT_ADAPTER_ID: &str = "scriptvault";
const SCRIPTVAULT_COMMAND: &str = "scriptvault";

/// Small abstraction over "run this with the user's real terminal".
pub trait ForegroundRunner {
    fn run_foreground(&mut self, command: &str) -> io::Result<ChildExit>;
}

/// Child-process exit state reduced to what launch orchestration needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildExit {
    Success,
    Status(String),
}

/// User-facing result of a launch attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchReport {
    message: String,
    refresh_after_return: bool,
}

impl LaunchReport {
    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn should_refresh(&self) -> bool {
        self.refresh_after_return
    }

    fn no_refresh(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            refresh_after_return: false,
        }
    }

    fn refresh(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            refresh_after_return: true,
        }
    }
}

/// Resolve and launch ScriptVault using the supplied terminal runner.
pub fn launch_scriptvault(config: &AppConfig, runner: &mut impl ForegroundRunner) -> LaunchReport {
    let Some(command) = resolve_scriptvault_command(config) else {
        return LaunchReport::no_refresh(
            "ScriptVault launch unavailable: scriptvault not found on PATH and no config binary set",
        );
    };

    match runner.run_foreground(&command) {
        Ok(ChildExit::Success) => LaunchReport::refresh("ScriptVault exited successfully"),
        Ok(ChildExit::Status(status)) => {
            LaunchReport::refresh(format!("ScriptVault exited with status {status}"))
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => LaunchReport::no_refresh(format!(
            "ScriptVault launch failed: binary not found ({command})"
        )),
        Err(err) => LaunchReport::no_refresh(format!("ScriptVault launch failed: {err}")),
    }
}

/// Resolve the ScriptVault launch target for Stage 3.
fn resolve_scriptvault_command(config: &AppConfig) -> Option<String> {
    scriptvault_from_path().or_else(|| scriptvault_from_config(config))
}

/// Prefer the user's PATH by asking the platform `which` command.
fn scriptvault_from_path() -> Option<String> {
    let output = Command::new("which")
        .arg(SCRIPTVAULT_COMMAND)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

/// Fall back to an explicit ScriptVault binary configured for the adapter.
fn scriptvault_from_config(config: &AppConfig) -> Option<String> {
    config
        .adapters
        .get(SCRIPTVAULT_ADAPTER_ID)
        .and_then(|adapter| adapter.binary.as_deref())
        .map(str::trim)
        .filter(|binary| !binary.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use rexops_core::AdapterConfig;

    use super::*;

    struct FakeRunner {
        exit: io::Result<ChildExit>,
        called_with: Option<String>,
    }

    impl ForegroundRunner for FakeRunner {
        fn run_foreground(&mut self, command: &str) -> io::Result<ChildExit> {
            self.called_with = Some(command.to_owned());
            match &self.exit {
                Ok(exit) => Ok(exit.clone()),
                Err(err) => Err(io::Error::new(err.kind(), err.to_string())),
            }
        }
    }

    #[test]
    fn scriptvault_from_config_uses_trimmed_binary() {
        let mut config = AppConfig::default();
        config.adapters.insert(
            SCRIPTVAULT_ADAPTER_ID.to_owned(),
            AdapterConfig {
                enabled: true,
                binary: Some("  /tmp/scriptvault  ".to_owned()),
                timeout_secs: None,
            },
        );

        assert_eq!(
            scriptvault_from_config(&config),
            Some("/tmp/scriptvault".to_owned())
        );
    }

    #[test]
    fn scriptvault_from_config_ignores_missing_or_empty_binary() {
        assert_eq!(scriptvault_from_config(&AppConfig::default()), None);

        let mut config = AppConfig::default();
        config.adapters.insert(
            SCRIPTVAULT_ADAPTER_ID.to_owned(),
            AdapterConfig {
                enabled: true,
                binary: Some("   ".to_owned()),
                timeout_secs: None,
            },
        );

        assert_eq!(scriptvault_from_config(&config), None);
    }

    #[test]
    fn launch_scriptvault_reports_success_and_refreshes() {
        let mut config = AppConfig::default();
        config.adapters.insert(
            SCRIPTVAULT_ADAPTER_ID.to_owned(),
            AdapterConfig {
                enabled: true,
                binary: Some("/tmp/scriptvault".to_owned()),
                timeout_secs: None,
            },
        );
        let mut runner = FakeRunner {
            exit: Ok(ChildExit::Success),
            called_with: None,
        };

        let report = launch_scriptvault(&config, &mut runner);

        assert_eq!(report.message(), "ScriptVault exited successfully");
        assert!(report.should_refresh());
        assert!(runner.called_with.is_some());
    }
}

// Learning Notes:
// - ForegroundRunner keeps terminal mechanics outside launch orchestration.
//   That lets App handle user intent while main.rs remains the terminal owner.
// - LaunchReport separates human-readable status from policy such as whether
//   RexOps should refresh after returning from a specialist.
