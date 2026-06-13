//! Footer status bar: per-screen key hints plus the job/toast/adapter
//! status segments.

use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use suite_ui::{KeyHints, StatusBar, Theme, Toast};

use crate::app::{App, Screen};

/// Footer key hints by screen and modal state.
pub(super) fn screen_hints(app: &App) -> &'static [(&'static str, &'static str)] {
    if app.pending_action.is_some() {
        return &[("Enter/y", "run"), ("n/Esc", "cancel")];
    }
    if app.palette_open {
        return &[
            ("type", "filter"),
            ("↑/↓", "move"),
            ("Enter", "run"),
            ("Esc", "close"),
        ];
    }
    // While actively filtering, the hints reflect the text-input contract: every
    // key types, Enter keeps the filter, Esc abandons it.
    if app.filtering {
        return &[
            ("type", "filter"),
            ("↑/↓", "move"),
            ("Enter", "apply"),
            ("Esc", "clear"),
        ];
    }
    match app.current_screen {
        Screen::Dashboard => &[
            ("q", "quit"),
            ("^P", "palette"),
            ("/", "filter"),
            ("r", "refresh"),
            ("?", "help"),
            ("1-7", "screens"),
        ],
        Screen::Adapters => &[
            ("q", "quit"),
            ("^P", "palette"),
            ("/", "filter"),
            ("j/k", "nav"),
            ("enter", "select"),
            ("1", "dashboard"),
        ],
        Screen::System | Screen::Scripts | Screen::Tools => &[
            ("q", "quit"),
            ("^P", "palette"),
            ("r", "refresh"),
            ("1", "dashboard"),
        ],
        Screen::Launcher => &[
            ("q", "quit"),
            ("↑/↓", "nav"),
            ("enter", "run"),
            ("esc", "back"),
            ("1", "dashboard"),
        ],
        Screen::Jobs => &[
            ("q", "quit"),
            ("^P", "palette"),
            ("x", "cancel job"),
            ("1", "dashboard"),
        ],
    }
}

pub(super) fn render_status_bar(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let available = app.snapshot.any_adapter_available();
    let count = app.snapshot.adapter_health.len();
    let hints_line = KeyHints {
        hints: screen_hints(app),
    }
    .line(theme);

    let (right, right_style) = if app.refreshing {
        ("working...", theme.working())
    } else if count == 0 {
        ("no adapters probed", theme.dim())
    } else if available {
        (
            "adapters available",
            theme.health(suite_ui::Health::Healthy),
        )
    } else {
        (
            "all adapters unavailable",
            theme.health(suite_ui::Health::Unavailable),
        )
    };

    let mut spans = hints_line.spans;
    spans.push(Span::raw("   |   "));
    if let Some((text, kind)) = &app.toast {
        spans.extend(Toast { text, kind: *kind }.line(theme).spans);
    } else {
        spans.extend(
            StatusBar {
                job: app.job_state(),
            }
            .line(theme)
            .spans,
        );
    }
    spans.push(Span::raw("   |   "));
    spans.push(Span::styled(right, right_style));

    let status = Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::ALL));
    f.render_widget(status, area);
}
