//! Top-level frame layout: splits the terminal into header / body / footer,
//! renders the header chrome, and routes the body to the active screen.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use suite_ui::Theme;

use super::{palette, status_bar};
use crate::app::{App, Modal, Screen};
use crate::screens;

pub fn render(f: &mut Frame, app: &App, theme: Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(f.area());

    render_header(f, app, chunks[0], theme);
    match app.current_screen {
        Screen::Dashboard => screens::render_dashboard(f, app, chunks[1], theme),
        Screen::Adapters => screens::render_adapters(f, app, chunks[1], theme),
        Screen::System => screens::render_system(f, app, chunks[1], theme),
        Screen::Scripts => screens::render_scripts(f, app, chunks[1], theme),
        Screen::Tools => screens::render_tools(f, app, chunks[1], theme),
        Screen::Launcher => screens::render_launcher(f, app, chunks[1], theme),
        Screen::Jobs => screens::render_jobs(f, app, chunks[1], theme),
    }
    status_bar::render_status_bar(f, app, chunks[2], theme);

    // Render exactly the overlay App::active_modal reports as on top — the SAME
    // source of truth on_action gates input by, so the modal drawn here is
    // always the one capturing keys. Only the topmost modal renders; any
    // lower-precedence state stays behind it.
    match app.active_modal() {
        Modal::Help => palette::render_help_popup(f, f.area(), theme),
        Modal::Palette => palette::render_palette(f, app, f.area(), theme),
        Modal::Confirm => {
            if let Some(pending) = &app.pending_action {
                palette::render_confirm_popup(f, pending, app.config(), f.area(), theme);
            }
        }
        Modal::None => {}
    }
}

fn render_header(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let screen_name = match app.current_screen {
        Screen::Dashboard => "Dashboard",
        Screen::Adapters => "Adapters",
        Screen::System => "System",
        Screen::Scripts => "Scripts",
        Screen::Tools => "Tools",
        Screen::Launcher => "Launcher",
        Screen::Jobs => "Jobs",
    };
    let title = if app.refreshing {
        format!("RexOps  —  {screen_name}  (refreshing...)")
    } else {
        format!("RexOps  —  {screen_name}")
    };

    let header = Paragraph::new(title).style(theme.title()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.dim()),
    );

    f.render_widget(header, area);
}
