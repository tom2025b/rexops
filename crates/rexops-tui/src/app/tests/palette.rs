use super::*;

#[test]
fn palette_opens_filters_and_dispatches_navigation() {
    let mut app = launcher_app();
    let mut runner = FakeRunner { calls: 0 };

    app.on_action(Action::OpenPalette, &mut runner);
    assert!(app.palette_open, "^P must open the palette");

    // Type "system" → the list narrows to the System nav command at top.
    for c in "system".chars() {
        app.on_action(Action::InputChar(c), &mut runner);
    }
    assert!(
        app.palette_commands().iter().any(|c| c.label == "system"),
        "query should surface the system command"
    );

    // Enter dispatches the selected command (nav → switch screen) and closes.
    app.palette_selected = app
        .palette_commands()
        .iter()
        .position(|c| c.label == "system")
        .expect("system present");
    let quit = app.on_action(Action::Activate, &mut runner);

    assert!(!quit);
    assert!(!app.palette_open, "dispatch must close the palette");
    assert_eq!(
        app.current_screen,
        Screen::System,
        "nav command must switch"
    );
}

#[test]
fn palette_run_tool_arms_confirm_without_spawning() {
    // Choosing a `run <tool>` command in the palette must arm the SAME
    // confirm gate as the Launcher when the tool has a command — never spawn
    // directly.
    let mut app = launcher_app();
    app.config.adapters.insert(
        "scripts".to_owned(),
        rexops_core::AdapterConfig {
            enabled: true,
            binary: Some("/tmp/scripts".to_owned()),
            timeout_secs: None,
        },
    );
    let mut runner = FakeRunner { calls: 0 };

    app.on_action(Action::OpenPalette, &mut runner);
    for c in "run scripts".chars() {
        app.on_action(Action::InputChar(c), &mut runner);
    }
    let pos = app
        .palette_commands()
        .iter()
        .position(|c| c.label == "run scripts")
        .expect("run scripts present");
    app.palette_selected = pos;
    app.on_action(Action::Activate, &mut runner);

    assert!(!app.palette_open, "dispatch closes the palette");
    assert_eq!(
        app.pending_action,
        Some(PendingAction::RunJob {
            id: "scripts".to_owned(),
            name: "Scripts".to_owned(),
        }),
        "run command must arm a job behind the confirm gate"
    );
    assert_eq!(runner.calls, 0, "arming must not spawn");
    assert!(app.job.is_none(), "must not start a job before confirm");
}

#[test]
fn palette_run_disabled_tool_does_not_open_confirm() {
    let mut app = launcher_app();
    let mut runner = FakeRunner { calls: 0 };

    app.on_action(Action::OpenPalette, &mut runner);
    for c in "run scripts".chars() {
        app.on_action(Action::InputChar(c), &mut runner);
    }
    let pos = app
        .palette_commands()
        .iter()
        .position(|c| c.label == "run scripts")
        .expect("run scripts present");
    app.palette_selected = pos;
    app.on_action(Action::Activate, &mut runner);

    assert!(!app.palette_open, "dispatch closes the palette");
    assert!(
        app.pending_action.is_none(),
        "disabled palette run must not open the confirm modal"
    );
    assert_eq!(runner.calls, 0, "disabled command must not spawn");
    assert!(app
        .recent_events
        .iter()
        .any(|e| e == "Scripts: disabled (no launch command)"));
}

#[test]
fn palette_esc_closes_without_dispatching() {
    let mut app = launcher_app();
    let mut runner = FakeRunner { calls: 0 };
    let screen_before = app.current_screen;

    app.on_action(Action::OpenPalette, &mut runner);
    let quit = app.on_action(Action::Cancel, &mut runner);

    assert!(!quit, "Esc in the palette closes it, does not quit");
    assert!(!app.palette_open);
    assert_eq!(app.current_screen, screen_before, "nothing was dispatched");
}

#[test]
fn palette_does_not_open_while_confirm_pending() {
    // The confirm modal is the innermost gate: ^P must not open the palette
    // while an action awaits confirmation.
    let mut app = super::launcher::launcher_app_with_proto();
    let mut runner = FakeRunner { calls: 0 };

    app.on_action(Action::Activate, &mut runner); // arm (confirm pending)
    app.on_action(Action::OpenPalette, &mut runner); // should be swallowed

    assert!(
        !app.palette_open,
        "palette must not open over the confirm modal"
    );
    assert!(app.pending_action.is_some(), "pending must be untouched");
}
