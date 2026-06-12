use super::*;

fn select_tool(app: &mut App, id: &str) {
    let idx = CATALOG
        .iter()
        .position(|t| t.id == id)
        .unwrap_or_else(|| panic!("{id} in catalog"));
    app.selected_tool = idx;
}

/// A Launcher app with `proto` selected and pinned to an explicit binary.
/// `proto` is the INTERACTIVE tool, so arming it yields a foreground
/// `LaunchTool` that drives the (fake) `ForegroundRunner` on confirm — the
/// path these runner-based tests exercise.
pub(super) fn launcher_app_with_proto() -> App {
    let mut app = launcher_app();
    // modify_config refreshes the launch-availability cache for us — the cache
    // can't drift from config because config is only reachable through it.
    app.modify_config(|cfg| {
        cfg.adapters.insert(
            "proto".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/tmp/proto".to_owned()),
                timeout_secs: None,
            },
        );
    });
    select_tool(&mut app, "proto");
    app
}

#[test]
fn activate_on_launcher_arms_foreground_tool_without_spawning() {
    // Enter on the Launcher must only *arm* a pending action — never spawn
    // before the user confirms. `proto` is interactive → foreground LaunchTool.
    let mut app = launcher_app_with_proto();
    let mut runner = FakeRunner { calls: 0 };

    let quit = app.on_action(Action::Activate, &mut runner);

    assert!(!quit);
    assert_eq!(
        app.pending_action,
        Some(PendingAction::LaunchTool {
            id: "proto".to_owned(),
            name: "Proto".to_owned(),
        })
    );
    assert_eq!(runner.calls, 0, "arming must not spawn a process");
}

#[test]
fn activate_on_launcher_arms_streamable_tool_as_a_job() {
    // A non-interactive tool (scripts) must arm a RunJob — the background,
    // streamed path — rather than a foreground LaunchTool. The command is
    // pinned so this tests the enabled streamable path, not disabled UX.
    let mut app = launcher_app();
    app.modify_config(|cfg| {
        cfg.adapters.insert(
            "scripts".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/tmp/scripts".to_owned()),
                timeout_secs: None,
            },
        );
    });
    select_tool(&mut app, "scripts");
    let mut runner = FakeRunner { calls: 0 };

    app.on_action(Action::Activate, &mut runner);

    assert_eq!(
        app.pending_action,
        Some(PendingAction::RunJob {
            id: "scripts".to_owned(),
            name: "Scripts".to_owned(),
        }),
        "a streamable tool must arm a background job"
    );
    assert_eq!(runner.calls, 0, "arming must not spawn a process");
}

#[test]
fn activate_on_disabled_launcher_entry_does_not_open_confirm() {
    let mut app = launcher_app();
    select_tool(&mut app, "scripts");
    let mut runner = FakeRunner { calls: 0 };

    let quit = app.on_action(Action::Activate, &mut runner);

    assert!(!quit);
    assert_eq!(runner.calls, 0, "disabled rows must not spawn");
    assert!(
        app.pending_action.is_none(),
        "disabled rows must not open the confirm modal"
    );
    assert!(app
        .recent_events
        .iter()
        .any(|e| e == "Scripts: disabled (no launch command)"));
}

#[test]
fn confirm_runs_foreground_tool_and_clears_it() {
    // With a pending foreground launch, Enter confirms: it runs once via the
    // ForegroundRunner, requests refresh, and clears the pending action.
    let mut app = launcher_app_with_proto();
    let mut runner = FakeRunner { calls: 0 };

    app.on_action(Action::Activate, &mut runner); // arm
    let quit = app.on_action(Action::Activate, &mut runner); // confirm

    assert!(!quit);
    assert_eq!(runner.calls, 1, "confirm must run exactly once");
    assert!(app.pending_action.is_none(), "pending must be cleared");
    assert!(app.refreshing);
    assert!(app
        .recent_events
        .iter()
        .any(|e| e == "Proto exited successfully"));
}

#[test]
fn confirm_streamable_tool_does_not_use_foreground_runner() {
    // Confirming a RunJob goes through the background job path, NOT the
    // foreground runner. The pinned binary isn't a real executable, so the
    // spawn fails and is reported — but the runner must never be touched, and
    // no job handle is left dangling.
    let mut app = launcher_app();
    app.modify_config(|cfg| {
        cfg.adapters.insert(
            "scripts".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/tmp/definitely-not-executable".to_owned()),
                timeout_secs: None,
            },
        );
    });
    select_tool(&mut app, "scripts");
    let mut runner = FakeRunner { calls: 0 };

    app.on_action(Action::Activate, &mut runner); // arm RunJob
    let quit = app.on_action(Action::Activate, &mut runner); // confirm

    assert!(!quit);
    assert_eq!(runner.calls, 0, "a job must not use the foreground runner");
    assert!(app.pending_action.is_none(), "pending must be cleared");
    assert!(app.job.is_none(), "a failed spawn leaves no job handle");
    assert!(app
        .recent_events
        .iter()
        .any(|e| e.contains("failed to start")));
}

#[test]
fn cancel_discards_pending_action_without_spawning() {
    // Esc with a pending action cancels: nothing runs, pending is cleared,
    // and the app does not quit.
    let mut app = launcher_app_with_proto();
    let mut runner = FakeRunner { calls: 0 };

    app.on_action(Action::Activate, &mut runner); // arm
    let quit = app.on_action(Action::Cancel, &mut runner); // cancel

    assert!(!quit, "cancelling a pending action must not quit");
    assert_eq!(runner.calls, 0, "cancel must not spawn a process");
    assert!(app.pending_action.is_none(), "pending must be cleared");
    assert!(app
        .recent_events
        .iter()
        .any(|e| e.contains("cancelled (nothing ran)")));
}

