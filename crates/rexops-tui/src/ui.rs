//! ui.rs — Top-level UI coordinator and outer layout.
//!
//! This file is intentionally kept thin. It owns:
//! - The overall vertical layout (header, main content area, status bar)
//! - Dispatching the main content area to the appropriate screen
//!   (Dashboard or Adapters with selection).
//!
//! All actual widget construction and styling is delegated to
//! `screens::dashboard` and `theme`.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, PendingAction};
use crate::screens;
use crate::theme;
use rexops_core::AppConfig;

/// Main render entry point called every frame.
pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(8),    // main content (delegated to screen)
            Constraint::Length(3), // status bar
        ])
        .split(f.area());

    render_header(f, app, chunks[0]);
    // Dispatch main content based on current screen (per plan's screens/ structure).
    match app.current_screen {
        crate::app::Screen::Dashboard => {
            screens::render_dashboard(f, app, chunks[1]);
        }
        crate::app::Screen::Adapters => {
            screens::render_adapters(f, app, chunks[1]);
        }
        crate::app::Screen::System => {
            screens::render_system(f, app, chunks[1]);
        }
        crate::app::Screen::Scripts => {
            screens::render_scripts(f, app, chunks[1]);
        }
        crate::app::Screen::Tools => {
            screens::render_tools(f, app, chunks[1]);
        }
        crate::app::Screen::Launcher => {
            screens::render_launcher(f, app, chunks[1]);
        }
    }
    render_status_bar(f, app, chunks[2]);

    // Nice help overlay popup (toggled with ?/h; press again to close).
    if app.show_help {
        render_help_popup(f, f.area());
    }

    // Confirmation modal takes precedence over everything else: if a mutating
    // action is awaiting confirmation, it MUST be the thing the user sees and
    // acts on. Drawn last so it sits on top of the screen and the help popup.
    if let Some(pending) = &app.pending_action {
        render_confirm_popup(f, pending, &app.config, f.area());
    }
}

fn render_header(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let title = if app.refreshing {
        "RexOps  —  Dashboard  (refreshing...)"
    } else {
        "RexOps  —  Dashboard"
    };

    let header = Paragraph::new(title).style(theme::title_style()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme::border_style()),
    );

    f.render_widget(header, area);
}

fn render_status_bar(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let available = app.snapshot.any_adapter_available();
    let count = app.snapshot.adapter_health.len();

    // While confirming, the status bar speaks only to the modal — every other
    // hint is irrelevant until the user confirms or cancels.
    let left = if app.pending_action.is_some() {
        "CONFIRM:  Enter = run  •  Esc = cancel"
    } else {
        match app.current_screen {
            crate::app::Screen::Dashboard => {
                "q quit  •  r refresh  •  ? help  •  1/2/3/4/5/6 screens"
            }
            crate::app::Screen::Adapters => {
                "q quit  •  r refresh  •  ? help  •  j/k nav  •  enter select  •  1 dashboard"
            }
            crate::app::Screen::System => "q quit  •  r refresh  •  ? help  •  1 dashboard",
            crate::app::Screen::Scripts => "q quit  •  r refresh  •  ? help  •  1 dashboard",
            crate::app::Screen::Tools => "q quit  •  r refresh  •  ? help  •  1 dashboard",
            crate::app::Screen::Launcher => {
                "q quit  •  ↑/↓ nav  •  enter launch  •  esc back  •  1 dashboard"
            }
        }
    };
    let right = if app.refreshing {
        "working..."
    } else if count == 0 {
        "no adapters probed"
    } else if available {
        "adapters available"
    } else {
        "all adapters unavailable"
    };

    let status = Paragraph::new(Line::from(vec![
        Span::raw(left),
        Span::raw("   |   "),
        Span::styled(
            right,
            ratatui::style::Style::default().fg(if available {
                ratatui::style::Color::Green
            } else {
                ratatui::style::Color::Red
            }),
        ),
    ]))
    .block(Block::default().borders(Borders::ALL));

    f.render_widget(status, area);
}

fn render_help_popup(f: &mut Frame, area: Rect) {
    let popup_area = centered_rect(55, 45, area);
    let help_text = vec![
        Line::from("RexOps TUI Help"),
        Line::from(""),
        Line::from("Global: q/Esc/Ctrl-C quit  •  r refresh (bg thread)  •  ?/h toggle this"),
        Line::from("Screens: 1 Dashboard (overview + risk + notes)  •  2 Adapters (list + detail)  •  3 System  •  4 Scripts  •  5 Tools  •  6 Launcher"),
        Line::from(""),
        Line::from(
            "In Adapters: j/k or ↑/↓ move  •  enter activate (note)  •  type to filter live",
        ),
        Line::from(
            "In Launcher: ↑/↓ select a tool  •  enter launch  •  esc back to Dashboard",
        ),
        Line::from("             esc = clear filter (or quit if none)  •  backspace edit filter"),
        Line::from(""),
        Line::from(
            "Selection and filter persist across refreshes. System adapter is always healthy.",
        ),
        Line::from(""),
        Line::from("Press ?/h again to close. See README and docs/TUI_DESIGN.md for more."),
    ];
    let popup = Paragraph::new(help_text).wrap(Wrap { trim: true }).block(
        Block::default()
            .title(" Help Overlay (press ?/h to close) ")
            .borders(Borders::ALL)
            .border_style(theme::border_style()),
    );
    f.render_widget(Clear, popup_area);
    f.render_widget(popup, popup_area);
}

/// Render the confirmation modal for a pending mutating action.
///
/// Deliberately loud and explicit (hard to miss): a yellow bordered box, a
/// bold ⚠ CONFIRM title, the action prompt, the dry-run preview of exactly what
/// would run, and an unambiguous Enter = YES / Esc = no affordance.
fn render_confirm_popup(f: &mut Frame, pending: &PendingAction, config: &AppConfig, area: Rect) {
    let popup_area = centered_rect(60, 35, area);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(pending.prompt(), theme::confirm_style())),
        Line::from(""),
        // The preview line IS the dry-run: what would happen, shown before it does.
        Line::from(Span::raw(pending.preview(config))),
        Line::from(""),
        Line::from(Span::styled(
            "[ Enter = YES, run it ]    [ Esc = no, cancel ]",
            theme::confirm_style(),
        )),
        Line::from(Span::styled(
            "Nothing runs until you press Enter.",
            theme::help_style(),
        )),
    ];

    let popup = Paragraph::new(lines).wrap(Wrap { trim: true }).block(
        Block::default()
            .title(" ⚠ CONFIRM ")
            .borders(Borders::ALL)
            .border_style(theme::confirm_style()),
    );

    // Clear erases whatever is behind the popup so the modal is fully opaque.
    f.render_widget(Clear, popup_area);
    f.render_widget(popup, popup_area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    let vert = popup_layout[1];
    let horiz_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert);
    horiz_layout[1]
}
