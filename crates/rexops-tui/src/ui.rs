//! ui.rs — Top-level UI coordinator and outer layout.
//!
//! This file is intentionally kept thin. It owns:
//! - The overall vertical layout (header, main content area, status bar)
//! - Dispatching the main content area to the appropriate screen
//!   (Dashboard or Adapters with selection).
//!
//! Styling comes from the shared `suite_ui::Theme`; the help and confirm
//! overlays are the suite's `HelpSheet` / `ConfirmModal`. Per-screen widget
//! construction is delegated to `screens::*`.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use suite_ui::{ConfirmModal, HelpSheet, PaletteFrame, PaletteItem, StatusBar, Theme};

use crate::app::{App, PendingAction};
use crate::screens;
use rexops_core::AppConfig;

/// Main render entry point called every frame.
pub fn render(f: &mut Frame, app: &App, theme: Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(8),    // main content (delegated to screen)
            Constraint::Length(3), // status bar
        ])
        .split(f.area());

    render_header(f, app, chunks[0], theme);
    // Dispatch main content based on current screen (per plan's screens/ structure).
    match app.current_screen {
        crate::app::Screen::Dashboard => {
            screens::render_dashboard(f, app, chunks[1], theme);
        }
        crate::app::Screen::Adapters => {
            screens::render_adapters(f, app, chunks[1], theme);
        }
        crate::app::Screen::System => {
            screens::render_system(f, app, chunks[1], theme);
        }
        crate::app::Screen::Scripts => {
            screens::render_scripts(f, app, chunks[1], theme);
        }
        crate::app::Screen::Tools => {
            screens::render_tools(f, app, chunks[1], theme);
        }
        crate::app::Screen::Launcher => {
            screens::render_launcher(f, app, chunks[1], theme);
        }
        crate::app::Screen::Jobs => {
            screens::render_jobs(f, app, chunks[1], theme);
        }
    }
    render_status_bar(f, app, chunks[2], theme);

    // Nice help overlay popup (toggled with ?/h; press again to close).
    if app.show_help {
        render_help_popup(f, f.area(), theme);
    }

    // The command palette sits above the screen (and the help popup), but BELOW
    // the confirm modal — choosing a `run <tool>` command arms a pending action,
    // and the confirm modal must then be the topmost, sole focus.
    if app.palette_open {
        render_palette(f, app, f.area(), theme);
    }

    // Confirmation modal takes precedence over everything else: if a mutating
    // action is awaiting confirmation, it MUST be the thing the user sees and
    // acts on. Drawn last so it sits on top of the screen, help, and palette.
    if let Some(pending) = &app.pending_action {
        render_confirm_popup(f, pending, &app.config, f.area(), theme);
    }
}

/// The command palette overlay, drawn with the suite's shared `PaletteFrame`.
/// Filtering + selection live on `App`; this just hands the filtered slice over.
fn render_palette(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let commands = app.palette_commands();
    let items: Vec<PaletteItem> = commands
        .iter()
        .map(|c| PaletteItem {
            label: &c.label,
            desc: &c.desc,
        })
        .collect();
    let selected = if items.is_empty() {
        None
    } else {
        Some(app.palette_selected)
    };
    PaletteFrame {
        query: &app.palette_query,
        items: &items,
        selected,
    }
    .render(f, area, theme);
}

fn render_header(f: &mut Frame, app: &App, area: ratatui::layout::Rect, theme: Theme) {
    let title = if app.refreshing {
        "RexOps  —  Dashboard  (refreshing...)"
    } else {
        "RexOps  —  Dashboard"
    };

    let header = Paragraph::new(title).style(theme.title()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.dim()),
    );

    f.render_widget(header, area);
}

