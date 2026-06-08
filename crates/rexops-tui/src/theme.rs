//! theme.rs — Colors, styles, and theming helpers for the TUI.
//!
//! All health color decisions live here so they are consistent.

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

/// Attention style for the confirmation modal: bright yellow + bold so a
/// pending mutating action is impossible to miss.
pub fn confirm_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}
