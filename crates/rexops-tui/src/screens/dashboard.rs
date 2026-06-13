//! screens/dashboard.rs — The main Dashboard screen.
//!
//! This is the primary view shown on startup: adapter health table,
//! risk summary, messages, and status hints.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, Wrap},
    Frame,
};

use rexops_core::AdapterHealth;
use suite_ui::{pane, SearchBar, Theme};

use crate::app::App;
use crate::ui::widgets;

/// Render the full dashboard into the given area.
pub fn render_dashboard(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // search / filter bar
            Constraint::Min(5),    // adapters table
            Constraint::Length(3), // risk
            Constraint::Min(2),    // messages / notes
            Constraint::Min(3),    // events / logs pane
        ])
        .split(area);

    render_search_bar(f, app, chunks[0], theme);
    render_adapters_table(f, app, chunks[1], theme);
    render_risk_summary(f, app, chunks[2], theme);
    render_messages(f, app, chunks[3], theme);
    render_logs(f, app, chunks[4], theme);
}

/// The shared suite-ui search bar, driving the adapters table below it. The
/// match count is the number of adapter rows the current filter keeps, so an
/// empty result is obvious at a glance.
fn render_search_bar(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let count = app.filtered_adapter_names().len();
    SearchBar {
        query: &app.filter,
        placeholder: "/ to filter adapters · esc clears",
        match_count: Some(count),
    }
    .render(f, area, theme);
}

fn render_adapters_table(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let header_cells = ["Adapter", "Health", "Info"]
        .into_iter()
        .map(|h| Cell::from(h).style(Style::default().add_modifier(Modifier::BOLD)));

    let header = Row::new(header_cells).bottom_margin(1);

    let rows: Vec<Row> = if app.snapshot.adapter_health.is_empty() {
        vec![Row::new(vec![
            Cell::from("bulwark (default)"),
            Cell::from("Unavailable")
                .style(theme.health(widgets::health_to_suite(AdapterHealth::Unavailable))),
            Cell::from("not probed yet — press 'r'"),
        ])]
    } else {
        // Restrict to the rows the live filter keeps. `filtered_adapter_names`
        // is the same matcher the Adapters screen uses, so the Dashboard table
        // and that screen always agree on what a query selects.
        let visible = app.filtered_adapter_names();
        // The selected adapter is shared with the Adapters screen; mark its row
        // the same way the Adapters list does — a `▶ ` marker + bold — so j/k
        // navigation is visible here too. `position` defaults to 0 so the first
        // visible row reads as selected when nothing matches by name.
        let sel_name = app.selected_adapter.clone().unwrap_or_default();
        let sel_pos = visible.iter().position(|n| n == &sel_name).unwrap_or(0);
        let mut rows: Vec<Row> = visible
            .iter()
            .enumerate()
            .filter_map(|(i, name)| {
                let health = app.snapshot.adapter_health.get(name)?;
                let health_cell = Cell::from(widgets::render_health_badge(*health, theme));

                let info = if health.is_available() {
                    "healthy / degraded — version in notes if known"
                } else {
                    "binary not found or probe failed"
                };

                let is_selected = i == sel_pos;
                let marker = if is_selected { "▶ " } else { "  " };
                let name_cell = if is_selected {
                    Cell::from(format!("{marker}{name}")).style(Style::default().add_modifier(Modifier::BOLD))
                } else {
                    Cell::from(format!("{marker}{name}"))
                };

                Some(Row::new(vec![name_cell, health_cell, Cell::from(info)]))
            })
            .collect();

        // A non-empty snapshot with an active filter that matches nothing gets a
        // clear empty-state row rather than a blank table.
        if rows.is_empty() {
            rows.push(Row::new(vec![
                Cell::from("(no matches)").style(theme.dim()),
                Cell::from(""),
                Cell::from("clear the filter with esc"),
            ]));
        }
        rows
    };

    let table = Table::new(
        rows,
        [
            Constraint::Length(22), // name + 2-char selection marker
            Constraint::Length(12),
            Constraint::Min(30),
        ],
    )
    .header(header)
    .block(pane("Adapters", theme));

    f.render_widget(table, area);
}

fn render_risk_summary(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let r = &app.snapshot.risk;

    let text = format!(
        "Risk: critical={} high={} medium={} low={} info={}  |  total={}  should_block={}",
        r.critical, r.high, r.medium, r.low, r.info, r.total_findings, r.should_block
    );

    let risk = Paragraph::new(text).block(pane("Risk Summary", theme));

    f.render_widget(risk, area);
}

fn render_messages(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let mut lines: Vec<Line> = Vec::new();

    if app.refreshing {
        lines.push(Line::from(Span::styled(
            "⟳ Refresh in progress — UI remains responsive. Press 'q' to quit anytime.",
            theme.working(),
        )));
    }

    // Help text moved to nice popup overlay (press ?); keep other messages.

    for note in app.snapshot.notes.iter().rev().take(6) {
        lines.push(Line::from(format!("• {note}")));
    }

    if lines.is_empty() {
        lines.push(Line::from("(no messages — press 'r' to probe adapters)"));
    }

    let notes = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .block(pane("Messages / Notes", theme));

    f.render_widget(notes, area);
}

fn render_logs(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let mut lines: Vec<Line> = Vec::new();

    if app.recent_events.is_empty() {
        lines.push(Line::from("(no events yet)"));
    } else {
        for event in app.recent_events.iter() {
            lines.push(widgets::render_log_line(event));
        }
    }

    let logs = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .block(pane("Events / Logs", theme));

    f.render_widget(logs, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rexops_core::{AdapterHealth, AppConfig, OpsSnapshot};
    use std::sync::mpsc;

    /// Render the Dashboard off-screen and flatten it to text, the same
    /// buffer-to-string approach the launchpad/jobs screen tests use.
    fn render_to_text(app: &App) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test backend");
        let theme = Theme::with_color(true);
        terminal
            .draw(|f| render_dashboard(f, app, f.area(), theme))
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

    /// An App carrying the given adapters (all Healthy), with `selected` chosen.
    fn app_with_adapters(names: &[&str], selected: &str) -> App {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default(), None);
        let mut snap = OpsSnapshot::new();
        for name in names {
            snap.adapter_health
                .insert((*name).to_owned(), AdapterHealth::Healthy);
        }
        app.apply_snapshot(snap);
        app.selected_adapter = Some(selected.to_owned());
        app
    }

    #[test]
    fn the_selected_adapter_row_carries_the_marker() {
        // The Dashboard table must show which adapter is selected (the same
        // selection the Adapters screen drives), so j/k navigation is visible
        // here. The selected row leads with `▶`; the others do not.
        let app = app_with_adapters(&["alpha", "bravo", "charlie"], "bravo");
        let text = render_to_text(&app);
        assert!(
            text.contains("▶ bravo"),
            "the selected adapter row must carry the marker:\n{text}"
        );
        // A non-selected adapter must NOT carry the marker.
        assert!(
            !text.contains("▶ alpha"),
            "only the selected row is marked:\n{text}"
        );
    }

    #[test]
    fn the_marker_follows_a_changed_selection() {
        // Moving the selection moves the marker — proving the table reads live
        // selection state, not a fixed first row.
        let app = app_with_adapters(&["alpha", "bravo", "charlie"], "charlie");
        let text = render_to_text(&app);
        assert!(text.contains("▶ charlie"), "marker on charlie:\n{text}");
        assert!(!text.contains("▶ bravo"), "bravo not marked:\n{text}");
    }
}
