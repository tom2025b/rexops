//! scripts.rs — Scripts screen (4th screen).
//!
//! Shows the structured scripts section from the Workstate snapshot.
//! Lists scripts with favorite markers. Uses the adapter_item widget for rows.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

use suite_ui::{pane, pane_blank, Theme};

use crate::app::App;
use crate::ui::widgets;

/// Render the Scripts screen.
pub fn render_scripts(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(5),    // list of scripts
        ])
        .split(area);

    render_scripts_header(f, app, chunks[0], theme);
    render_scripts_list(f, app, chunks[1], theme);
}

fn render_scripts_header(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let health = app
        .snapshot
        .adapter_health
        .get("scripts")
        .copied()
        .unwrap_or(rexops_core::AdapterHealth::Unknown);
    let badge = widgets::render_health_badge(health, theme);

    let header = Paragraph::new(Line::from(vec![Span::raw("Scripts / Vault "), badge]))
        .block(pane_blank(theme));

    f.render_widget(header, area);
}

fn render_scripts_list(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let mut lines: Vec<Line> = Vec::new();

    if let Some(sv) = &app.snapshot.scripts {
        if sv.scripts.is_empty() {
            lines.push(Line::from("No scripts found."));
        } else {
            for s in &sv.scripts {
                // Use the adapter_item widget for consistent rendering.
                // info field: description or empty.
                let info = s.description.as_deref().unwrap_or("");
                let item = widgets::render_adapter_item(
                    s.label(),
                    rexops_core::AdapterHealth::Healthy,
                    info,
                    false,
                    theme,
                );
                lines.push(item);
                // Opportunistic favorite star: only if this script's id/name is in
                // the feed's favorites list. Never a correctness dependency.
                if sv.is_favorite(s) {
                    lines.push(Line::from(Span::styled(
                        "   ★ favorite",
                        theme.live_marker(),
                    )));
                }
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "Total: {} scripts, {} favorites, {} recents",
            sv.total(),
            sv.favorites_count(),
            sv.recents_count()
        )));
    } else {
        lines.push(Line::from(
            "No scripts data yet — press 'r' to load Workstate.",
        ));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Tip: Press '1' for Dashboard, '2' for Adapters, '3' for System.",
        theme.dim(),
    )));

    let list = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .block(pane("Scripts", theme));

    f.render_widget(list, area);
}
