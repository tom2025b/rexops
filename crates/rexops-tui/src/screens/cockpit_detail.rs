//! screens/cockpit_detail.rs — the per-component drill-down (Phase C).
//!
//! Reached by pressing Enter on a non-launchable card or `g` on any focused
//! card. Shows the one component in depth: its registry identity (role, group,
//! maturity, whether it launches) joined with its live status (health, vital,
//! freshness). Pure render over `app.selected_component`. Esc backs out to the
//! cockpit; Enter launches it if it is launchable.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use suite_ui::{pane, Heartbeat, Theme};

use crate::app::App;
use crate::ui::cockpit_widgets::light_state_from_health;
use crate::ui::cockpit_widgets::status_light::light_span;

/// Render the detail screen for `app.selected_component`.
pub fn render_cockpit_detail(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let Some(id) = app.selected_component.as_deref() else {
        let msg = Paragraph::new(Line::from(
            "No component selected — press 1 for the cockpit.",
        ))
        .block(pane("Detail", theme));
        f.render_widget(msg, area);
        return;
    };

    let live = app.snapshot.components.iter().find(|c| c.id == id);
    let reg = rexops_core::component_by_id(id);

    let mut lines: Vec<Line> = Vec::new();

    // Title: lamp + name.
    let (name, health) = match live {
        Some(c) => (c.name.as_str(), c.health),
        None => (id, rexops_core::AdapterHealth::Unknown),
    };
    lines.push(Line::from(vec![
        light_span(light_state_from_health(health), theme),
        Span::raw(" "),
        Span::styled(name.to_owned(), theme.title()),
    ]));

    // Registry identity.
    if let Some(r) = reg {
        lines.push(Line::from(Span::styled(
            format!("role: {}", r.role),
            theme.dim(),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "launch: {}",
                if r.launch.is_some() {
                    "yes (Enter to run)"
                } else {
                    "none (read-only)"
                }
            ),
            theme.dim(),
        )));
    }

    // Live status.
    if let Some(c) = live {
        let vital = c.vital.as_deref().unwrap_or("—");
        lines.push(Line::from(Span::styled(
            format!("vital: {vital}"),
            theme.dim(),
        )));
        lines.push(Line::from(Span::styled(
            format!("status: {}", c.maturity),
            theme.dim(),
        )));
    }

    // Heartbeat history: only for StatusCommand components. Shows the sparkline
    // over the full ring buffer so the operator can see the recent response-time
    // trend without leaving the drill-down. Falls back gracefully to "no data yet"
    // when the buffer is empty (e.g. before the first probe completes).
    if reg.is_some_and(|r| matches!(r.health, rexops_core::HealthSource::StatusCommand { .. })) {
        let samples = app.heartbeats.samples(id);
        let latest = app.heartbeats.latest(id);
        let hb_text = Heartbeat {
            samples: &samples,
            latest_ms: latest,
        }
        .text();
        lines.push(Line::from(Span::styled(
            format!("heartbeat: {hb_text}"),
            theme.dim(),
        )));
    }

    lines.push(Line::from(Span::styled(
        "Enter launches (if launchable) · Esc back to cockpit",
        theme.dim(),
    )));

    f.render_widget(Paragraph::new(lines).block(pane("Component", theme)), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    use rexops_core::{AdapterHealth, AppConfig, ComponentStatus, OpsSnapshot};
    use std::sync::mpsc;

    fn app_focused_on(id: &str) -> App {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default(), None);
        let mut snap = OpsSnapshot::new();
        snap.push_component(ComponentStatus {
            id: "bulwark".into(),
            name: "Bulwark".into(),
            group: "field tool".into(),
            maturity: "live".into(),
            health: AdapterHealth::Healthy,
            freshness: None,
            vital: Some("1 crit 1 high".into()),
            launchable: true,
        });
        app.apply_snapshot(snap);
        app.selected_component = Some(id.to_owned());
        app
    }

    fn render(app: &App) -> String {
        let backend = TestBackend::new(60, 12);
        let mut terminal = Terminal::new(backend).expect("backend");
        let theme = Theme::with_color(false);
        terminal
            .draw(|f| render_cockpit_detail(f, app, f.area(), theme))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let w = buf.area.width as usize;
        let mut out = String::new();
        for (i, cell) in buf.content.iter().enumerate() {
            if i % w == 0 && i != 0 {
                out.push('\n');
            }
            out.push_str(cell.symbol());
        }
        out
    }

    #[test]
    fn detail_shows_identity_and_live_vital() {
        let text = render(&app_focused_on("bulwark"));
        assert!(text.contains("Bulwark"), "name:\n{text}");
        assert!(text.contains("security"), "registry role:\n{text}");
        assert!(text.contains("1 crit 1 high"), "live vital:\n{text}");
        assert!(text.contains("Esc"), "back hint:\n{text}");
    }

    #[test]
    fn detail_without_a_selection_guides_the_user() {
        let (tx, _rx) = mpsc::channel();
        let app = App::new(tx, AppConfig::default(), None); // nothing selected
        let text = render(&app);
        assert!(
            text.to_lowercase().contains("no component selected"),
            "empty detail guides instead of blank:\n{text}"
        );
    }

    /// Build an app focused on Pulse with heartbeat samples recorded.
    fn app_focused_on_pulse_with_samples() -> App {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default(), None);
        let mut snap = OpsSnapshot::new();
        snap.push_component(ComponentStatus {
            id: "pulse".into(),
            name: "Pulse".into(),
            group: "monitor".into(),
            maturity: "live".into(),
            health: AdapterHealth::Healthy,
            freshness: None,
            vital: Some("plain vital".into()),
            launchable: true,
        });
        app.apply_snapshot(snap);
        app.selected_component = Some("pulse".to_owned());
        // Record samples so the heartbeat section has data.
        app.heartbeats.record("pulse", 4);
        app.heartbeats.record("pulse", 7);
        app.heartbeats.record("pulse", 12);
        app
    }

    #[test]
    fn detail_shows_heartbeat_section_for_status_command_component() {
        // A StatusCommand component (pulse) drill-down must show a heartbeat line
        // with the ♥ glyph from the HeartbeatLog sparkline.
        let text = render(&app_focused_on_pulse_with_samples());
        assert!(text.contains("Pulse"), "name present:\n{text}");
        // The ♥ glyph from Heartbeat::text() must be present.
        assert!(
            text.contains('♥'),
            "StatusCommand detail shows heartbeat ♥ glyph:\n{text}"
        );
    }

    #[test]
    fn detail_does_not_show_heartbeat_section_for_non_status_command_component() {
        // A non-StatusCommand component (bulwark uses Probe) must NOT show a
        // heartbeat section in its drill-down.
        let text = render(&app_focused_on("bulwark"));
        // No samples recorded for bulwark, so no ♥ glyph in the output.
        assert!(
            !text.contains('♥'),
            "non-StatusCommand detail must not show heartbeat:\n{text}"
        );
    }
}

// Learning Notes
// - The detail JOINS two sources: the static registry row (component_by_id) for
//   identity that never changes (role, whether it launches) and the live
//   ComponentStatus for state (health, vital). Neither alone is the full story.
// - It reads only app.selected_component — the same id the cockpit focus uses —
//   so "drill into the focused card" needs no extra plumbing than the screen swap.
// - The heartbeat section appears only for StatusCommand components; it is guarded
//   by a registry lookup so a Probe or Feed component cannot accidentally show it.
