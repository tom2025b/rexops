use std::sync::mpsc;

use rexops_core::AppConfig;
use suite_ui::Theme;

use super::status_bar::{render_status_bar, screen_hints};
use crate::app::{App, Screen};
use crate::commands::PendingAction;
use crate::jobs::LastOutcome;

fn app_on(screen: Screen) -> App {
    let (tx, _rx) = mpsc::channel();
    let mut app = App::new(tx, AppConfig::default());
    app.current_screen = screen;
    app
}

#[test]
fn every_screen_has_non_empty_hints() {
    for screen in [
        Screen::Dashboard,
        Screen::Adapters,
        Screen::System,
        Screen::Scripts,
        Screen::Tools,
        Screen::Launcher,
        Screen::Jobs,
    ] {
        let app = app_on(screen);
        let hints = screen_hints(&app);
        assert!(!hints.is_empty(), "{screen:?} must show footer hints");
        assert_eq!(hints[0].0, "q", "{screen:?} should lead with quit");
    }
}

#[test]
fn footer_hints_advertise_navigation_and_scrollback() {
    // The footer is the always-visible discoverability surface; it must match
    // behaviour. The Dashboard is navigable (j/k move the adapter selection) and
    // the Jobs screen scrolls its output (j/k) — both must be advertised, or the
    // legend trains users to think the keys are inert.
    let dash = screen_hints(&app_on(Screen::Dashboard));
    assert!(
        dash.iter().any(|(k, _)| *k == "j/k"),
        "the Dashboard is navigable; its hints must advertise j/k: {dash:?}"
    );

    let jobs = screen_hints(&app_on(Screen::Jobs));
    assert!(
        jobs.iter().any(|(k, v)| *k == "j/k" && v.contains("scroll")),
        "the Jobs screen scrolls output; its hints must advertise j/k scroll: {jobs:?}"
    );
}

#[test]
fn a_pending_action_overrides_the_screen_hints_with_confirm() {
    let mut app = app_on(Screen::Dashboard);
    app.pending_action = Some(PendingAction::RunJob {
        id: "x".to_owned(),
        name: "x".to_owned(),
    });
    assert_eq!(
        screen_hints(&app),
        &[("Enter/y", "run"), ("n/Esc", "cancel")]
    );
}

#[test]
fn an_open_palette_overrides_the_screen_hints() {
    let mut app = app_on(Screen::Adapters);
    app.palette_open = true;
    let hints = screen_hints(&app);
    assert!(
        hints.iter().any(|(_, label)| *label == "close"),
        "palette hints shown"
    );
    assert!(!hints.iter().any(|(_, label)| *label == "select"));
}

#[test]
fn confirm_takes_precedence_over_an_open_palette() {
    let mut app = app_on(Screen::Jobs);
    app.palette_open = true;
    app.pending_action = Some(PendingAction::LaunchTool {
        id: "x".to_owned(),
        name: "x".to_owned(),
    });
    assert_eq!(
        screen_hints(&app),
        &[("Enter/y", "run"), ("n/Esc", "cancel")]
    );
}

fn footer_text(app: &App) -> String {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let backend = TestBackend::new(120, 3);
    let mut terminal = Terminal::new(backend).expect("test backend");
    let theme = Theme::with_color(false);
    terminal
        .draw(|f| render_status_bar(f, app, f.area(), theme))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    buffer.content.iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn an_active_toast_replaces_the_status_bar_job_segment() {
    let mut app = app_on(Screen::Jobs);
    app.last_outcome = Some(LastOutcome {
        name: "backup".into(),
        ok: true,
        cancelled: false,
    });
    app.toast = Some(("backup — done".into(), suite_ui::ToastKind::Success));

    let footer = footer_text(&app);
    assert!(
        footer.contains("backup — done"),
        "the outcome must be shown"
    );
    assert_eq!(
        footer.matches("backup — done").count(),
        1,
        "the outcome must appear exactly once, not duplicated: {footer:?}"
    );
}

#[test]
fn the_status_bar_job_segment_shows_once_the_toast_is_cleared() {
    let mut app = app_on(Screen::Jobs);
    app.last_outcome = Some(LastOutcome {
        name: "backup".into(),
        ok: true,
        cancelled: false,
    });
    app.toast = None;

    let footer = footer_text(&app);
    assert_eq!(
        footer.matches("backup — done").count(),
        1,
        "the StatusBar segment shows the outcome once: {footer:?}"
    );
}