fn render_status_bar(f: &mut Frame, app: &App, area: ratatui::layout::Rect, theme: Theme) {
    let available = app.snapshot.any_adapter_available();
    let count = app.snapshot.adapter_health.len();

    // While confirming, the status bar speaks only to the modal — every other
    // hint is irrelevant until the user confirms or cancels.
    let left = if app.pending_action.is_some() {
        "CONFIRM:  Enter = run  •  Esc = cancel"
    } else if app.palette_open {
        "PALETTE:  type to filter  •  ↑/↓ move  •  Enter run  •  Esc close"
    } else {
        match app.current_screen {
            crate::app::Screen::Dashboard => {
                "q quit  •  ^P palette  •  type to filter  •  esc clear  •  r refresh  •  ? help  •  1-7 screens"
            }
            crate::app::Screen::Adapters => {
                "q quit  •  ^P palette  •  j/k nav  •  enter select  •  1 dashboard"
            }
            crate::app::Screen::System => "q quit  •  ^P palette  •  r refresh  •  1 dashboard",
            crate::app::Screen::Scripts => "q quit  •  ^P palette  •  r refresh  •  1 dashboard",
            crate::app::Screen::Tools => "q quit  •  ^P palette  •  r refresh  •  1 dashboard",
            crate::app::Screen::Launcher => {
                "q quit  •  ↑/↓ nav  •  enter run (confirm)  •  esc back  •  1 dashboard"
            }
            crate::app::Screen::Jobs => {
                "q quit  •  ^P palette  •  x cancel job  •  1 dashboard"
            }
        }
    };
    // The right-hand state badge reuses the shared health styling so "available"
    // vs "unavailable" reads the same way as every other health cue in the suite.
    let (right, right_style) = if app.refreshing {
        ("working...", theme.working())
    } else if count == 0 {
        ("no adapters probed", theme.dim())
    } else if available {
        ("adapters available", theme.health(suite_ui::Health::Healthy))
    } else {
        (
            "all adapters unavailable",
            theme.health(suite_ui::Health::Unavailable),
        )
    };

    // The persistent job-status segment from the shared suite chrome, folded into
    // the footer between the keybind hints and the adapter badge. `StatusBar::line`
    // gives us the styled spans so the whole footer stays one bordered row.
    let job_line = StatusBar { job: app.job_state() }.line(theme);

    let mut spans = vec![Span::raw(left), Span::raw("   |   ")];
    spans.extend(job_line.spans);
    spans.push(Span::raw("   |   "));
    spans.push(Span::styled(right, right_style));

    let status =
        Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::ALL));

    f.render_widget(status, area);
}

/// The keybinding help overlay, drawn with the suite's shared `HelpSheet`.
fn render_help_popup(f: &mut Frame, area: Rect, theme: Theme) {
    let rows = [
        ("^P · :", "open the command palette"),
        ("q / Esc / ^C", "quit (Esc clears a filter / closes the palette first)"),
        ("r", "refresh (background thread)"),
        ("? / h", "toggle this help"),
        ("1", "Dashboard — overview, risk, notes"),
        ("2", "Adapters — list + detail; type to filter"),
        ("3", "System"),
        ("4", "Scripts"),
        ("5", "Tools"),
        ("6", "Launcher"),
        ("7", "Jobs — live output of a background job"),
        ("j / k · ↑ / ↓", "move the selection (Adapters / Launcher / palette)"),
        ("Enter", "activate selection / run (asks to confirm)"),
        ("x", "cancel the running job (Jobs screen)"),
        ("backspace", "edit the Adapters filter / palette query"),
    ];
    HelpSheet {
        title: "RexOps Keybindings",
        rows: &rows,
    }
    .render(f, area, theme);
}

/// Render the confirmation modal for a pending mutating action.
///
/// The suite's `ConfirmModal` draws title + message; we fold the dry-run preview
/// (exactly what would run) into the message so the safety affordance survives.
fn render_confirm_popup(
    f: &mut Frame,
    pending: &PendingAction,
    config: &AppConfig,
    area: Rect,
    theme: Theme,
) {
    let message = format!("{}   {}", pending.prompt(), pending.preview(config));
    ConfirmModal {
        title: "⚠ Confirm",
        message: &message,
    }
    .render(f, area, theme);
}
