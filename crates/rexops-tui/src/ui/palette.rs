//! Modal overlays: the command palette, help sheet, and confirm popup.

use ratatui::{layout::Rect, Frame};
use rexops_core::AppConfig;
use suite_ui::{ConfirmModal, HelpSheet, PaletteFrame, PaletteItem, Theme};

use crate::app::App;
use crate::commands::PendingAction;

pub(super) fn render_palette(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let commands = app.palette_commands();
    let items: Vec<PaletteItem> = commands
        .iter()
        .map(|command| PaletteItem {
            label: &command.label,
            desc: &command.desc,
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

pub(super) fn render_help_popup(f: &mut Frame, area: Rect, theme: Theme) {
    let rows = [
        ("^P · :", "open the command palette"),
        (
            "q / Esc / ^C",
            "quit (Esc clears a filter / closes the palette first)",
        ),
        ("r", "refresh (background thread)"),
        ("? / h", "toggle this help"),
        ("1", "Dashboard — overview, risk, notes"),
        ("2", "Adapters — list + detail; type to filter"),
        ("3", "System"),
        ("4", "Scripts"),
        ("5", "Tools"),
        ("6", "Launcher"),
        ("7", "Jobs — live output of a background job"),
        (
            "j / k · ↑ / ↓",
            "move the selection (Adapters / Launcher / palette)",
        ),
        ("Enter", "activate selection / run enabled tools"),
        ("y / n", "confirm / cancel a pending run"),
        ("x", "cancel the running job (Jobs screen)"),
        ("backspace", "edit the Adapters filter / palette query"),
    ];
    HelpSheet {
        title: "RexOps Keybindings",
        rows: &rows,
    }
    .render(f, area, theme);
}

pub(super) fn render_confirm_popup(
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
