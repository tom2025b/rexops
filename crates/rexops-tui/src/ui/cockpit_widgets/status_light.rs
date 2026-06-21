//! status_light.rs — the one-glyph health lamp.
//!
//! `●`/`◍`/`○`/`✗` for healthy/degraded/neutral/down. One glyph, one colour —
//! the warning light the cockpit can't-miss. Colour comes from the shared
//! `Theme` (via the suite's `Health` styling) so "green = healthy" reads
//! identically across the whole suite.

use ratatui::style::Style;
use ratatui::text::Span;
use suite_ui::{Health, Theme};

use crate::ui::cockpit_widgets::LightState;

/// The lamp glyph for a state. Distinct per state so it's unambiguous.
pub fn light_glyph(state: LightState) -> &'static str {
    match state {
        LightState::Healthy => "●",
        LightState::Degraded => "◍",
        LightState::Neutral => "○",
        LightState::Down => "✗",
    }
}

/// The lamp glyph styled with the canonical colour for its state, derived from
/// the shared theme. Healthy/Degraded/Down reuse the theme's `Health` styling so
/// the cockpit matches every other suite surface; Neutral is the theme's dim.
pub fn light_span(state: LightState, theme: Theme) -> Span<'static> {
    let style: Style = match state {
        LightState::Healthy => theme.health(Health::Healthy),
        LightState::Degraded => theme.health(Health::Degraded),
        LightState::Down => theme.health(Health::Unavailable),
        LightState::Neutral => theme.dim(),
    };
    Span::styled(light_glyph(state), style)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::cockpit_widgets::LightState;

    #[test]
    fn each_state_has_its_distinct_glyph() {
        assert_eq!(light_glyph(LightState::Healthy), "●");
        assert_eq!(light_glyph(LightState::Degraded), "◍");
        assert_eq!(light_glyph(LightState::Neutral), "○");
        assert_eq!(light_glyph(LightState::Down), "✗");
        // All four glyphs are distinct so the lamp is unambiguous at a glance.
        let mut all = vec![
            light_glyph(LightState::Healthy),
            light_glyph(LightState::Degraded),
            light_glyph(LightState::Neutral),
            light_glyph(LightState::Down),
        ];
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), 4, "all four lamp glyphs must be distinct");
    }

    #[test]
    fn span_carries_the_glyph_text() {
        // The styled span must render the same glyph (style is theme-dependent
        // and not asserted here; the glyph identity is the contract).
        let theme = Theme::with_color(true);
        let span = light_span(LightState::Healthy, theme);
        assert_eq!(span.content.as_ref(), "●");
    }
}

// Learning Notes
// - The glyph fn is split from the styled-span fn so tests can assert the glyph
//   identity without depending on theme colours (which vary with NO_COLOR).
// - We map onto suite-ui's existing `Health` styling rather than inventing new
//   colours, so the lamp is consistent with the health strip/badges already in
//   the suite. Only "Neutral" has no Health equivalent → theme.dim().
