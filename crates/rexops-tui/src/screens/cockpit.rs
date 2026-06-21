//! screens/cockpit.rs — the cockpit landing screen (replaces the Dashboard).
//!
//! The suite's state at a glance: an identity banner, a grid of component status
//! cards grouped by the metaphor (Brain / Monitors / Field Tools / …), and a
//! one-line risk/hint strip. Pure render — it reads only the already-resolved
//! `OpsSnapshot.components` the Phase A registry walk produced, plus system
//! facts and the risk rollup. No I/O, no app mutation.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use suite_ui::{pane, Theme};

use crate::app::App;
use crate::screens::cockpit_nav::GROUP_ORDER;
use crate::ui::cockpit_widgets::card_grid::{render_card_grid, CardSection};
use crate::ui::cockpit_widgets::identity_banner::{render_identity_banner, BannerInput};
use crate::ui::cockpit_widgets::{light_state_from_health, CardInput};

/// Render the cockpit into `area`.
pub fn render_cockpit(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // identity banner
            Constraint::Min(6),    // card grid
            Constraint::Length(1), // risk / hint strip
        ])
        .split(area);

    render_banner(f, app, chunks[0], theme);

    if app.snapshot.components.is_empty() {
        let msg = Paragraph::new(Line::from(
            "No components yet — press 'r' to probe the suite.",
        ))
        .block(pane("Cockpit", theme));
        f.render_widget(msg, chunks[1]);
    } else {
        render_grid(f, app, chunks[1], theme);
    }

    render_risk_strip(f, app, chunks[2], theme);
}

fn render_banner(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let sys = app.snapshot.system.as_ref();
    let live = app
        .snapshot
        .components
        .iter()
        .filter(|c| c.maturity == "live")
        .count();
    let total = app.snapshot.components.len();
    let clock = rexops_core::format_unix_millis_utc(app.snapshot.generated_at_ms);
    let input = BannerInput {
        host: sys.and_then(|s| s.hostname.as_deref()),
        kernel: sys.and_then(|s| s.kernel.as_deref()),
        uptime: sys.and_then(|s| s.uptime.as_deref()),
        clock: &clock,
        live,
        total,
        alerts: alerts_count(app),
    };
    render_identity_banner(f, input, area, theme);
}

/// Render the grouped card grid. Card-input storage is built per group and kept
/// alive on the stack for that group's render (CardInput borrows from each row).
fn render_grid(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let comps = &app.snapshot.components;
    let group_rects = pre_split_groups(area, comps);

    for ((label, ids), grect) in GROUP_ORDER.iter().zip(group_rects.iter()) {
        let inputs: Vec<CardInput> = comps
            .iter()
            .filter(|c| ids.contains(&c.group.as_str()))
            .map(|c| CardInput {
                name: &c.name,
                role: &c.group,
                light: light_state_from_health(c.health),
                vital: c.vital.as_deref(),
                dim: c.maturity == "planned",
            })
            .collect();
        if inputs.is_empty() {
            continue;
        }
        let sections = [CardSection {
            label,
            cards: &inputs,
        }];
        render_card_grid(f, &sections, *grect, theme);
    }
}

/// Vertically split `area` into one rect per GROUP_ORDER entry, proportional to
/// each group's card count (empty groups get a zero-share slice).
fn pre_split_groups(area: Rect, comps: &[rexops_core::ComponentStatus]) -> Vec<Rect> {
    let counts: Vec<u16> = GROUP_ORDER
        .iter()
        .map(|(_, ids)| {
            comps
                .iter()
                .filter(|c| ids.contains(&c.group.as_str()))
                .count() as u16
        })
        .collect();
    let total: u32 = u32::from(counts.iter().copied().sum::<u16>().max(1));
    let constraints: Vec<Constraint> = counts
        .iter()
        .map(|&n| Constraint::Ratio(u32::from(n), total))
        .collect();
    Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area)
        .to_vec()
}

fn render_risk_strip(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let r = &app.snapshot.risk;
    let text = format!(
        "RISK  crit {} · high {} · med {} · low {}    [1] cockpit  [r] refresh  [?] help",
        r.critical, r.high, r.medium, r.low
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(text, theme.dim()))),
        area,
    );
}

/// Alerts = the count that makes the banner loud: critical + high findings.
/// `RiskSummary` counts are `u32`; the banner takes `usize`, so widen here.
fn alerts_count(app: &App) -> usize {
    let r = &app.snapshot.risk;
    (r.critical + r.high) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    use rexops_core::{AdapterHealth, AppConfig, ComponentStatus, OpsSnapshot};
    use std::sync::mpsc;

    fn app_with_components() -> App {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default(), None);
        let mut snap = OpsSnapshot::new();
        snap.push_component(ComponentStatus {
            id: "workstate".into(),
            name: "Workstate".into(),
            group: "brain".into(),
            maturity: "live".into(),
            health: AdapterHealth::Healthy,
            freshness: None,
            vital: Some("3/3 fresh".into()),
            launchable: false,
        });
        snap.push_component(ComponentStatus {
            id: "pulse".into(),
            name: "Pulse".into(),
            group: "monitor".into(),
            maturity: "planned".into(),
            health: AdapterHealth::Unknown,
            freshness: None,
            vital: None,
            launchable: false,
        });
        app.apply_snapshot(snap);
        app
    }

    fn render(app: &App) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("backend");
        let theme = Theme::with_color(false);
        terminal
            .draw(|f| render_cockpit(f, app, f.area(), theme))
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
    fn cockpit_renders_a_card_per_component_grouped() {
        let app = app_with_components();
        let text = render(&app);
        assert!(text.contains("Workstate"), "workstate card:\n{text}");
        assert!(text.contains("Pulse"), "pulse card:\n{text}");
        assert!(text.contains("3/3 fresh"), "workstate vital:\n{text}");
        assert!(
            text.to_uppercase().contains("BRAIN"),
            "brain group:\n{text}"
        );
        assert!(
            text.to_uppercase().contains("MONITOR"),
            "monitor group:\n{text}"
        );
    }

    #[test]
    fn planned_component_renders_neutral_lamp() {
        let app = app_with_components();
        let text = render(&app);
        assert!(
            text.contains('○'),
            "neutral lamp for planned pulse:\n{text}"
        );
    }

    #[test]
    fn empty_components_show_guidance_not_a_blank_screen() {
        let (tx, _rx) = mpsc::channel();
        let app = App::new(tx, AppConfig::default(), None); // no snapshot applied
        let text = render(&app);
        assert!(
            text.to_lowercase().contains("no components")
                || text.to_lowercase().contains("press 'r'"),
            "empty cockpit guides the user instead of rendering blank:\n{text}"
        );
    }
}

// Learning Notes
// - The cockpit is a PURE projection of OpsSnapshot.components — no probing, no
//   mutation — so it unit-tests off-screen exactly like render_dashboard did.
// - CardInput borrows from each ComponentStatus, so the owned `inputs` Vec is
//   built per group and kept on the stack for that group's render call; that's
//   why we render group-by-group rather than building one global slice.
// - GROUP_ORDER makes the metaphor the layout and fixes display order; a
//   component whose group string isn't listed simply isn't shown (none today).
