//! launcher.rs — TUI launch orchestration for specialist tools.
//!
//! This module decides *what* to launch and how to report the result. It does
//! not own terminal state; the caller supplies a ForegroundRunner that knows
//! how to suspend/restore the TUI around a child process.

use std::io;

use rexops_core::AppConfig;

use super::catalog;

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

/// Fully resolved child process invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl LaunchCommand {
    pub fn display(&self) -> String {
        if self.args.is_empty() {
            self.program.clone()
        } else {
            format!("{} {}", self.program, self.args.join(" "))
        }
    }
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
/// `tool_id` keys both the PATH lookup (a std `$PATH` walk) and the per-adapter
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

/// Resolve a tool's launch target. Config is authoritative: an explicitly
/// configured `binary` wins, and only when none is configured do we fall back to
/// the tool on the user's PATH. Returns None when neither yields a command (e.g.
/// a feed-only tool with no executable), or when the adapter is administratively
/// disabled (`enabled: false`) — a disabled adapter never resolves to a command,
/// even when its binary is on PATH.
///
/// The config-over-PATH order is deliberate: `binary` is an administrative pin
/// (the same control surface as `enabled`), so a stray same-named binary on PATH
/// must not silently shadow the build an operator chose.
///
/// `pub(crate)` so the confirmation layer (PendingAction::preview) can show the
/// resolved command as a dry-run *without* spawning anything.
pub fn resolve_command(tool_id: &str, config: &AppConfig) -> Option<String> {
    if !adapter_enabled(tool_id, config) {
        return None;
    }
    command_from_config(tool_id, config).or_else(|| command_from_path(tool_id))
}

/// Whether an adapter is administratively enabled. An adapter absent from config
/// is enabled by default; one present with `enabled: false` is disabled. Mirrors
/// the snapshot layer's `map_or(true, |c| c.enabled)` semantics.
fn adapter_enabled(tool_id: &str, config: &AppConfig) -> bool {
    config
        .adapters
        .get(tool_id)
        .is_none_or(|adapter| adapter.enabled)
}

/// Resolve the complete launch command for a catalog tool, including any
/// catalog-owned arguments needed to open the interactive surface.
pub fn resolve_launch_command(tool_id: &str, config: &AppConfig) -> Option<LaunchCommand> {
    let program = resolve_command(tool_id, config)?;
    let args = catalog::by_id(tool_id)
        .map(|tool| {
            tool.launch_args
                .iter()
                .map(|arg| (*arg).to_owned())
                .collect()
        })
        .unwrap_or_default();
    Some(LaunchCommand { program, args })
}

/// Resolve a bare command name against the user's `PATH`, the way a shell does:
/// walk each `PATH` entry in order and return the first `entry/tool_id` that
/// exists and is executable. Returns the full resolved path.
///
/// This is a hermetic std-only lookup — no spawning the external `which` binary
/// (which may be absent on minimal images and pulls in its own PATH/quirks). A
/// name already containing a path separator is treated as an explicit path and
/// checked directly, not searched (mirroring shell behaviour).
fn command_from_path(tool_id: &str) -> Option<String> {
    search_path(tool_id, std::env::var_os("PATH").as_deref())
}

/// PATH resolution against an explicit `path_var` (so tests don't touch the
/// process-global `PATH`). Walks `path_var` entries in order, returning the
/// first executable `entry/tool_id`. A name with a separator is an explicit
/// path, checked directly rather than searched.
fn search_path(tool_id: &str, path_var: Option<&std::ffi::OsStr>) -> Option<String> {
    use std::path::Path;

    // An explicit path (contains a separator) isn't PATH-searched — check it
    // directly, like a shell running `./foo` or `/usr/bin/foo`.
    if tool_id.contains(std::path::MAIN_SEPARATOR) {
        let p = Path::new(tool_id);
        return is_executable_file(p).then(|| tool_id.to_owned());
    }

    std::env::split_paths(path_var?)
        .map(|dir| dir.join(tool_id))
        .find(|candidate| is_executable_file(candidate))
        .map(|candidate| candidate.to_string_lossy().into_owned())
}

