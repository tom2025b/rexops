//! scripts.rs — Scripts / ScriptVault screen (new 4th screen).
//!
//! Shows the structured ScriptVaultInfo from the snapshot (populated from
//! ScriptVaultAdapter). Lists scripts with favorite markers, etc.
//! Uses the adapter_item widget for rendering rows.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::App;
use crate::theme;
use crate::widgets;

/// Render the Scripts screen.
pub fn render_scripts(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(5),    // list of scripts
        ])
        .split(area);

    render_scripts_header(f, app, chunks[0]);
    render_scripts_list(f, app, chunks[1]);
}

fn render_scripts_header(f: &mut Frame, app: &App, area: Rect) {
    let health = app
        .snapshot
        .adapter_health
        .get("scriptvault")
        .copied()
        .unwrap_or(rexops_core::AdapterHealth::Unknown);
    let badge = widgets::render_health_badge(health);

    let header = Paragraph::new(Line::from(vec![Span::raw("Scripts / Vault "), badge])).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme::border_style()),
    );

    f.render_widget(header, area);
}

fn render_scripts_list(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    if let Some(sv) = &app.snapshot.scriptvault {
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
                );
                lines.push(item);
                // Opportunistic favorite star: only if this script's id/name is in
                // the feed's favorites list. Never a correctness dependency.
                if sv.is_favorite(s) {
                    lines.push(Line::from(Span::styled(
                        "   ★ favorite",
                        theme::health_style(&rexops_core::AdapterHealth::Healthy),
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
            "No scriptvault data yet — press 'r' to probe (or check config for scriptvault.enabled).",
        ));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Tip: Press '1' for Dashboard, '2' for Adapters, '3' for System.",
        theme::help_style(),
    )));

    let list = Paragraph::new(lines).wrap(Wrap { trim: true }).block(
        Block::default()
            .title(" Scripts ")
            .borders(Borders::ALL)
            .border_style(theme::border_style()),
    );

    f.render_widget(list, area);
}

// Learning Notes:
// - New screen added with minimal code by following the established pattern
//   (enum + action + key + dispatch + render fn + mod export).
// - Reuses widgets::render_adapter_item for script rows (even though names
//   are scripts, the widget is generic enough for name + info).
// - Structured data from snapshot.scriptvault makes rendering clean; notes
//   are still there for fallback/CLI.
// - Favorite marker is appended simply; a future widget could handle icons better.
