use super::*;

/// Enter filter mode the way a user does: press `/`. Asserts the slash itself
/// is the trigger and is NOT appended to the query.
fn enter_filter(app: &mut App, runner: &mut FakeRunner) {
    app.on_action(Action::InputChar('/'), runner);
    assert!(app.filtering, "/ must enter filter mode");
    assert_eq!(app.filter, "", "the '/' trigger must not land in the query");
}

#[test]
fn slash_enters_filter_mode_and_typing_drives_the_shared_filter() {
    // `/` enters filter mode; subsequent characters narrow the adapter view.
    let mut app = dashboard_app_with_adapters(&["bulwark", "scripts", "system"]);
    let mut runner = FakeRunner { calls: 0 };
    enter_filter(&mut app, &mut runner);
    for c in "bul".chars() {
        app.on_action(Action::InputChar(c), &mut runner);
    }
    assert_eq!(app.filter, "bul");
    assert_eq!(app.filtered_adapter_names(), vec!["bulwark".to_owned()]);
}

#[test]
fn filter_mode_captures_bound_command_letters_as_text() {
    // THE FIX: once in filter mode the screen runs in Text mode, so the keymap
    // delivers bound command letters (q, r, digits, j, k) as InputChar — they
    // must type into the filter, not fire quit/refresh/screen-switch/nav.
    let mut app = dashboard_app_with_adapters(&["queue7", "bulwark"]);
    let mut runner = FakeRunner { calls: 0 };
    enter_filter(&mut app, &mut runner);
    for c in "queue7".chars() {
        let quit = app.on_action(Action::InputChar(c), &mut runner);
        assert!(!quit, "no character may quit while filtering (got '{c}')");
    }
    assert_eq!(
        app.filter, "queue7",
        "every bound letter must type into the filter"
    );
    assert_eq!(app.filtered_adapter_names(), vec!["queue7".to_owned()]);
}

#[test]
fn enter_applies_the_filter_and_leaves_filter_mode() {
    let mut app = dashboard_app_with_adapters(&["bulwark", "scripts"]);
    let mut runner = FakeRunner { calls: 0 };
    enter_filter(&mut app, &mut runner);
    for c in "bul".chars() {
        app.on_action(Action::InputChar(c), &mut runner);
    }
    let quit = app.on_action(Action::Activate, &mut runner);
    assert!(!quit);
    assert!(!app.filtering, "Enter must exit filter mode");
    assert_eq!(app.filter, "bul", "Enter keeps the applied filter");
    assert_eq!(app.filtered_adapter_names(), vec!["bulwark".to_owned()]);
}

#[test]
fn esc_abandons_the_filter_and_does_not_quit() {
    let mut app = dashboard_app_with_adapters(&["bulwark", "scripts"]);
    let mut runner = FakeRunner { calls: 0 };
    enter_filter(&mut app, &mut runner);
    for c in "bul".chars() {
        app.on_action(Action::InputChar(c), &mut runner);
    }
    assert_eq!(app.filter, "bul");
    // Esc while filtering clears the query, exits filter mode, and does NOT quit.
    let quit = app.on_action(Action::Cancel, &mut runner);
    assert!(!quit, "esc must abandon the filter, not quit");
    assert!(!app.filtering, "esc must exit filter mode");
    assert!(app.filter.is_empty());
    assert_eq!(app.filtered_adapter_names().len(), 2);
}

