//! card_grid.rs — lay grouped status cards into responsive columns.
//!
//! Each section is a dim label row ("BRAIN") followed by its cards in 1–3
//! columns depending on terminal width, so the cockpit reflows gracefully from
//! a wide terminal down to a narrow one (one column, stacked). The grouping is
//! the metaphor made visible: Brain / Monitors / Field Tools as on-screen rows.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use suite_ui::Theme;

use crate::ui::cockpit_widgets::status_card::render_status_card;
use crate::ui::cockpit_widgets::CardInput;

/// One labelled group of cards.
pub struct CardSection<'a> {
    pub label: &'a str,
    pub cards: &'a [CardInput<'a>],
}

/// Responsive column count. 3 wide, 2 medium, 1 narrow — thresholds chosen so a
/// card (~22 cols + borders) is never crushed below readability.
pub fn columns_for_width(width: u16) -> usize {
    if width >= 66 {
        3
    } else if width >= 44 {
        2
    } else {
        1
    }
}

/// Card height in rows: 3 content lines + 2 border rows.
const CARD_HEIGHT: u16 = 5;

/// Render all sections top-to-bottom into `area`.
pub fn render_card_grid(f: &mut Frame, sections: &[CardSection], area: Rect, theme: Theme) {
    let cols = columns_for_width(area.width);

    // Pre-size each section: 1 label row + ceil(cards/cols) rows of CARD_HEIGHT.
    let section_constraints: Vec<Constraint> = sections
        .iter()
        .map(|s| {
            let rows = s.cards.len().div_ceil(cols).max(1) as u16;
            Constraint::Length(1 + rows * CARD_HEIGHT)
        })
        .collect();

    let section_rects = Layout::default()
        .direction(Direction::Vertical)
        .constraints(section_constraints)
        .split(area);

    for (section, srect) in sections.iter().zip(section_rects.iter()) {
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(*srect);

        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                section.label.to_owned(),
                theme.dim(),
            ))),
            parts[0],
        );

        render_cards_in_columns(f, section.cards, parts[1], cols, theme);
    }
}

/// Lay a section's cards into `cols` columns across `area`, wrapping into rows.
fn render_cards_in_columns(
    f: &mut Frame,
    cards: &[CardInput],
    area: Rect,
    cols: usize,
    theme: Theme,
) {
    if cards.is_empty() {
        return;
    }
    let row_count = cards.len().div_ceil(cols);
    let row_constraints: Vec<Constraint> = (0..row_count)
        .map(|_| Constraint::Length(CARD_HEIGHT))
        .collect();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .split(area);

    for (r, row_rect) in rows.iter().enumerate() {
        let col_constraints: Vec<Constraint> = (0..cols)
            .map(|_| Constraint::Ratio(1, cols as u32))
            .collect();
        let cells = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(col_constraints)
            .split(*row_rect);

        for c in 0..cols {
            let idx = r * cols + c;
            if let Some(card) = cards.get(idx) {
                render_status_card(f, *card, cells[c], theme);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::cockpit_widgets::{CardInput, LightState};
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn column_count_is_responsive_to_width() {
        assert_eq!(columns_for_width(80), 3, "wide → 3 columns");
        assert_eq!(columns_for_width(50), 2, "medium → 2 columns");
        assert_eq!(columns_for_width(30), 1, "narrow → 1 column (stacks)");
    }

    #[test]
    fn grid_renders_section_label_and_all_cards() {
        let cards = [
            CardInput {
                name: "Workstate",
                role: "brain",
                light: LightState::Healthy,
                vital: Some("3/3 fresh"),
                dim: false,
                marker: None,
                focused: false,
            },
            CardInput {
                name: "Bulwark",
                role: "security",
                light: LightState::Degraded,
                vital: Some("1 crit"),
                dim: false,
                marker: None,
                focused: false,
            },
        ];
        let sections = [CardSection {
            label: "BRAIN",
            cards: &cards,
        }];

        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).expect("backend");
        let theme = Theme::with_color(false);
        terminal
            .draw(|f| render_card_grid(f, &sections, f.area(), theme))
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
        assert!(out.contains("BRAIN"), "section label rendered:\n{out}");
        assert!(out.contains("Workstate"), "first card rendered:\n{out}");
        assert!(out.contains("Bulwark"), "second card rendered:\n{out}");
    }
}

// Learning Notes
// - `div_ceil` gives the row count without float math; pre-sizing sections by
//   their card-row count means the vertical layout never clips a card.
// - Columns use `Ratio(1, cols)` so they share width evenly and reflow when the
//   terminal resizes — the responsive behaviour is just `columns_for_width`
//   feeding the constraint count, which is why that fn is extracted + tested.
