//! launchpad.rs — The Launcher screen (6th screen).
//!
//! Lists the available specialist tools with a short description and lets the
//! user pick one (↑/↓) and launch it (Enter). Launch orchestration itself lives
//! in `crate::launcher` (the module is deliberately named differently from this
//! screen to avoid confusion: `screens::launchpad` renders, `crate::launcher`
//! resolves+spawns).
//!
//! The catalog is a small static list. Not every entry is launchable; sections
//! sourced from Workstate have no executable, which `launcher::launch_tool`
//! handles by reporting "no launch command yet" rather than erroring.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::App;
use crate::theme;
use crate::widgets;

/// One entry in the launcher catalog: the adapter/tool id (keys `which` and the
/// config binary), the display name, and a one-line description.
pub struct ToolEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

/// The static catalog of launchable tools shown on the Launcher screen.
///
/// Single source of truth: both the renderer here and the navigation/launch
/// logic in `app.rs` index into this slice, so the list can never drift between
/// what is shown and what Enter acts on.
pub const CATALOG: &[ToolEntry] = &[
    ToolEntry {
        id: "bulwark",
        name: "Bulwark",
        description: "Content/security inspection (live scan)",
    },
    ToolEntry {
        id: "proto",
        name: "Proto",
        description: "Guided protocol / checklist runner (interactive)",
    },
    ToolEntry {
        id: "scripts",
        name: "Scripts",
        description: "Script inventory from Workstate",
    },
    ToolEntry {
        id: "tools",
        name: "Tools",
        description: "Tool ownership & lifecycle from Workstate",
    },
    ToolEntry {
        id: "workstate",
        name: "Workstate",
        description: "Snapshot source of truth",
    },
];

/// Render the Launcher screen.
pub fn render_launcher(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(5),    // tool list
        ])
        .split(area);

    render_launcher_header(f, chunks[0]);
    render_launcher_list(f, app, chunks[1]);
}

fn render_launcher_header(f: &mut Frame, area: Rect) {
    let header = Paragraph::new(Line::from(Span::raw(
        "Launcher — pick a tool, Enter to launch",
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme::border_style()),
    );
    f.render_widget(header, area);
}

fn render_launcher_list(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    for (i, tool) in CATALOG.iter().enumerate() {
        // Badge reflects probe health for this tool (Unknown until probed).
        let health = app
            .snapshot
            .adapter_health
            .get(tool.id)
            .copied()
            .unwrap_or(rexops_core::AdapterHealth::Unknown);
        let item = widgets::render_adapter_item(
            tool.name,
            health,
            tool.description,
            i == app.selected_tool,
        );
        lines.push(item);
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↑/↓ select  •  Enter launch (asks to confirm)  •  Esc back to Dashboard",
        theme::help_style(),
    )));
    lines.push(Line::from(Span::styled(
        "Workstate-sourced sections report 'no launch command yet'.",
        theme::help_style(),
    )));

    let list = Paragraph::new(lines).wrap(Wrap { trim: true }).block(
        Block::default()
            .title(" Tools ")
            .borders(Borders::ALL)
            .border_style(theme::border_style()),
    );

    f.render_widget(list, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_includes_proto_as_launchable() {
        // Proto is a real PATH binary (installed via `cargo install --path .`),
        // so RexOps's `which proto` resolves it. It must be in the catalog to be
        // offered on the Launcher screen.
        let proto = CATALOG.iter().find(|t| t.id == "proto");
        let proto = proto.expect("Proto must be registered in the launcher catalog");
        assert_eq!(proto.name, "Proto");
        assert!(!proto.description.is_empty());
    }

    #[test]
    fn catalog_ids_are_unique() {
        // Ids key both the `which` lookup and the config-binary fallback, so a
        // duplicate id would make two rows resolve to the same command.
        let mut ids: Vec<&str> = CATALOG.iter().map(|t| t.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "catalog tool ids must be unique");
    }
}

// Learning Notes:
// - New screen via the established path: enum variant + action + key + ui
//   dispatch + render fn + mod export. Nothing exotic.
// - CATALOG is `pub const` so app.rs shares the exact same list for navigation
//   and launching — the renderer and the behavior can never disagree.
// - We reuse widgets::render_adapter_item (name + health badge + info + selected
//   highlight) so the Launcher looks identical in style to the other list
//   screens for free.
