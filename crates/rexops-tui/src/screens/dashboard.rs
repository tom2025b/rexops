//! screens/dashboard.rs — The main Dashboard screen.
//!
//! This is the primary view shown on startup: adapter health table,
//! risk summary, messages, and status hints.
//!
//! It is deliberately self-contained so that later we can have other
//! screens (e.g. a focused Adapters list or a Tools inventory) that
//! reuse the same App state and theme.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
    Frame,
};

use rexops_core::AdapterHealth;

use crate::app::App;
use crate::theme;
use crate::widgets;

/// Render the full dashboard into the given area.
pub fn render_dashboard(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),    // adapters table
            Constraint::Length(3), // risk
            Constraint::Min(2),    // messages / notes
            Constraint::Min(3),    // events / logs pane
        ])
        .split(area);

    render_adapters_table(f, app, chunks[0]);
    render_risk_summary(f, app, chunks[1]);
    render_messages(f, app, chunks[2]);
    render_logs(f, app, chunks[3]);
}

fn render_adapters_table(f: &mut Frame, app: &App, area: Rect) {
    let header_cells = ["Adapter", "Health", "Info"]
        .into_iter()
        .map(|h| Cell::from(h).style(Style::default().add_modifier(Modifier::BOLD)));

    let header = Row::new(header_cells).bottom_margin(1);

    let rows: Vec<Row> = if app.snapshot.adapter_health.is_empty() {
        vec![Row::new(vec![
            Cell::from("bulwark (default)"),
            Cell::from("Unavailable").style(theme::health_style(&AdapterHealth::Unavailable)),
            Cell::from("not probed yet — press 'r'"),
        ])]
    } else {
        app.snapshot
            .adapter_health
            .iter()
            .map(|(name, health)| {
                let health_cell = Cell::from(widgets::render_health_badge(*health));

                let info = if health.is_available() {
                    "healthy / degraded — version in notes if known"
                } else {
                    "binary not found or probe failed"
                };

                Row::new(vec![
                    Cell::from(name.clone()),
                    health_cell,
                    Cell::from(info),
                ])
            })
            .collect()
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
    .block(
        Block::default()
            .title(" Adapters ")
            .borders(Borders::ALL)
            .border_style(theme::border_style()),
    );

    f.render_widget(table, area);
}

fn render_risk_summary(f: &mut Frame, app: &App, area: Rect) {
    let r = &app.snapshot.risk;

    let text = format!(
        "Risk: critical={} high={} medium={} low={} info={}  |  total={}  should_block={}",
        r.critical, r.high, r.medium, r.low, r.info, r.total_findings, r.should_block
    );

    let risk = Paragraph::new(text).block(
        Block::default()
            .title(" Risk Summary ")
            .borders(Borders::ALL)
            .border_style(theme::border_style()),
    );

    f.render_widget(risk, area);
}

fn render_messages(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    if app.refreshing {
        lines.push(Line::from(Span::styled(
            "⟳ Refresh in progress — UI remains responsive. Press 'q' to quit anytime.",
            theme::working_style(),
        )));
    }

    // Help text moved to nice popup overlay (press ?/h); keep other messages.

    for note in app.snapshot.notes.iter().rev().take(6) {
        lines.push(Line::from(format!("• {note}")));
    }

    if lines.is_empty() {
        lines.push(Line::from("(no messages — press 'r' to probe adapters)"));
    }

    let notes = Paragraph::new(lines).wrap(Wrap { trim: true }).block(
        Block::default()
            .title(" Messages / Notes ")
            .borders(Borders::ALL)
            .border_style(theme::border_style()),
    );

    f.render_widget(notes, area);
}

fn render_logs(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    if app.recent_events.is_empty() {
        lines.push(Line::from("(no events yet)"));
    } else {
        for event in app.recent_events.iter() {
            lines.push(widgets::render_log_line(event));
        }
    }

    let logs = Paragraph::new(lines).wrap(Wrap { trim: true }).block(
        Block::default()
            .title(" Events / Logs ")
            .borders(Borders::ALL)
            .border_style(theme::border_style()),
    );

    f.render_widget(logs, area);
}

// Learning Notes:
// - By moving the dashboard into its own file under screens/ we follow the
//   plan's recommended layout.
// - The render functions are still pure (take &App, write to Frame).
// - We delegate all style decisions to theme:: so this file doesn't contain
//   Color::Green etc.
// - For a real multi-screen app we would have a trait `Screen { fn render(...) }`
//   or an enum dispatch in a top-level render. For now a simple function is
//   the simplest thing that works.
