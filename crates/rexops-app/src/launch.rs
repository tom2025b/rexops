//! launch.rs — Shared launch-command resolution for the cockpit.
//!
//! This is the *single* source of "what runs when you launch a tool", used by
//! both the TUI (which suspends/restores the terminal around the child) and the
//! `rexops launch` CLI command. Keeping it here — in the app crate both
//! front-ends already depend on — is what stops the two surfaces drifting on
//! program resolution or args (CR-2: the dry-run/preview must match what runs).
//!
//! It is pure apart from the `which` lookup: given a tool id and config it
//! resolves a program and the registry-owned args, or `None` when nothing is
//! launchable. Running the command is the caller's concern.

use std::process::Command;

use rexops_core::AppConfig;

/// Fully resolved child process invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl LaunchCommand {
    /// A single-line, copy-pasteable rendering of the command.
    pub fn display(&self) -> String {
        if self.args.is_empty() {
            self.program.clone()
        } else {
            format!("{} {}", self.program, self.args.join(" "))
        }
    }
}

/// Resolve the complete launch command for a tool, including any registry-owned
/// arguments needed to open its interactive surface. Args come from the
/// component's `LaunchSpec`; the program is resolved (config `binary` then
/// `which`) — the registry is the single source of launch data.
pub fn resolve_launch_command(tool_id: &str, config: &AppConfig) -> Option<LaunchCommand> {
    let program = resolve_program(tool_id, config)?;
    let args = rexops_core::component_by_id(tool_id)
        .and_then(|c| c.launch)
        .map(|l| l.args.iter().map(|arg| (*arg).to_owned()).collect())
        .unwrap_or_default();
    Some(LaunchCommand { program, args })
}

/// The program-only half: an explicitly configured `binary` wins, else the tool
/// on the user's PATH. `None` when neither yields a command (e.g. a feed-only
/// tool with no executable) or when the adapter is administratively disabled.
///
/// The config-over-PATH order is deliberate: `binary` is an administrative pin
/// (the same control surface as `enabled`), so a stray same-named binary on PATH
/// must not silently shadow the build an operator chose.
fn resolve_program(tool_id: &str, config: &AppConfig) -> Option<String> {
    if !config.adapter_enabled(tool_id) {
        return None;
    }
    command_from_config(tool_id, config).or_else(|| command_from_path(tool_id))
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

// Learning Notes
// - Moved here from `rexops-tui::tools::launcher` in Phase F so the CLI can
//   share it without depending on the TUI crate. rexops-tui re-exports
//   `LaunchCommand` + `resolve_launch_command`, so its call sites are unchanged.
// - Args read straight from `rexops_core::component_by_id(..).launch` — the
//   registry is the one launch source (Phase D), so no `catalog` shim is needed.

#[cfg(test)]
mod tests {
    use super::*;
    use rexops_core::AdapterConfig;

    fn config_with(tool: &str, cfg: AdapterConfig) -> AppConfig {
        let mut c = AppConfig::default();
        c.adapters.insert(tool.to_owned(), cfg);
        c
    }

    #[test]
    fn bulwark_resolves_with_its_registry_args() {
        // bulwark has a LaunchSpec (args: ["tui"]); a configured binary resolves
        // the program, and the args come from the registry.
        let cfg = config_with(
            "bulwark",
            AdapterConfig {
                enabled: true,
                binary: Some("/usr/bin/bulwark".to_owned()),
                timeout_secs: None,
                ..Default::default()
            },
        );
        let cmd = resolve_launch_command("bulwark", &cfg).expect("bulwark resolves");
        assert_eq!(cmd.program, "/usr/bin/bulwark");
        assert_eq!(cmd.args, vec!["tui".to_owned()]);
        assert_eq!(cmd.display(), "/usr/bin/bulwark tui");
    }

    #[test]
    fn proto_resolves_bare() {
        // proto's LaunchSpec carries no args.
        let cfg = config_with(
            "proto",
            AdapterConfig {
                enabled: true,
                binary: Some("/usr/bin/proto".to_owned()),
                timeout_secs: None,
                ..Default::default()
            },
        );
        let cmd = resolve_launch_command("proto", &cfg).expect("proto resolves");
        assert_eq!(cmd.program, "/usr/bin/proto");
        assert!(cmd.args.is_empty());
        assert_eq!(cmd.display(), "/usr/bin/proto");
    }

    #[test]
    fn disabled_adapter_never_resolves() {
        let cfg = config_with(
            "bulwark",
            AdapterConfig {
                enabled: false,
                binary: Some("/usr/bin/bulwark".to_owned()),
                timeout_secs: None,
                ..Default::default()
            },
        );
        assert!(
            resolve_launch_command("bulwark", &cfg).is_none(),
            "a disabled adapter must not resolve even with a configured binary"
        );
    }

    #[test]
    fn missing_binary_and_not_on_path_is_none() {
        let cfg = config_with(
            "bulwark",
            AdapterConfig {
                enabled: true,
                binary: Some("/nonexistent/bulwark-xyz-phasef".to_owned()),
                timeout_secs: None,
                ..Default::default()
            },
        );
        // A configured binary path resolves the program even if absent on disk —
        // existence is checked at spawn time, not here. So this resolves to the
        // configured path; the gate/exec reports a not-found at run time.
        let cmd = resolve_launch_command("bulwark", &cfg).expect("configured binary resolves");
        assert_eq!(cmd.program, "/nonexistent/bulwark-xyz-phasef");
    }

    #[test]
    fn no_binary_and_not_on_path_is_none() {
        // An id with no configured binary that is also not a real PATH binary
        // resolves to nothing — the program-only half yields None, so there is
        // no command regardless of any LaunchSpec. (Using a deliberately
        // bogus id keeps the test independent of what is installed on PATH.)
        let cfg = config_with(
            "rexops-no-such-tool-phasef",
            AdapterConfig {
                enabled: true,
                binary: None,
                timeout_secs: None,
                ..Default::default()
            },
        );
        assert!(resolve_launch_command("rexops-no-such-tool-phasef", &cfg).is_none());
    }
}