#[test]
fn n_discards_pending_action_without_escape() {
    let mut app = launcher_app_with_proto();
    let mut runner = FakeRunner { calls: 0 };

    app.on_action(Action::Activate, &mut runner); // arm
    let quit = app.on_action(Action::InputChar('n'), &mut runner); // cancel

    assert!(!quit, "n must cancel a pending action");
    assert_eq!(runner.calls, 0, "cancel must not spawn a process");
    assert!(app.pending_action.is_none(), "pending must be cleared");
}

#[test]
fn y_confirms_pending_action_without_enter() {
    let mut app = launcher_app_with_proto();
    let mut runner = FakeRunner { calls: 0 };

    app.on_action(Action::Activate, &mut runner); // arm
    let quit = app.on_action(Action::InputChar('y'), &mut runner); // confirm

    assert!(!quit, "y must confirm a pending action");
    assert_eq!(runner.calls, 1, "confirm must run exactly once");
    assert!(app.pending_action.is_none(), "pending must be cleared");
}

#[test]
fn other_keys_are_swallowed_while_pending() {
    // The modal is modal: any non-confirm/cancel key while pending is
    // ignored. It must not navigate, must not spawn, and must leave the
    // pending action untouched.
    let mut app = launcher_app_with_proto();
    let mut runner = FakeRunner { calls: 0 };

    app.on_action(Action::Activate, &mut runner); // arm
    let before = app.selected_tool;
    let quit = app.on_action(Action::Down, &mut runner); // should be swallowed

    assert!(!quit);
    assert_eq!(runner.calls, 0, "swallowed key must not spawn");
    assert_eq!(app.selected_tool, before, "navigation must be blocked");
    assert!(
        app.pending_action.is_some(),
        "pending must survive a swallowed key"
    );
}

#[test]
fn preview_shows_resolved_command_or_no_command() {
    // The dry-run preview resolves the command without spawning. A pinned
    // binary shows "Will run: <path>"; a feed-only tool shows that nothing
    // would run.
    //
    // We pin an id that is NOT on PATH so the config-binary fallback is what
    // resolves — otherwise a real PATH hit on the dev box
    // would win and make the assertion environment-dependent (same reason
    // the launcher.rs tests use a fake id).
    let mut app = launcher_app();
    app.modify_config(|cfg| {
        cfg.adapters.insert(
            "definitely-not-a-real-tool-xyz".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/tmp/fake-tool".to_owned()),
                timeout_secs: None,
            },
        );
    });

    let launch = PendingAction::LaunchTool {
        id: "definitely-not-a-real-tool-xyz".to_owned(),
        name: "FakeTool".to_owned(),
    };
    assert_eq!(launch.preview(app.config()), "Will run:  /tmp/fake-tool");

    let feed_only = PendingAction::LaunchTool {
        // A different id that is never on PATH and has no config binary.
        id: "another-nonexistent-feed-tool-abc".to_owned(),
        name: "Workstate".to_owned(),
    };
    assert_eq!(
        feed_only.preview(app.config()),
        "No launch command yet (nothing will run)"
    );
}

#[test]
fn launcher_down_and_up_wrap_around_catalog() {
    let mut app = launcher_app();
    let mut runner = FakeRunner { calls: 0 };
    let last = CATALOG.len() - 1;

    // Down advances, then wraps from the last entry back to 0.
    app.on_action(Action::Down, &mut runner);
    assert_eq!(app.selected_tool, 1);
    for _ in 1..CATALOG.len() {
        app.on_action(Action::Down, &mut runner);
    }
    assert_eq!(app.selected_tool, 0, "Down must wrap past the end");

    // Up from 0 wraps to the last entry.
    app.on_action(Action::Up, &mut runner);
    assert_eq!(app.selected_tool, last, "Up must wrap before the start");
}

#[test]
fn launcher_esc_goes_back_to_dashboard_not_quit() {
    let mut app = launcher_app();
    let mut runner = FakeRunner { calls: 0 };

    let quit = app.on_action(Action::Cancel, &mut runner);

    assert!(!quit, "Esc on Launcher must not quit the app");
    assert_eq!(app.current_screen, Screen::Dashboard);
}

#[test]
fn launcher_enter_arms_the_selected_tool() {
    // Activate on the Launcher must arm a PendingAction for the *selected*
    // catalog tool, carrying that tool's id and name, and must not spawn.
    // `tools` is non-interactive → it arms a RunJob once a command exists.
    let mut app = launcher_app();
    app.modify_config(|cfg| {
        cfg.adapters.insert(
            "tools".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/tmp/tools".to_owned()),
                timeout_secs: None,
            },
        );
    });
    select_tool(&mut app, "tools");
    let entry = &CATALOG[app.selected_tool];
    let mut runner = FakeRunner { calls: 0 };

    app.on_action(Action::Activate, &mut runner);

    assert_eq!(
        app.pending_action,
        Some(PendingAction::RunJob {
            id: entry.id.to_owned(),
            name: entry.name.to_owned(),
        }),
        "Activate must arm the selected tool"
    );
    assert_eq!(runner.calls, 0, "arming must not spawn a process");
}

// --- command palette ----------------------------------------------------
