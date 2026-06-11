use super::*;

#[test]
fn typing_on_the_dashboard_drives_the_shared_filter() {
    // Before this change, InputChar was a no-op off the Adapters screen. Now
    // the Dashboard takes filter input too, narrowing the adapter view.
    let mut app = dashboard_app_with_adapters(&["bulwark", "scripts", "system"]);
    let mut runner = FakeRunner { calls: 0 };
    for c in "bul".chars() {
        app.on_action(Action::InputChar(c), &mut runner);
    }
    assert_eq!(app.filter, "bul");
    assert_eq!(app.filtered_adapter_names(), vec!["bulwark".to_owned()]);
}

#[test]
fn esc_clears_the_dashboard_filter_without_quitting() {
    let mut app = dashboard_app_with_adapters(&["bulwark", "scripts"]);
    let mut runner = FakeRunner { calls: 0 };
    for c in "bul".chars() {
        app.on_action(Action::InputChar(c), &mut runner);
    }
    assert_eq!(app.filter, "bul");
    // Esc with a non-empty filter clears it and does NOT request quit.
    let quit = app.on_action(Action::Cancel, &mut runner);
    assert!(
        !quit,
        "esc must clear the filter, not quit, while filtering"
    );
    assert!(app.filter.is_empty());
    assert_eq!(app.filtered_adapter_names().len(), 2);
}

#[test]
fn backspace_edits_the_dashboard_filter() {
    let mut app = dashboard_app_with_adapters(&["bulwark", "scripts"]);
    let mut runner = FakeRunner { calls: 0 };
    for c in "bulx".chars() {
        app.on_action(Action::InputChar(c), &mut runner);
    }
    assert_eq!(app.filter, "bulx");
    assert!(
        app.filtered_adapter_names().is_empty(),
        "'bulx' matches nothing"
    );
    app.on_action(Action::Backspace, &mut runner);
    assert_eq!(app.filter, "bul");
    assert_eq!(app.filtered_adapter_names(), vec!["bulwark".to_owned()]);
}

#[test]
fn filter_typing_is_inert_on_a_non_filter_screen() {
    // System is not a filter screen, so characters there must NOT mutate the
    // shared filter (they stay available for that screen's own bindings).
    let mut app = dashboard_app_with_adapters(&["bulwark", "scripts"]);
    app.current_screen = Screen::System;
    let mut runner = FakeRunner { calls: 0 };
    for c in "bul".chars() {
        app.on_action(Action::InputChar(c), &mut runner);
    }
    assert!(app.filter.is_empty(), "typing on System must not filter");
}
