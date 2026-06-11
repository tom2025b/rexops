//! Reusable ratatui widgets shared by the screens: health badges, adapter
//! list rows, and log/event lines. Each is a pure function from core data
//! (e.g. `AdapterHealth`) to ratatui primitives (Span / Line), so screens
//! compose without duplicating rendering logic.
//!
//! This module also owns the one piece of health glue RexOps still needs:
//! mapping its own `AdapterHealth` onto the suite's shared `Health` so the
//! shared, NO_COLOR-safe styling from `suite_ui::Theme::health` applies.

use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};

use rexops_core::AdapterHealth;
use suite_ui::{Health, Theme};

/// Convert an `AdapterHealth` into the suite's `Health` (a 1:1 mapping).
pub fn health_to_suite(health: AdapterHealth) -> Health {
    match health {
        AdapterHealth::Healthy => Health::Healthy,
        AdapterHealth::Degraded => Health::Degraded,
        AdapterHealth::Unavailable => Health::Unavailable,
        AdapterHealth::Unknown => Health::Unknown,
    }
}

/// Renders a health badge as a Span (colored text).
/// Example output: "✓ Healthy" in green, "✗ Unavailable" in red, etc.
pub fn render_health_badge(health: AdapterHealth, theme: Theme) -> Span<'static> {
    let (symbol, text) = match health {
        AdapterHealth::Healthy => ("✓", "Healthy"),
        AdapterHealth::Degraded => ("!", "Degraded"),
        AdapterHealth::Unavailable => ("✗", "Unavailable"),
        AdapterHealth::Unknown => ("?", "Unknown"),
    };
    Span::styled(
        format!("{symbol} {text}"),
        theme.health(health_to_suite(health)),
    )
}

/// Renders an adapter list item as a Line.
/// Includes prefix for selection, name, health badge, and info snippet.
pub fn render_adapter_item(
    name: &str,
    health: AdapterHealth,
    info: &str,
    is_selected: bool,
    theme: Theme,
) -> Line<'static> {
    let prefix = if is_selected { "▶ " } else { "  " };
    let name_span = if is_selected {
        Span::styled(format!("{prefix}{name}"), Style::new().bold())
    } else {
        Span::raw(format!("{prefix}{name}"))
    };
    let badge = render_health_badge(health, theme);
    Line::from(vec![
        name_span,
        Span::raw(" "),
        badge,
        Span::raw(format!(" — {info}")),
    ])
}

/// Render a log/event line for the Dashboard's Events pane.
pub fn render_log_line(msg: &str) -> Line<'static> {
    Line::from(format!("• {msg}"))
}
