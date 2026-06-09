//! palette.rs — the command palette's command set and filtering.
//!
//! The palette is the unified command surface: navigation (screen switches),
//! actions (refresh / help), and a `run <tool>` per launchable catalog tool that
//! arms the Job Runner. This module owns *what the commands are* and *how a query
//! filters them*. Opening/closing, selection movement, and dispatch live in
//! `app.rs`; the chrome is drawn by `suite_ui::PaletteFrame` in `ui.rs`.
//!
//! `run <tool>` commands carry the tool id + name and, when chosen, go through
//! the same arm → confirm (dry-run) → run gate as the Launcher screen — the
//! palette never spawns anything directly.

use crate::action::Action;
use crate::screens::launchpad::CATALOG;

/// One command the palette can dispatch. Kept a small enum (not boxed closures):
/// the set is fixed and each variant maps to an existing `Action` or to arming a
/// job, so dispatch stays a plain `match` in `app.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Run a high-level action that already exists (refresh, toggle help, or a
    /// screen switch). The carried `Action` is dispatched as if keyed directly.
    Action(Action),
    /// Run a suite tool by catalog id (shown as `name`). Arms the Job Runner via
    /// the confirm gate; never spawns directly from the palette.
    RunTool { id: String, name: String },
}

/// One selectable palette row: a short label, a one-line description, and the
/// command it dispatches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteCommand {
    pub label: String,
    pub desc: String,
    pub command: Command,
}

/// The full command catalog, in display order: navigation, then actions, then a
/// `run <tool>` per catalog tool. Built fresh each open so it always reflects the
/// current tool catalog.
pub fn commands() -> Vec<PaletteCommand> {
    let mut cmds = vec![
        nav(
            "dashboard",
            "go to the Dashboard",
            Action::SwitchToDashboard,
        ),
        nav("adapters", "go to Adapters", Action::SwitchToAdapters),
        nav("system", "go to System info", Action::SwitchToSystem),
        nav("scripts", "go to Scripts", Action::SwitchToScripts),
        nav("tools", "go to Tools / inventory", Action::SwitchToTools),
        nav("launcher", "go to the Launcher", Action::SwitchToLauncher),
        nav("jobs", "go to Jobs (live output)", Action::SwitchToJobs),
        action(
            "refresh",
            "re-probe adapters in the background",
            Action::Refresh,
        ),
        action("help", "toggle the keybinding help", Action::ToggleHelp),
    ];

    // A `run <tool>` per catalog entry. Dispatch arms the confirm gate; whether
    // the tool streams in a Job or hands over the terminal is decided there, so
    // the palette stays uniform.
    for tool in CATALOG {
        cmds.push(PaletteCommand {
            label: format!("run {}", tool.id),
            desc: format!("run {} — {}", tool.name, tool.description),
            command: Command::RunTool {
                id: tool.id.to_owned(),
                name: tool.name.to_owned(),
            },
        });
    }
    cmds
}

/// Filter `commands()` by a case-insensitive substring match on label OR
/// description. An empty query returns everything (in catalog order).
pub fn filter(query: &str) -> Vec<PaletteCommand> {
    let all = commands();
    if query.is_empty() {
        return all;
    }
    let q = query.to_lowercase();
    all.into_iter()
        .filter(|c| c.label.to_lowercase().contains(&q) || c.desc.to_lowercase().contains(&q))
        .collect()
}

fn nav(label: &str, desc: &str, action: Action) -> PaletteCommand {
    PaletteCommand {
        label: label.to_owned(),
        desc: desc.to_owned(),
        command: Command::Action(action),
    }
}

fn action(label: &str, desc: &str, action: Action) -> PaletteCommand {
    PaletteCommand {
        label: label.to_owned(),
        desc: desc.to_owned(),
        command: Command::Action(action),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_includes_nav_actions_and_one_run_per_tool() {
        let cmds = commands();
        // Every catalog tool yields exactly one `run <id>` command.
        for tool in CATALOG {
            let label = format!("run {}", tool.id);
            assert_eq!(
                cmds.iter().filter(|c| c.label == label).count(),
                1,
                "expected exactly one palette command for {label}"
            );
        }
        // A couple of navigation/action commands are present.
        assert!(cmds.iter().any(|c| c.label == "refresh"));
        assert!(cmds.iter().any(|c| c.label == "jobs"));
    }

    #[test]
    fn filter_matches_label_or_description_case_insensitively() {
        // "bul" matches the bulwark run command by label.
        let by_label = filter("bul");
        assert!(by_label.iter().any(|c| c.label == "run bulwark"));

        // "probe" matches refresh by its DESCRIPTION, not its label.
        let by_desc = filter("probe");
        assert!(
            by_desc.iter().any(|c| c.label == "refresh"),
            "description match should surface 'refresh'"
        );

        // Case-insensitive.
        assert_eq!(filter("REFRESH").len(), filter("refresh").len());
    }

    #[test]
    fn empty_query_returns_everything() {
        assert_eq!(filter("").len(), commands().len());
    }

    #[test]
    fn no_match_returns_empty() {
        assert!(filter("zzz-no-such-command").is_empty());
    }
}
