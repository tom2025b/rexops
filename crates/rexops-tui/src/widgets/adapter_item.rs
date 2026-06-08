//! adapter_item.rs — Simple widget for rendering a single adapter row in lists.
//!
//! Used by the Adapters screen to render each item with name, health badge,
//! and optional info. Keeps rendering logic reusable and out of the screen.

use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};

use rexops_core::AdapterHealth;
use suite_ui::Theme;

use crate::widgets::health_badge;

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
    let badge = health_badge::render_health_badge(health, theme);
    Line::from(vec![
        name_span,
        Span::raw(" "),
        badge,
        Span::raw(format!(" — {info}")),
    ])
}
