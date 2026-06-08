//! screens/adapters.rs — Adapters screen with keyboard-selectable list and detail preview.
//!
//! Uses the adapter_names + selected_adapter (name) + filter from App (populated from snapshot).
//! Shows a left list (highlighted selection) + right detail pane for the selected adapter.
//!
//! Navigation (j/k, arrows, enter) is handled in App::on_action; this just renders state.
//! Selection wraps; detail shows health and recent notes mentioning the adapter.
//!
//! Uses manual highlight with a marker to keep render state simple.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::App;
use crate::theme;
use crate::widgets;

use rexops_core::AdapterHealth;

/// Render the Adapters screen into the given area.
pub fn render_adapters(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    render_adapter_list(f, app, chunks[0]);
    render_adapter_detail(f, app, chunks[1]);
}

fn render_adapter_list(f: &mut Frame, app: &App, area: Rect) {
    let visible = app.filtered_adapter_names();
    let mut lines: Vec<Line> = Vec::new();

    let filter_suffix = if app.filter.is_empty() {
        String::new()
    } else {
        format!(" [filter: {}]", app.filter)
    };

    if visible.is_empty() {
        lines.push(Line::from(
            "(no matching adapters — backspace/esc to clear or press 'r')",
        ));
    } else {
        let sel_name = app.selected_adapter.clone().unwrap_or_default();
        let sel_pos = visible.iter().position(|n| n == &sel_name).unwrap_or(0);
        for (i, name) in visible.iter().enumerate() {
            let is_selected = i == sel_pos;
            let health = app
                .snapshot
                .adapter_health
                .get(name)
                .copied()
                .unwrap_or(AdapterHealth::Unknown);
            let info = if health.is_available() {
                "healthy / degraded — version in notes if known"
            } else {
                "binary not found or probe failed"
            };
            let item = widgets::render_adapter_item(name, health, info, is_selected);
            lines.push(item);
        }
    }

    let title = format!(
        " Adapters{} (j/k/arrows, enter, chars to filter, esc/backspace) ",
        filter_suffix
    );
    let list = Paragraph::new(lines).wrap(Wrap { trim: true }).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(theme::border_style()),
    );

    f.render_widget(list, area);
}

fn render_adapter_detail(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    let visible = app.filtered_adapter_names();
    let sel_name = app.selected_adapter.clone().unwrap_or_default();
    let sel_pos = visible.iter().position(|n| n == &sel_name).unwrap_or(0);

    if let Some(name) = visible.get(sel_pos) {
        lines.push(Line::from(Span::styled(
            format!("Detail for: {name}"),
            theme::title_style(),
        )));

        if let Some(health) = app.snapshot.adapter_health.get(name) {
            let style = theme::health_style(health);
            lines.push(Line::from(vec![
                Span::raw("Health: "),
                Span::styled(format!("{:?}", health), style),
            ]));
        }

        // Show notes that mention this adapter.
        let related: Vec<&String> = app
            .snapshot
            .notes
            .iter()
            .filter(|n| {
                n.to_lowercase().contains(&name.to_lowercase())
                    || n.contains("system") && name == "system"
            })
            .collect();

        if !related.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from("Related notes:"));
            for n in related.iter().take(5) {
                lines.push(Line::from(format!("• {}", n)));
            }
        } else {
            lines.push(Line::from(""));
            lines.push(Line::from(
                "(no specific notes for this adapter; press 'r' or activate to surface)",
            ));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Tip: 'enter' surfaces selection. Press ?/h for full help popup.",
            theme::help_style(),
        )));
    } else {
        lines.push(Line::from("No adapter selected."));
    }

    let detail = Paragraph::new(lines).wrap(Wrap { trim: true }).block(
        Block::default()
            .title(" Preview / Detail ")
            .borders(Borders::ALL)
            .border_style(theme::border_style()),
    );

    f.render_widget(detail, area);
}
