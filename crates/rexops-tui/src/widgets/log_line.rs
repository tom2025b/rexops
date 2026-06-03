//! log_line.rs — Simple widget for rendering a single log/event line.
//!
//! Used in the Dashboard's Events/Logs pane. Keeps the rendering (timestamp
//! prefix, color for level, etc.) out of the screen code.
//!
//! Currently very basic (just the text with optional prefix). Future could
//! add levels (info/warn/error) with colors.

use ratatui::text::Line;

/// Renders a log line. For now just wraps the message.
/// In a richer version this would take a timestamp, level, etc.
pub fn render_log_line(msg: &str) -> Line<'static> {
    // Prefix with bullet for list-like appearance in the pane.
    Line::from(format!("• {msg}"))
}

// Learning Notes:
// - Extracting even tiny renderers like this to widgets/ makes the
//   dashboard.rs much cleaner and easier to test or theme.
// - The events are currently just strings in App; a real impl might
//   use a structured LogEvent enum here.
