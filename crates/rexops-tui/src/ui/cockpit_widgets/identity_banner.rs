//! identity_banner.rs — the cockpit's top bar: who/where + a one-glance rollup.
//!
//! `host · kernel · up <uptime>      <clock>   N/M live · K alerts`. Absent
//! facts are omitted (no empty `·` runs). The alert count is the one place the
//! banner gets loud: 0 alerts reads calm, >0 uses the theme's severity styling.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use suite_ui::{Severity, Theme};

/// Borrowed inputs for the banner.
pub struct BannerInput<'a> {
    pub host: Option<&'a str>,
    pub kernel: Option<&'a str>,
    pub uptime: Option<&'a str>,
    pub clock: &'a str,
    pub live: usize,
    pub total: usize,
    pub alerts: usize,
}

/// Render the banner into `area` (expects a 1-row-tall rect).
pub fn render_identity_banner(f: &mut Frame, input: BannerInput, area: Rect, theme: Theme) {
    // Identity segments: only the present ones, joined by " · " — so an absent
    // host/kernel/uptime never leaves a dangling separator.
    let mut left: Vec<String> = Vec::new();
    if let Some(h) = input.host {
        left.push(h.to_owned());
    }
    if let Some(k) = input.kernel {
        left.push(format!("kernel {k}"));
    }
    if let Some(u) = input.uptime {
        left.push(format!("up {u}"));
    }
    let identity = left.join(" · ");

    let rollup = format!("{}/{} live", input.live, input.total);
    let alerts = format!("{} alerts", input.alerts);

    // Plain text is `Style::default()`; the loud-alert style reuses the suite's
    // severity axis (Critical) so trouble reads the same red+bold as everywhere.
    let text_style: Style = Style::default();
    let alert_style = if input.alerts > 0 {
        theme.severity(Severity::Critical)
    } else {
        theme.dim()
    };

    let line = Line::from(vec![
        Span::styled(identity, theme.dim()),
        Span::raw("    "),
        Span::styled(input.clock.to_owned(), theme.dim()),
        Span::raw("   "),
        Span::styled(rollup, text_style),
        Span::styled(" · ", theme.dim()),
        Span::styled(alerts, alert_style),
    ]);

    f.render_widget(Paragraph::new(line), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn render(input: BannerInput) -> String {
        let backend = TestBackend::new(80, 3);
        let mut terminal = Terminal::new(backend).expect("backend");
        let theme = Theme::with_color(false);
        terminal
            .draw(|f| render_identity_banner(f, input, f.area(), theme))
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
    fn banner_shows_identity_and_rollup() {
        let text = render(BannerInput {
            host: Some("rex-laptop"),
            kernel: Some("6.17"),
            uptime: Some("4d 2h"),
            clock: "2026-06-20 21:14Z",
            live: 3,
            total: 11,
            alerts: 2,
        });
        assert!(text.contains("rex-laptop"), "host:\n{text}");
        assert!(text.contains("6.17"), "kernel:\n{text}");
        assert!(text.contains("3/11 live"), "live rollup:\n{text}");
        assert!(text.contains("2 alerts"), "alert count:\n{text}");
    }

    #[test]
    fn banner_omits_absent_fields_without_empty_separators() {
        let text = render(BannerInput {
            host: None,
            kernel: None,
            uptime: None,
            clock: "now",
            live: 0,
            total: 0,
            alerts: 0,
        });
        // No host/kernel/uptime → the line must not contain a stray "· ·" run.
        assert!(!text.contains("· ·"), "no empty separator runs:\n{text}");
        assert!(text.contains("now"), "clock still shown:\n{text}");
    }
}

// Learning Notes
// - Building the identity from a Vec joined by " · " is what prevents the empty
//   "· ·" runs when host/kernel/uptime are absent — you can't get a separator
//   without two real segments.
// - The alert span is the only conditional style: calm at 0, loud above. That's
//   the "calm by default, loud on trouble" rule applied at the banner level.
