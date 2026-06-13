use super::*;

#[test]
fn esc_closes_the_help_overlay_instead_of_quitting() {
    // The regression: the help sheet rendered over everything but did not
    // capture input, so Esc fell through to the normal Cancel path and quit the
    // app. With the modal gate, Esc must close help and NOT quit.
    let mut app = bare_app();
    app.on_action(Action::ToggleHelp, &mut FakeRunner { calls: 0 });
    assert!(app.show_help, "? opens the help overlay");

    let quit = app.on_action(Action::Cancel, &mut FakeRunner { calls: 0 });

    assert!(!quit, "Esc on the help overlay must not quit");
    assert!(!app.show_help, "Esc must close the help overlay");
}

#[test]
fn any_key_dismisses_help_and_does_not_reach_the_screen_behind() {
    // While help is up, a key that would normally switch screens / enter filter
    // mode must only dismiss the overlay — the screen behind it stays untouched.
    let mut app = bare_app();
    app.current_screen = Screen::Dashboard;
    app.on_action(Action::ToggleHelp, &mut FakeRunner { calls: 0 });

    // '1' would switch to Dashboard; '/' would enter filter mode. Neither must
    // fire — help just closes.
    app.on_action(Action::SwitchToAdapters, &mut FakeRunner { calls: 0 });
    assert!(!app.show_help, "any key closes help");
    assert_eq!(
        app.current_screen,
        Screen::Dashboard,
        "the key must not switch screens behind the overlay"
    );

    // Re-open and prove a filter-entering key is swallowed too.
    app.on_action(Action::ToggleHelp, &mut FakeRunner { calls: 0 });
    app.on_action(Action::InputChar('/'), &mut FakeRunner { calls: 0 });
    assert!(!app.show_help, "any key closes help");
    assert!(
        !app.filtering,
        "/ must not enter filter mode behind the overlay"
    );
}

#[test]
fn toggle_help_again_still_closes_it() {
    // `?` while help is open also just closes it (uniform "any key dismisses").
    let mut app = bare_app();
    app.on_action(Action::ToggleHelp, &mut FakeRunner { calls: 0 });
    assert!(app.show_help);
    app.on_action(Action::ToggleHelp, &mut FakeRunner { calls: 0 });
    assert!(!app.show_help, "? closes the help overlay");
}
