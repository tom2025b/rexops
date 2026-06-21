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
        ("?", "toggle this help"),
        (
            "/",
            "filter the list (Dashboard / Adapters); Esc clears, Enter keeps",
        ),
        ("1", "Dashboard — overview, risk, notes"),
        ("2", "Adapters — list + detail"),
        ("3", "System"),
        ("4", "Scripts"),
        ("5", "Tools"),
        ("6", "Launcher"),
        ("7", "Jobs — live output of a background job"),
        (
            "j / k · ↑ / ↓",
            "move selection (cockpit cards / Adapters / Launcher / palette); scroll output (Jobs)",
        ),
        (
            "a-z (cockpit)",
            "press a card's letter to launch it (confirm first)",
        ),
        ("g", "drill into the focused cockpit card's detail"),
        (
            "Enter",
            "activate selection / launch a launchable card / run enabled tools",
        ),
        ("y / n", "confirm / cancel a pending run"),
        ("x", "cancel the running job (Jobs screen)"),
        ("backspace", "edit the filter / palette query"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Render the help overlay off-screen and flatten it to text, the same
    /// buffer-to-string approach the screen tests use.
    fn help_text() -> String {
        let backend = TestBackend::new(90, 30);
        let mut terminal = Terminal::new(backend).expect("test backend");
        let theme = Theme::with_color(true);
        terminal
            .draw(|f| render_help_popup(f, f.area(), theme))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let width = buffer.area.width as usize;
        let mut out = String::new();
        for (i, cell) in buffer.content.iter().enumerate() {
            if i % width == 0 && i != 0 {
                out.push('\n');
            }
            out.push_str(cell.symbol());
        }
        out
    }

    #[test]
    fn help_documents_the_slash_filter_and_drops_the_dead_h_toggle() {
        // The help sheet is the canonical keybinding reference; it must match the
        // shipped keymap. After the input overhaul: `/` enters filter mode, and
        // `h` is NO LONGER a help toggle (it collided with vim-h). Guard both so
        // the reference can't drift from the keymap again.
        let text = help_text();

        // The `/` filter feature is documented.
        assert!(
            text.contains("filter the list"),
            "help must document the / filter:\n{text}"
        );

        // `h` is no longer advertised as a help toggle. The old row read
        // "? / h"; the corrected one reads just "?". Assert the dead pairing is
        // gone.
        assert!(
            !text.contains("? / h"),
            "help must not advertise the removed `h` toggle:\n{text}"
        );

        // The modal filter replaced "type to filter"; that stale phrasing must
        // be gone.
        assert!(
            !text.contains("type to filter"),
            "help must not describe the old bare-typing filter model:\n{text}"
        );
    }

    #[test]
    fn help_documents_the_cockpit_card_hotkeys() {
        let text = help_text();
        assert!(text.contains("card"), "help mentions card hotkeys:\n{text}");
        assert!(
            text.to_lowercase().contains("drill"),
            "help mentions drill:\n{text}"
        );
    }
}
