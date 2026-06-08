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
    widgets::{Paragraph, Wrap},
    Frame,
};

use suite_ui::{pane, Theme};

use crate::app::{self, App};
use crate::launcher;
use crate::widgets;

/// Width the tool name is padded to so the badges and tags line up into columns.
/// The catalog names are short ("Workstate" is the longest at 9), so 10 leaves a
/// single space of gutter before the badge.
const NAME_COL: usize = 10;

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
pub fn render_launcher(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(5),    // tool list
            Constraint::Length(4), // selected-tool detail
        ])
        .split(area);

    render_launcher_header(f, chunks[0], theme);
    render_launcher_list(f, app, chunks[1], theme);
    render_launcher_detail(f, app, chunks[2], theme);
}

fn render_launcher_header(f: &mut Frame, area: Rect, theme: Theme) {
    let line = Line::from(Span::styled(
        "Pick a tool with ↑/↓, then Enter to launch (you'll be asked to confirm).",
        theme.dim(),
    ));
    let header = Paragraph::new(line).block(pane("Launcher", theme));
    f.render_widget(header, area);
}

/// Render a single launcher row: an accent selection rail + row tint on the
/// selected row, the name padded into a column, the health badge, and a dim
/// run-mode / install-state tag so the user can see at a glance what Enter does.
fn render_launcher_row(app: &App, index: usize, tool: &ToolEntry, theme: Theme) -> Line<'static> {
    let selected = index == app.selected_tool;

    // The suite's selection look: an accent rail glyph on the selected row, a
    // plain gutter otherwise. The rail keeps its accent because `selection()`
    // sets no foreground (see Theme::selection docs).
    let rail = if selected {
        Span::styled("▌ ", theme.selected_rail())
    } else {
        Span::raw("  ")
    };

    let health = app
        .snapshot
        .adapter_health
        .get(tool.id)
        .copied()
        .unwrap_or(rexops_core::AdapterHealth::Unknown);
    let badge = widgets::render_health_badge(health, theme);

    // Run-mode + install-state tag. `resolve_command` is read-only (no spawn).
    let tag = if launcher::resolve_command(tool.id, &app.config).is_none() {
        "· not installed".to_string()
    } else if app::is_streamable(tool.id) {
        "· streams".to_string()
    } else {
        "· interactive".to_string()
    };

    let name = format!("{:<width$}", tool.name, width = NAME_COL);
    let name_span = if selected {
        Span::styled(name, theme.selection())
    } else {
        Span::styled(name, theme.title())
    };

    Line::from(vec![
        rail,
        name_span,
        Span::raw(" "),
        badge,
        Span::raw("  "),
        Span::styled(tag, theme.dim()),
    ])
}

fn render_launcher_list(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let lines: Vec<Line> = CATALOG
        .iter()
        .enumerate()
        .map(|(i, tool)| render_launcher_row(app, i, tool, theme))
        .collect();

    let list = Paragraph::new(lines).block(pane("Tools", theme));
    f.render_widget(list, area);
}

/// The detail pane: the full description of the currently selected tool (so a
/// long one is never clipped in its row), plus the standing note that
/// Workstate-sourced sections aren't launchable.
fn render_launcher_detail(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let mut lines: Vec<Line> = Vec::new();

    if let Some(tool) = CATALOG.get(app.selected_tool) {
        lines.push(Line::from(vec![
            Span::styled(format!("{}: ", tool.name), theme.title()),
            Span::raw(tool.description.to_string()),
        ]));
    }
    lines.push(Line::from(Span::styled(
        "Workstate-sourced sections report 'no launch command yet'.",
        theme.dim(),
    )));

    let detail = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .block(pane("Detail", theme));
    f.render_widget(detail, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rexops_core::AppConfig;
    use std::sync::mpsc;

    /// Render the Launcher into an off-screen buffer and flatten it to text, so a
    /// test can assert on what actually appears (glyphs + tags). Mirrors the
    /// suite-ui gallery's buffer-to-string approach.
    fn render_to_text(app: &App) -> String {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).expect("test backend");
        let theme = Theme::with_color(true);
        terminal
            .draw(|f| render_launcher(f, app, f.area(), theme))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let width = buffer.area.width as usize;
        let mut out = String::new();
        for (i, cell) in buffer.content.iter().enumerate() {
            if i % width == 0 && i != 0 {
                out.push('\n');
            }
            out.push_str(cell.symbol());
        }
        out
    }

    /// A fresh App with the given selected tool index (no probed snapshot, so
    /// every tool reads as Unknown health — fine for layout/tag assertions).
    fn app_with_selection(selected: usize) -> App {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default());
        app.selected_tool = selected;
        app
    }

    #[test]
    fn selected_row_shows_the_accent_rail_glyph() {
        let app = app_with_selection(0);
        let text = render_to_text(&app);
        // The first catalog row is selected → the rail glyph precedes its name.
        assert!(text.contains("▌ Bulwark"), "selected row must show the rail:\n{text}");
        // A non-selected row has no rail before its name.
        assert!(!text.contains("▌ Proto"), "non-selected rows have no rail:\n{text}");
    }

    #[test]
    fn rows_carry_a_run_mode_or_install_tag() {
        // With a default config and no `which` hits in the test environment,
        // tools resolve to no command → "not installed". The tag column is what
        // we assert is present (the polish that tells the user what Enter does).
        let app = app_with_selection(0);
        let text = render_to_text(&app);
        assert!(
            text.contains("not installed")
                || text.contains("interactive")
                || text.contains("streams"),
            "every row should carry a run-mode/install tag:\n{text}"
        );
    }

    #[test]
    fn detail_pane_echoes_the_selected_tools_description() {
        // Select "Proto" (index 1) and confirm its full description shows in the
        // detail pane, not just the row.
        let app = app_with_selection(1);
        let text = render_to_text(&app);
        assert!(text.contains("Proto:"), "detail names the selected tool:\n{text}");
        assert!(
            text.contains("protocol") || text.contains("checklist"),
            "detail shows the selected tool's description:\n{text}"
        );
    }

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
