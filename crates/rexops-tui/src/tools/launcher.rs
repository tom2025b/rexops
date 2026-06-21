//! launcher.rs — TUI launch orchestration for specialist tools.
//!
//! This module decides *what* to launch and how to report the result. It does
//! not own terminal state; the caller supplies a ForegroundRunner that knows
//! how to suspend/restore the TUI around a child process.

use std::io;

use rexops_core::AppConfig;

// Launch-command resolution moved to `rexops_app::launch` in Phase F so the CLI
// can share it without depending on the TUI crate. We re-export the types here
// so this module (and `tools::`) keep their existing surface unchanged.
pub use rexops_app::{resolve_launch_command, LaunchCommand};

/// Small abstraction over "run this with the user's real terminal".
pub trait ForegroundRunner {
    fn run_foreground(&mut self, command: &LaunchCommand) -> io::Result<ChildExit>;
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
    let Some(command) = resolve_launch_command(tool_id, config) else {
        return LaunchReport::no_refresh(format!("{name} has no launch command yet"));
    };

    match runner.run_foreground(&command) {
        Ok(ChildExit::Success) => LaunchReport::refresh(format!("{name} exited successfully")),
        Ok(ChildExit::Status(status)) => {
            LaunchReport::refresh(format!("{name} exited with status {status}"))
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => LaunchReport::no_refresh(format!(
            "{name} launch failed: binary not found ({})",
            command.display()
        )),
        Err(err) => LaunchReport::no_refresh(format!("{name} launch failed: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use rexops_core::AdapterConfig;

    use super::*;

    struct FakeRunner {
        exit: io::Result<ChildExit>,
        called_with: Option<LaunchCommand>,
    }

    impl ForegroundRunner for FakeRunner {
        fn run_foreground(&mut self, command: &LaunchCommand) -> io::Result<ChildExit> {
            self.called_with = Some(command.clone());
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
                ..Default::default()
            },
        );
        config
    }

    /// Build a config that pins a tool's binary but administratively disables
    /// the adapter (`enabled: false`).
    fn config_with_disabled_binary(tool_id: &str, binary: &str) -> AppConfig {
        let mut config = AppConfig::default();
        config.adapters.insert(
            tool_id.to_owned(),
            AdapterConfig {
                enabled: false,
                binary: Some(binary.to_owned()),
                timeout_secs: None,
                ..Default::default()
            },
        );
        config
    }

    #[test]
    fn launch_tool_refuses_disabled_adapter_and_skips_runner() {
        // launch_tool must treat a disabled adapter as unlaunchable: report
        // gracefully and never touch the foreground runner.
        let config = config_with_disabled_binary("scripts", "/tmp/scripts");
        let mut runner = FakeRunner {
            exit: Ok(ChildExit::Success),
            called_with: None,
        };

        let report = launch_tool("scripts", "Scripts", &config, &mut runner);

        assert_eq!(report.message(), "Scripts has no launch command yet");
        assert!(!report.should_refresh());
        assert!(
            runner.called_with.is_none(),
            "disabled adapter must not spawn"
        );
    }

    // Note: the program/args resolution unit tests (config-over-PATH, trimming,
    // disabled-adapter, missing-binary) moved with `resolve_launch_command` into
    // `rexops_app::launch` in Phase F. The tests below exercise the TUI launch
    // *orchestration* (`launch_tool` + the runner) that still lives here.

    #[test]
    fn launch_tool_reports_success_and_refreshes() {
        // Use a fake id so the launched command is the configured binary
        // deterministically. (Config now wins over PATH regardless, but a fake
        // id also keeps any stray PATH hit out of the picture entirely.)
        let id = "definitely-not-a-real-tool-xyz";
        let config = config_with_binary(id, "/tmp/fake-tool");
        let mut runner = FakeRunner {
            exit: Ok(ChildExit::Success),
            called_with: None,
        };

        let report = launch_tool(id, "FakeTool", &config, &mut runner);

        assert_eq!(report.message(), "FakeTool exited successfully");
        assert!(report.should_refresh());
        assert_eq!(
            runner.called_with,
            Some(LaunchCommand {
                program: "/tmp/fake-tool".to_owned(),
                args: Vec::new(),
            })
        );
    }

    #[test]
    fn bulwark_launch_uses_tui_subcommand() {
        let config = config_with_binary("bulwark", "/tmp/bulwark");

        let command = resolve_launch_command("bulwark", &config).expect("bulwark resolves");

        assert_eq!(command.args, vec!["tui".to_owned()]);
        assert_eq!(command.display(), format!("{} tui", command.program));
    }

    #[test]
    fn proto_launch_uses_bare_interactive_picker() {
        let config = config_with_binary("proto", "/tmp/proto");

        let command = resolve_launch_command("proto", &config).expect("proto resolves");

        assert!(
            command.args.is_empty(),
            "Proto must launch bare so its own interactive picker can select a protocol"
        );
        assert_eq!(command.display(), command.program);
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
