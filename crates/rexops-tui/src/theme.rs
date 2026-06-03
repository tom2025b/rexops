//! theme.rs — Colors, styles, and theming helpers for the TUI.
//!
//! All "what color is healthy?" decisions live here so they are consistent
//! and easy to change or make configurable later.

use ratatui::style::{Color, Modifier, Style};

use rexops_core::AdapterHealth;

/// Return the style (color + modifiers) to use for a given adapter health.
pub fn health_style(health: &AdapterHealth) -> Style {
    match health {
        AdapterHealth::Healthy => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        AdapterHealth::Degraded => Style::default().fg(Color::Yellow),
        AdapterHealth::Unavailable => Style::default().fg(Color::Red),
        AdapterHealth::Unknown => Style::default().fg(Color::DarkGray),
    }
}

/// Title / header style.
pub fn title_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

/// Border style for blocks.
pub fn border_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// Style for the "refreshing" / working indicator.
pub fn working_style() -> Style {
    Style::default().fg(Color::Yellow)
}

/// Style for help text.
pub fn help_style() -> Style {
    Style::default().fg(Color::Blue)
}

// Learning Notes:
// - Having a dedicated theme module (even a small one) prevents "magic colors"
//   scattered through ui code.
// - We can later extend this to a full Theme struct loaded from config or
//   with dark/light variants without touching every render function.
// - health_style is the most important one because AdapterHealth is the
//   primary status signal the whole app is built around.
