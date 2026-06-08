//! log_line.rs — Simple widget for rendering a single log/event line.
//!
//! Used in the Dashboard's Events/Logs pane. Keeps the rendering (timestamp
//! prefix, color for level, etc.) out of the screen code.
//!
//! Currently renders the text with a simple prefix.

use ratatui::text::Line;

/// Render a log line.
pub fn render_log_line(msg: &str) -> Line<'static> {
    // Prefix with bullet for list-like appearance in the pane.
    Line::from(format!("• {msg}"))
}
