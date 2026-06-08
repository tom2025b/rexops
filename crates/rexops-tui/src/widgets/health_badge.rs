//! health_badge.rs — Small reusable widget for rendering AdapterHealth.
//!
//! Renders a colored indicator + text for health status.
//! Used in tables/lists to avoid duplicating style logic in every screen.

use ratatui::text::Span;

use rexops_core::AdapterHealth;

use crate::theme;

/// Renders a health badge as a Span (colored text).
/// Example output: "✓ Healthy" in green, "✗ Unavailable" in red, etc.
///
/// This is a pure function — pass the health, get a styled Span back.
/// Callers can put it in a Cell, Line, etc.
pub fn render_health_badge(health: AdapterHealth) -> Span<'static> {
    let (symbol, text) = match health {
        AdapterHealth::Healthy => ("✓", "Healthy"),
        AdapterHealth::Degraded => ("!", "Degraded"),
        AdapterHealth::Unavailable => ("✗", "Unavailable"),
        AdapterHealth::Unknown => ("?", "Unknown"),
    };
    Span::styled(format!("{symbol} {text}"), theme::health_style(&health))
}
