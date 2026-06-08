//! tools.rs — Tools screen (5th screen, key '5').
//!
//! Shows the structured ToolsInfo from the Workstate snapshot. Lists
//! each tool with owner, lifecycle state, and health-check tally, badging
//! by the per-tool `status` (ok / attention).
//!
//! Reuses the adapter_item widget + health badge for visual consistency with
//! Adapters and Scripts screens. No selection/filter on this screen yet.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use suite_ui::{pane, Theme};

use crate::app::App;
use crate::widgets;

/// Render the Tools screen.
pub fn render_tools(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header with health badge
            Constraint::Min(5),    // list of tools + details
        ])
        .split(area);

    render_tools_header(f, app, chunks[0], theme);
    render_tools_list(f, app, chunks[1], theme);
}

fn render_tools_header(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    // Look up the section health recorded for tools (set during build_snapshot).
    // Falls back to Unknown if not present (graceful degradation, per error handling doc).
    let health = app
        .snapshot
        .adapter_health
        .get("tools")
        .copied()
        .unwrap_or(rexops_core::AdapterHealth::Unknown);
    let badge = widgets::render_health_badge(health, theme);

    // Header shows the conceptual name + live badge (same pattern as scripts/system headers).
    let header = Paragraph::new(Line::from(vec![Span::raw("Tools / Inventory "), badge])).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.dim()),
    );

    f.render_widget(header, area);
}

/// Map the per-tool aggregate `status` ("ok" / "attention") into
/// our AdapterHealth for badge rendering in the list. "attention" → Degraded so
/// it stands out visually; "ok" → Healthy.
fn tool_status_to_adapter_health(status: &str) -> rexops_core::AdapterHealth {
    match status.to_lowercase().as_str() {
        "ok" => rexops_core::AdapterHealth::Healthy,
        "attention" => rexops_core::AdapterHealth::Degraded,
        _ => rexops_core::AdapterHealth::Unknown,
    }
}

fn render_tools_list(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let mut lines: Vec<Line> = Vec::new();

    if let Some(tf) = &app.snapshot.tools {
        if tf.tools.is_empty() {
            lines.push(Line::from("No tools found."));
        } else {
            for t in &tf.tools {
                // Compact info string: owner, lifecycle, and health-check tally
                // from the Workstate section.
                let info = format!(
                    "owner: {}  lifecycle: {}{}  health: {}/{}{}",
                    t.owner,
                    t.lifecycle_state,
                    if t.review_due_flag {
                        match t.review_after.as_deref() {
                            Some(date) => format!("  review due since {date}"),
                            None => "  review due".to_string(),
                        }
                    } else {
                        String::new()
                    },
                    t.health_passed,
                    t.health_total,
                    if t.drifted { "  (drifted)" } else { "" }
                );

                // Main row: display_name + badge (from status) + info.
                let item_health = tool_status_to_adapter_health(&t.status);
                let item =
                    widgets::render_adapter_item(&t.display_name, item_health, &info, false, theme);
                lines.push(item);
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "Total: {} tools, {} need attention (as of {})",
            tf.tool_count, tf.attention_count, tf.as_of
        )));
    } else {
        lines.push(Line::from(
            "No tools data yet — press 'r' to load Workstate.",
        ));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Tip: Press '1' for Dashboard, '2' for Adapters, '3' for System, '4' for Scripts.",
        theme.dim(),
    )));

    // The block title and border match the style of other list screens.
    let list = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .block(pane("Tools", theme));

    f.render_widget(list, area);
}
