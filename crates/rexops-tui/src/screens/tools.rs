//! tools.rs — Tools / ToolFoundry screen (5th screen, key '5').
//!
//! Shows the structured ToolFoundryInfo from the snapshot (populated from
//! ToolFoundryAdapter). Lists tools with owner, per-tool health, and symlink
//! status. This demonstrates ToolFoundry's focus (ownership / lifecycle /
//! health / symlinks) in the TUI.
//!
//! Reuses the adapter_item widget + health badge for visual consistency with
//! Adapters and Scripts screens. No selection/filter on this screen yet
//! (kept deliberately simple, like Scripts).

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::App;
use crate::theme;
use crate::widgets;

/// Render the Tools screen.
pub fn render_tools(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header with health badge
            Constraint::Min(5),    // list of tools + details
        ])
        .split(area);

    render_tools_header(f, app, chunks[0]);
    render_tools_list(f, app, chunks[1]);
}

fn render_tools_header(f: &mut Frame, app: &App, area: Rect) {
    // Look up the adapter health recorded for "toolfoundry" (set during build_snapshot).
    // Falls back to Unknown if not present (graceful degradation, per error handling doc).
    let health = app
        .snapshot
        .adapter_health
        .get("toolfoundry")
        .copied()
        .unwrap_or(rexops_core::AdapterHealth::Unknown);
    let badge = widgets::render_health_badge(health);

    // Header shows the conceptual name + live badge (same pattern as scripts/system headers).
    let header = Paragraph::new(Line::from(vec![Span::raw("Tools / Inventory "), badge])).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme::border_style()),
    );

    f.render_widget(header, area);
}

/// Map the free-string health from ToolFoundry sample ("healthy", "degraded")
/// into our AdapterHealth for badge rendering in the list. Keeps the demo
/// visually interesting without adding new types or complex logic.
fn tool_health_to_adapter_health(h: &str) -> rexops_core::AdapterHealth {
    match h.to_lowercase().as_str() {
        "healthy" => rexops_core::AdapterHealth::Healthy,
        "degraded" => rexops_core::AdapterHealth::Degraded,
        "unavailable" => rexops_core::AdapterHealth::Unavailable,
        _ => rexops_core::AdapterHealth::Unknown,
    }
}

fn render_tools_list(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    if let Some(tf) = &app.snapshot.toolfoundry {
        if tf.tools.is_empty() {
            lines.push(Line::from("No tools found."));
        } else {
            for t in &tf.tools {
                // Build a compact info string for the widget (owner + symlink).
                // The per-tool health goes into its own badge via the helper above.
                let sl = t.symlink.as_deref().unwrap_or("-");
                let info = format!("owner: {}  symlink: {}", t.owner, sl);

                // Use the widget for the main row (name + badge + info). We pass
                // a mapped health so the "old-tool" shows degraded visually.
                let item_health = tool_health_to_adapter_health(&t.health);
                let item = widgets::render_adapter_item(&t.name, item_health, &info, false);
                lines.push(item);

                // Append a small detail line for the raw tool health string (educational;
                // shows that ToolFoundry can report its own notion of health per tool).
                if !t.health.is_empty() {
                    lines.push(Line::from(Span::styled(
                        format!("   tool health: {}", t.health),
                        theme::help_style(),
                    )));
                }
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "Total: {} tools (from ToolFoundry stub)",
            tf.total
        )));
    } else {
        // Excellent degraded state: clear message + hint what to do.
        lines.push(Line::from(
            "No toolfoundry data yet — press 'r' to probe (or check config for toolfoundry.enabled).",
        ));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Tip: Press '1' for Dashboard, '2' for Adapters, '3' for System, '4' for Scripts.",
        theme::help_style(),
    )));

    // The block title and border match the style of other list screens.
    let list = Paragraph::new(lines).wrap(Wrap { trim: true }).block(
        Block::default()
            .title(" Tools ")
            .borders(Borders::ALL)
            .border_style(theme::border_style()),
    );

    f.render_widget(list, area);
}

// Learning Notes:
// - Exact mirror of scripts.rs structure: split header + list, lookup adapter_health
//   by string key, reuse render_adapter_item + health badge, fallback text when
//   Option is None (respects "enabled" and probe failures).
// - Small pure helper tool_health_to_adapter_health() shows how to bridge the
//   stringly health from the stub into our typed AdapterHealth for visuals.
//   In a real ToolFoundryAdapter we would probably return typed health too.
// - No Up/Down/selection on this screen (yet); Scripts didn't have it either.
//   Adding later is easy because Action::Up/Down already guard on screen==Adapters.
// - Educational comments on nearly every line per project rules.
// - Demonstrates the plan's "ToolFoundryAdapter (ownership/lifecycle/health/symlinks)".