/// Whether `path` is a regular file the current process may execute. On Unix we
/// check the executable permission bits; elsewhere, existence as a file is the
/// best portable proxy.
fn is_executable_file(path: &std::path::Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Any execute bit set (owner/group/other) — matches how a shell decides
        // a PATH hit is runnable without resolving the caller's uid/gid.
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
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
            },
        );
        config
    }

    #[test]
    fn resolve_command_returns_none_for_disabled_adapter() {
        // A disabled adapter must never resolve to a command, even when its
        // binary is explicitly configured (and would otherwise win the
        // config-fallback). This is the P1: enabled: false must be respected.
        let config = config_with_disabled_binary("scripts", "/tmp/scripts");
        assert_eq!(resolve_command("scripts", &config), None);
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

    #[test]
    fn configured_binary_overrides_a_binary_on_path() {
        // Config is authoritative: when a tool is BOTH configured with an
        // explicit binary AND present on PATH, resolution must return the
        // configured path — never the PATH hit. A stray same-named binary on
        // PATH must not silently shadow the build an operator pinned.
        //
        // We discover a real on-PATH command at runtime (sh is on any POSIX
        // box, but we resolve it rather than assume a path) so the test proves
        // "config wins over a genuine PATH hit" without hardcoding a location.
        let on_path = command_from_path("sh").expect("sh must be on PATH for this test");
        assert!(
            !on_path.is_empty(),
            "precondition: `which sh` resolved to a real path"
        );

        // Pin a DIFFERENT path in config for that same id. If PATH still won,
        // resolution would return `on_path`; config winning returns our pin.
        let pinned = "/tmp/pinned-sh-override";
        assert_ne!(pinned, on_path, "the pin must differ from the PATH hit");
        let config = config_with_binary("sh", pinned);

        assert_eq!(
            resolve_command("sh", &config),
            Some(pinned.to_owned()),
            "configured binary must win over the PATH hit"
        );
    }

    #[test]
    fn path_is_used_only_when_no_binary_is_configured() {
        // The fallback half of the contract: with NO configured binary, an
        // on-PATH tool still resolves (to the PATH location). This guards
        // against a reorder accidentally dropping the PATH fallback entirely.
        let on_path = command_from_path("sh").expect("sh must be on PATH for this test");
        assert_eq!(
            resolve_command("sh", &AppConfig::default()),
            Some(on_path),
            "with no config binary, PATH is the fallback"
        );
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

#[cfg(test)]
mod path_walk_tests {
    use super::*;
    use std::ffi::OsStr;
    use std::os::unix::fs::PermissionsExt;

    /// Write a file with the given mode into a fresh per-test temp dir; return
    /// (dir, path). The dir name includes the test name so parallel tests never
    /// collide. Tests pass the dir as an explicit PATH to `search_path`, so they
    /// never touch the process-global `PATH` (no env race with other tests).
    fn write_mode(name: &str, mode: u32) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "rexops-pathwalk-{}-{}",
            std::process::id(),
            name
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join(name);
        std::fs::write(&path, b"#!/bin/sh\n").expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("chmod");
        (dir, path)
    }

    #[test]
    fn path_walk_finds_an_executable_in_a_later_path_entry() {
        // The std walk must behave like a shell: scan PATH entries in order and
        // return the first executable hit — no external `which`. A junk dir comes
        // first; the tool only exists in our dir.
        let (dir, path) = write_mode("pw-exec-tool", 0o755);
        let path_var = format!("/nonexistent-rexops-xyz:{}", dir.display());

        let got = search_path("pw-exec-tool", Some(OsStr::new(&path_var)));
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            got,
            Some(path.to_string_lossy().into_owned()),
            "PATH walk must resolve the executable to its full path"
        );
    }

    #[test]
    fn path_walk_skips_a_non_executable_file() {
        // A same-named NON-executable file on PATH must not count as a hit — a
        // shell wouldn't run it, and neither do we (what the `which` shell-out
        // got for free, the std walk must reproduce).
        let (dir, _path) = write_mode("pw-plain-tool", 0o644); // no execute bits

        let got = search_path("pw-plain-tool", Some(OsStr::new(&dir.display().to_string())));
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(got, None, "a non-executable file is not a runnable PATH hit");
    }

    #[test]
    fn an_explicit_path_is_checked_directly_not_searched() {
        // A name containing a separator is an explicit path: check it as-is,
        // don't PATH-search. Empty PATH proves we did NOT rely on searching.
        let (dir, path) = write_mode("pw-explicit", 0o755);

        let got = search_path(&path.to_string_lossy(), Some(OsStr::new("")));
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            got,
            Some(path.to_string_lossy().into_owned()),
            "an explicit executable path resolves to itself"
        );
    }

    #[test]
    fn no_path_var_resolves_nothing() {
        // With no PATH at all, a bare name can't resolve.
        assert_eq!(search_path("anything", None), None);
    }
}
