//! system.rs — System info screen.
//!
//! Shows health and details for the "system" adapter (from SystemAdapter).
//! Uses structured system data from the snapshot.
//!
//! Simple render: health badge + list of system facts.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use suite_ui::{pane, Theme};

use crate::app::App;
use crate::widgets;

/// Render the System screen.
pub fn render_system(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header for system
            Constraint::Min(5),    // details
        ])
        .split(area);

    render_system_header(f, app, chunks[0], theme);
    render_system_details(f, app, chunks[1], theme);
}

fn render_system_header(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let health = app
        .snapshot
        .adapter_health
        .get("system")
        .copied()
        .unwrap_or(rexops_core::AdapterHealth::Unknown);
    let badge = widgets::render_health_badge(health, theme);

    let header = Paragraph::new(Line::from(vec![Span::raw("System Info "), badge])).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.dim()),
    );

    f.render_widget(header, area);
}

fn render_system_details(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let mut lines: Vec<Line> = Vec::new();

    if let Some(sys) = &app.snapshot.system {
        if let Some(h) = &sys.hostname {
            lines.push(Line::from(format!("hostname: {h}")));
        }
        if let Some(k) = &sys.kernel {
            lines.push(Line::from(format!("kernel: {k}")));
        }
        if let Some(u) = &sys.uptime {
            lines.push(Line::from(format!("uptime: {u}")));
        }
        if !sys.disk.is_empty() {
            lines.push(Line::from("disk:"));
            for d in sys.disk.iter().take(4) {
                lines.push(Line::from(format!("  {d}")));
            }
        }
    } else {
        // Fallback to notes parsing (for older snapshots or if not populated).
        let system_notes: Vec<&String> = app
            .snapshot
            .notes
            .iter()
            .filter(|n| n.starts_with("system "))
            .collect();

        if system_notes.is_empty() {
            lines.push(Line::from(
                "No system details yet — press 'r' to probe (or check config).",
            ));
            lines.push(Line::from(""));
            lines.push(Line::from(
                "SystemAdapter provides: hostname, kernel, uptime, disk usage.",
            ));
        } else {
            for note in system_notes {
                let clean = note.strip_prefix("system ").unwrap_or(note);
                lines.push(Line::from(format!("• {}", clean)));
            }
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Tip: Press '1' for Dashboard, '2' for Adapters. 'r' to refresh.",
        theme.dim(),
    )));

    let details = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .block(pane("Details", theme));

    f.render_widget(details, area);
}