#[test]
fn backspace_edits_the_filter_while_filtering() {
    let mut app = dashboard_app_with_adapters(&["bulwark", "scripts"]);
    let mut runner = FakeRunner { calls: 0 };
    enter_filter(&mut app, &mut runner);
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
fn typing_without_entering_filter_mode_does_not_filter() {
    // Outside filter mode, characters are NOT filter input. (In production they
    // are command bindings; here, driving InputChar directly, they must simply
    // not mutate the filter — only `/` opens it.)
    let mut app = dashboard_app_with_adapters(&["bulwark", "scripts"]);
    let mut runner = FakeRunner { calls: 0 };
    for c in "bul".chars() {
        app.on_action(Action::InputChar(c), &mut runner);
    }
    assert!(!app.filtering);
    assert!(
        app.filter.is_empty(),
        "typing must not filter until '/' is pressed"
    );
}

#[test]
fn slash_does_not_open_filter_on_a_non_filter_screen() {
    // System is not a filter screen: `/` must not enter filter mode there.
    let mut app = dashboard_app_with_adapters(&["bulwark", "scripts"]);
    app.current_screen = Screen::System;
    let mut runner = FakeRunner { calls: 0 };
    app.on_action(Action::InputChar('/'), &mut runner);
    assert!(!app.filtering, "/ must not filter off a filter screen");
    assert!(app.filter.is_empty());
}

#[test]
fn switching_screens_exits_filter_mode() {
    // Filtering is only valid on the screen it started on; leaving must reset it
    // so the keymap doesn't stay in Text mode on the new screen.
    let mut app = dashboard_app_with_adapters(&["bulwark", "scripts"]);
    let mut runner = FakeRunner { calls: 0 };
    enter_filter(&mut app, &mut runner);
    app.on_action(Action::SwitchToLauncher, &mut runner);
    assert!(!app.filtering, "switching screens must exit filter mode");
}

#[test]
fn opening_the_palette_exits_filter_mode() {
    let mut app = dashboard_app_with_adapters(&["bulwark", "scripts"]);
    let mut runner = FakeRunner { calls: 0 };
    enter_filter(&mut app, &mut runner);
    app.on_action(Action::OpenPalette, &mut runner);
    assert!(!app.filtering, "opening the palette must exit filter mode");
    assert!(app.palette_open);
}

#[test]
fn j_k_navigate_the_adapter_selection_on_the_adapters_screen() {
    // j/k move the adapter table selection on the Adapters screen. (As of Phase
    // C the cockpit/Dashboard's j/k move CARD focus instead — see
    // app::tests::cockpit — so the adapter-table movement is asserted here, on
    // the screen that owns it.) Names are stored sorted, so selection starts on
    // "alpha"; Down → "bravo", Up → back to "alpha".
    let mut app = dashboard_app_with_adapters(&["alpha", "bravo", "charlie"]);
    app.current_screen = Screen::Adapters;
    app.select_first_visible_adapter();
    assert_eq!(app.selected_adapter.as_deref(), Some("alpha"));

    let mut runner = FakeRunner { calls: 0 };
    app.on_action(Action::Down, &mut runner);
    assert_eq!(
        app.selected_adapter.as_deref(),
        Some("bravo"),
        "Down moves the adapter selection on the Adapters screen"
    );
    app.on_action(Action::Up, &mut runner);
    assert_eq!(
        app.selected_adapter.as_deref(),
        Some("alpha"),
        "Up moves back"
    );
}

#[test]
fn filtered_names_cache_tracks_filter_and_snapshot_changes() {
    // The render path reads a cached filtered list rather than recomputing per
    // frame; this guards that the cache stays coherent with its two inputs.
    let mut app = dashboard_app_with_adapters(&["bulwark", "scripts", "system"]);
    let mut runner = FakeRunner { calls: 0 };

    // Input 1 — the filter: applying "s" narrows the cache to s-matches.
    enter_filter(&mut app, &mut runner);
    app.on_action(Action::InputChar('s'), &mut runner);
    assert_eq!(
        app.filtered_adapter_names(),
        vec!["scripts".to_owned(), "system".to_owned()],
        "cache must reflect the active filter"
    );

    // Input 2 — a fresh snapshot arriving WHILE the filter is applied: the cache
    // must rebuild from the new adapter set, still honouring the filter. This is
    // the path that would silently go stale if apply_snapshot forgot to rebuild.
    app.apply_snapshot(snapshot_with_adapters(&["sentry", "bulwark", "saturn"]));
    assert_eq!(
        app.filter, "s",
        "the applied filter survives a refresh (sanity)"
    );
    assert_eq!(
        app.filtered_adapter_names(),
        vec!["saturn".to_owned(), "sentry".to_owned()],
        "cache must rebuild from the new snapshot, still filtered by 's'"
    );
}
