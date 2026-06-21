//! status_card.rs — one component as a card: a lamp + name, its role, one vital.
//!
//! The cockpit's core widget. Three short lines inside the shared rounded
//! `pane`: `<light> <name>` / `<role>` / `<vital>`. One number per instrument —
//! depth lives one keypress away on the detail screens, not here.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use suite_ui::{pane, Theme};

use crate::ui::cockpit_widgets::status_light::light_span;
use crate::ui::cockpit_widgets::CardInput;

/// Draw a single status card into `area`.
pub fn render_status_card(f: &mut Frame, input: CardInput, area: Rect, theme: Theme) {
    let block = pane("", theme);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // light + name
            Constraint::Length(1), // role
            Constraint::Length(1), // vital
        ])
        .split(inner);

    // Plain text is `Style::default()` (the suite Theme has no neutral-text
    // accessor; default renders correctly under both colour and NO_COLOR). A
    // dim/planned card mutes the name and vital via the theme's dim style.
    let text_style: Style = Style::default();
    let name_style = if input.dim { theme.dim() } else { text_style };

    let title = Line::from(vec![
        light_span(input.light, theme),
        Span::raw(" "),
        Span::styled(input.name.to_owned(), name_style),
    ]);
    f.render_widget(Paragraph::new(title), rows[0]);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(input.role.to_owned(), theme.dim()))),
        rows[1],
    );

    let vital = input.vital.unwrap_or("—");
    let vital_style = if input.dim { theme.dim() } else { text_style };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(vital.to_owned(), vital_style))),
        rows[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::cockpit_widgets::{CardInput, LightState};
    use ratatui::{backend::TestBackend, Terminal};

    fn render(input: CardInput) -> String {
        let backend = TestBackend::new(28, 6);
        let mut terminal = Terminal::new(backend).expect("backend");
        let theme = Theme::with_color(false); // NO_COLOR → assert text only
        terminal
            .draw(|f| render_status_card(f, input, f.area(), theme))
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
    fn card_shows_light_name_role_and_vital() {
        let text = render(CardInput {
            name: "Bulwark",
            role: "security",
            light: LightState::Healthy,
            vital: Some("1 crit 1 high"),
            dim: false,
        });
        assert!(text.contains("Bulwark"), "name on the card:\n{text}");
        assert!(text.contains("security"), "role on the card:\n{text}");
        assert!(text.contains("1 crit 1 high"), "vital on the card:\n{text}");
        assert!(
            text.contains('●'),
            "healthy lamp glyph on the card:\n{text}"
        );
    }

    #[test]
    fn card_without_a_vital_shows_a_dash() {
        let text = render(CardInput {
            name: "Pulse",
            role: "heartbeat",
            light: LightState::Neutral,
            vital: None,
            dim: true,
        });
        assert!(text.contains("Pulse"), "name present:\n{text}");
        assert!(
            text.contains('—'),
            "missing vital renders as em dash:\n{text}"
        );
        assert!(text.contains('○'), "neutral lamp glyph:\n{text}");
    }
}

// Learning Notes
// - The card draws the pane, then splits its INNER rect into three 1-line rows.
//   Using `block.inner(area)` before rendering the block is the standard ratatui
//   "frame + content" pattern and keeps the text off the border.
// - `vital.unwrap_or("—")` makes "no number yet" explicit (a planned component)
//   rather than a blank line that reads as a render bug.
