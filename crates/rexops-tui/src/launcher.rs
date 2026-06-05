//! launcher.rs — TUI launch orchestration for specialist tools.
//!
//! This module decides *what* to launch and how to report the result. It does
//! not own terminal state; the caller supplies a ForegroundRunner that knows
//! how to suspend/restore the TUI around a child process.

use std::io;
use std::process::Command;

use rexops_core::AppConfig;

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

/// Resolve and launch any tool by id using the supplied terminal runner.
///
/// `tool_id` keys both the `which <tool_id>` PATH lookup and the per-adapter
/// config `binary` fallback; `name` is the display name used in messages.
///
/// Not every entry is launchable. When no command resolves we return a
/// no-refresh report saying so, and never call the runner.
pub fn launch_tool(
    tool_id: &str,
    name: &str,
    config: &AppConfig,
    runner: &mut impl ForegroundRunner,
) -> LaunchReport {
    let Some(command) = resolve_command(tool_id, config) else {
        return LaunchReport::no_refresh(format!("{name} has no launch command yet"));
    };

    match runner.run_foreground(&command) {
        Ok(ChildExit::Success) => LaunchReport::refresh(format!("{name} exited successfully")),
        Ok(ChildExit::Status(status)) => {
            LaunchReport::refresh(format!("{name} exited with status {status}"))
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => LaunchReport::no_refresh(format!(
            "{name} launch failed: binary not found ({command})"
        )),
        Err(err) => LaunchReport::no_refresh(format!("{name} launch failed: {err}")),
    }
}

/// Resolve a tool's launch target: prefer the user's PATH, then the per-adapter
/// configured binary. Returns None when neither yields a command (e.g. a
/// feed-only tool with no executable).
///
/// `pub(crate)` so the confirmation layer (PendingAction::preview) can show the
/// resolved command as a dry-run *without* spawning anything.
pub(crate) fn resolve_command(tool_id: &str, config: &AppConfig) -> Option<String> {
    command_from_path(tool_id).or_else(|| command_from_config(tool_id, config))
}

/// Prefer the user's PATH by asking the platform `which` command.
fn command_from_path(tool_id: &str) -> Option<String> {
    let output = Command::new("which").arg(tool_id).output().ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

/// Fall back to an explicit binary configured for this tool's adapter.
fn command_from_config(tool_id: &str, config: &AppConfig) -> Option<String> {
    config
        .adapters
        .get(tool_id)
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

    /// Build a config that pins a tool's binary to an explicit path.
    fn config_with_binary(tool_id: &str, binary: &str) -> AppConfig {
        let mut config = AppConfig::default();
        config.adapters.insert(
            tool_id.to_owned(),
            AdapterConfig {
                enabled: true,
                binary: Some(binary.to_owned()),
                timeout_secs: None,
            },
        );
        config
    }

    #[test]
    fn command_from_config_uses_trimmed_binary() {
        let config = config_with_binary("scripts", "  /tmp/scripts  ");
        assert_eq!(
            command_from_config("scripts", &config),
            Some("/tmp/scripts".to_owned())
        );
    }

    #[test]
    fn command_from_config_ignores_missing_or_empty_binary() {
        assert_eq!(command_from_config("scripts", &AppConfig::default()), None);

        let config = config_with_binary("scripts", "   ");
        assert_eq!(command_from_config("scripts", &config), None);
    }

    #[test]
    fn launch_tool_reports_success_and_refreshes() {
        // Use an id that is NOT on PATH so the config-binary fallback is what
        // resolves — otherwise a real `which <id>` hit on the dev/CI box would
        // win and make the launched-command assertion environment-dependent.
        let id = "definitely-not-a-real-tool-xyz";
        let config = config_with_binary(id, "/tmp/fake-tool");
        let mut runner = FakeRunner {
            exit: Ok(ChildExit::Success),
            called_with: None,
        };

        let report = launch_tool(id, "FakeTool", &config, &mut runner);

        assert_eq!(report.message(), "FakeTool exited successfully");
        assert!(report.should_refresh());
        assert_eq!(runner.called_with.as_deref(), Some("/tmp/fake-tool"));
    }

    #[test]
    fn launch_tool_without_command_reports_gracefully_and_skips_runner() {
        // A feed-only tool (no PATH binary, no config binary) must not error and
        // must not invoke the runner.
        let mut runner = FakeRunner {
            exit: Ok(ChildExit::Success),
            called_with: None,
        };

        let report = launch_tool(
            "definitely-not-a-real-tool-xyz",
            "Workstate",
            &AppConfig::default(),
            &mut runner,
        );

        assert_eq!(report.message(), "Workstate has no launch command yet");
        assert!(!report.should_refresh());
        assert!(runner.called_with.is_none(), "runner must not be called");
    }
}

// Learning Notes:
// - ForegroundRunner keeps terminal mechanics outside launch orchestration.
//   That lets App handle user intent while main.rs remains the terminal owner.
// - LaunchReport separates human-readable status from policy such as whether
//   RexOps should refresh after returning from a specialist.
// - launch_tool is generic over tool id: the id keys both `which` and the config
//   binary fallback. Entries with no executable resolve to None and get a
//   graceful "no launch command yet" report. The runner is never called, so
//   non-launchable entries are a normal case.
