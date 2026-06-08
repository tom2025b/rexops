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
use crate::health;
use crate::widgets;

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
        placeholder: "type to filter adapters · esc clears",
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
                .style(theme.health(health::to_suite(AdapterHealth::Unavailable))),
            Cell::from("not probed yet — press 'r'"),
        ])]
    } else {
        // Restrict to the rows the live filter keeps. `filtered_adapter_names`
        // is the same matcher the Adapters screen uses, so the Dashboard table
        // and that screen always agree on what a query selects.
        let visible = app.filtered_adapter_names();
        let mut rows: Vec<Row> = visible
            .iter()
            .filter_map(|name| {
                let health = app.snapshot.adapter_health.get(name)?;
                let health_cell = Cell::from(widgets::render_health_badge(*health, theme));

                let info = if health.is_available() {
                    "healthy / degraded — version in notes if known"
                } else {
                    "binary not found or probe failed"
                };

                Some(Row::new(vec![
                    Cell::from(name.clone()),
                    health_cell,
                    Cell::from(info),
                ]))
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
            Constraint::Length(20),
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

    // Help text moved to nice popup overlay (press ?/h); keep other messages.

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
