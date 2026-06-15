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

use rexops_core::{AdapterHealth, Freshness};
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

/// Render a Workstate *section*'s freshness as a badge Span.
///
/// Sections carry freshness, not health (see `rexops_core::Freshness`), so this
/// uses neutral styling — `stale` must NOT read as a red/yellow fault. `None`
/// means no Workstate snapshot has been read yet, which renders as "? Unknown".
/// Example output: "✓ fresh", "• stale", "? Unknown".
pub fn render_freshness_badge(freshness: Option<Freshness>, theme: Theme) -> Span<'static> {
    let Some(freshness) = freshness else {
        // No snapshot read yet — distinct from "read, and stale/missing".
        return Span::styled("? Unknown", theme.health(Health::Unknown));
    };
    match freshness {
        // Fresh is the only "good, current" state — green, like a healthy badge.
        Freshness::Fresh => Span::styled("✓ fresh", theme.health(Health::Healthy)),
        // Stale/Missing/Unknown are neutral informational states, not alarms: dim.
        Freshness::Stale => Span::styled("• stale", theme.dim()),
        Freshness::Missing => Span::styled("• missing", theme.dim()),
        Freshness::Unknown => Span::styled("? unknown", theme.dim()),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_badge_text_distinguishes_each_state() {
        let theme = Theme::with_color(false);
        // None = no snapshot read yet → Unknown (NOT a fresh/stale claim).
        assert_eq!(render_freshness_badge(None, theme).content, "? Unknown");
        // Fresh is the only "current/good" badge.
        assert_eq!(
            render_freshness_badge(Some(Freshness::Fresh), theme).content,
            "✓ fresh"
        );
        // Stale/Missing/Unknown are neutral, lowercase, dot-marked — not alarms.
        assert_eq!(
            render_freshness_badge(Some(Freshness::Stale), theme).content,
            "• stale"
        );
        assert_eq!(
            render_freshness_badge(Some(Freshness::Missing), theme).content,
            "• missing"
        );
        assert_eq!(
            render_freshness_badge(Some(Freshness::Unknown), theme).content,
            "? unknown"
        );
    }

    #[test]
    fn fresh_section_does_not_render_the_permanent_unknown_badge() {
        // Regression for the P2: a present, Fresh section must NOT read "? Unknown"
        // (the bug was querying adapter_health, where sections never appear).
        let theme = Theme::with_color(false);
        let badge = render_freshness_badge(Some(Freshness::Fresh), theme).content;
        assert_ne!(
            badge, "? Unknown",
            "a fresh section must not badge as Unknown"
        );
    }
}
