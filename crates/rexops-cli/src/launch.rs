//! launch.rs — the `rexops launch <tool>` command.
//!
//! Mirrors the TUI's confirm-before-run gate on the CLI. Resolution is shared
//! with the TUI via `rexops_app::launch` (one source of launch truth). The gate
//! decision is factored into the pure [`decide`] function so it is fully unit
//! tested; the actual stdin read and process exec are thin wrappers around it.

use std::io::{self, IsTerminal, Write};
use std::process::Command;

use rexops_app::{resolve_launch_command, LaunchCommand};
use rexops_core::AppConfig;

/// What the gate decided to do with a launch request. Pure and exhaustive so it
/// can be unit-tested without touching stdin or spawning a process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateOutcome {
    /// Don't launch; print `msg` to stderr and exit with `code`.
    Refuse { msg: String, code: u8 },
    /// Print the resolved command and exit 0 without running it.
    DryRun(LaunchCommand),
    /// Run the resolved command (foreground).
    Run(LaunchCommand),
    /// The user declined at the prompt; exit 0, run nothing.
    Aborted,
}

/// Decide what to do for a launch request, given the resolved command and the
/// gate inputs. `answer` is the user's prompt response (None when no prompt was
/// shown, e.g. `--yes`, `--dry-run`, or non-interactive).
///
/// Rules (see the Phase F design):
/// - unresolved tool            → Refuse(1)
/// - `--dry-run`                → DryRun (never runs, even with `--yes`)
/// - `--yes`                    → Run (skip the prompt)
/// - interactive, answered yes  → Run
/// - interactive, answered no   → Aborted (declining is not an error)
/// - non-interactive, no `--yes`→ Refuse(1) (don't hang; don't run blind)
pub fn decide(
    resolved: Option<LaunchCommand>,
    tool: &str,
    yes: bool,
    dry_run: bool,
    is_tty: bool,
    answer: Option<&str>,
) -> GateOutcome {
    let Some(cmd) = resolved else {
        return GateOutcome::Refuse {
            msg: format!(
                "'{tool}' is not launchable (no launch command — not on PATH, \
                 no configured binary, or the adapter is disabled)"
            ),
            code: 1,
        };
    };

    if dry_run {
        return GateOutcome::DryRun(cmd);
    }
    if yes {
        return GateOutcome::Run(cmd);
    }
    if !is_tty {
        return GateOutcome::Refuse {
            msg: "refusing to launch without confirmation; pass --yes to confirm \
                  non-interactively"
                .to_owned(),
            code: 1,
        };
    }
    match answer.map(str::trim) {
        Some("y") | Some("Y") | Some("yes") | Some("YES") => GateOutcome::Run(cmd),
        _ => GateOutcome::Aborted,
    }
}

/// Run `rexops launch <tool>`. Returns the process exit code.
pub fn run_launch(tool: &str, yes: bool, dry_run: bool, config: &AppConfig) -> u8 {
    let resolved = resolve_launch_command(tool, config);

    // Only prompt when we actually need an interactive answer.
    let is_tty = io::stdin().is_terminal();
    let need_prompt = resolved.is_some() && !yes && !dry_run && is_tty;
    let answer = if need_prompt {
        // Safe to unwrap the preview: need_prompt implies resolved.is_some().
        let preview = resolved.as_ref().unwrap().display();
        prompt_yes_no(&format!("Run: {preview}"))
    } else {
        None
    };

    match decide(resolved, tool, yes, dry_run, is_tty, answer.as_deref()) {
        GateOutcome::Refuse { msg, code } => {
            eprintln!("rexops: {msg}");
            code
        }
        GateOutcome::DryRun(cmd) => {
            println!("would run: {}", cmd.display());
            0
        }
        GateOutcome::Aborted => {
            eprintln!("aborted");
            0
        }
        GateOutcome::Run(cmd) => exec_foreground(&cmd),
    }
}

/// Print a `[y/N]` prompt to stderr and read one line from stdin.
fn prompt_yes_no(question: &str) -> Option<String> {
    eprint!("{question}  [y/N] ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    match io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => None, // EOF or error → treat as "no"
        Ok(_) => Some(line),
    }
}

/// Spawn the command in the foreground, inheriting our terminal, and return its
/// exit code (a missing binary or signal maps to a non-zero code with a message).
fn exec_foreground(cmd: &LaunchCommand) -> u8 {
    match Command::new(&cmd.program).args(&cmd.args).status() {
        Ok(status) => status.code().map(|c| c as u8).unwrap_or(1),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            eprintln!(
                "rexops: launch failed: binary not found ({})",
                cmd.display()
            );
            1
        }
        Err(e) => {
            eprintln!("rexops: launch failed: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd() -> LaunchCommand {
        LaunchCommand {
            program: "/usr/bin/bulwark".to_owned(),
            args: vec!["tui".to_owned()],
        }
    }

    #[test]
    fn unlaunchable_tool_is_refused_with_exit_1() {
        let out = decide(None, "ghost", false, false, true, Some("y"));
        match out {
            GateOutcome::Refuse { code, msg } => {
                assert_eq!(code, 1);
                assert!(msg.contains("ghost"), "message names the tool");
            }
            other => panic!("expected Refuse, got {other:?}"),
        }
    }

    #[test]
    fn dry_run_previews_and_never_runs_even_with_yes() {
        assert_eq!(
            decide(Some(cmd()), "bulwark", true, true, true, None),
            GateOutcome::DryRun(cmd()),
            "--dry-run wins over --yes and never runs"
        );
    }

    #[test]
    fn yes_runs_without_a_prompt() {
        assert_eq!(
            decide(Some(cmd()), "bulwark", true, false, false, None),
            GateOutcome::Run(cmd()),
            "--yes runs even when not a TTY"
        );
    }

    #[test]
    fn interactive_yes_runs() {
        assert_eq!(
            decide(Some(cmd()), "bulwark", false, false, true, Some("y\n")),
            GateOutcome::Run(cmd())
        );
    }

    #[test]
    fn interactive_no_aborts() {
        assert_eq!(
            decide(Some(cmd()), "bulwark", false, false, true, Some("n\n")),
            GateOutcome::Aborted
        );
        // Empty answer (just Enter) defaults to No.
        assert_eq!(
            decide(Some(cmd()), "bulwark", false, false, true, Some("\n")),
            GateOutcome::Aborted
        );
    }

    #[test]
    fn non_interactive_without_yes_is_refused() {
        match decide(Some(cmd()), "bulwark", false, false, false, None) {
            GateOutcome::Refuse { code, .. } => assert_eq!(code, 1),
            other => panic!("expected Refuse, got {other:?}"),
        }
    }
}
