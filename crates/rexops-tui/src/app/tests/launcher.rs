use super::*;

fn select_tool(app: &mut App, id: &str) {
    let idx = crate::tools::launchable()
        .iter()
        .position(|t| t.id == id)
        .unwrap_or_else(|| panic!("{id} in catalog"));
    app.selected_tool = idx;
}

/// A Launcher app with `bulwark` selected and pinned to an explicit binary.
/// `bulwark` is a foreground tool, so arming it yields a `LaunchTool` that
/// drives the fake `ForegroundRunner` on confirm.
pub(super) fn launcher_app_with_proto() -> App {
    let mut app = launcher_app();
    // modify_config refreshes the launch-availability cache for us — the cache
    // can't drift from config because config is only reachable through it.
    app.modify_config(|cfg| {
        cfg.adapters.insert(
            "bulwark".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/tmp/bulwark".to_owned()),
                timeout_secs: None,
                ..Default::default()
            },
        );
    });
    select_tool(&mut app, "bulwark");
    app
}

#[test]
fn activate_on_launcher_arms_foreground_tool_without_spawning() {
    // Enter on the Launcher must only *arm* a pending action — never spawn
    // before the user confirms. `bulwark` is foreground → LaunchTool.
    let mut app = launcher_app_with_proto();
    let mut runner = FakeRunner { calls: 0 };

    let quit = app.on_action(Action::Activate, &mut runner);

    assert!(!quit);
    assert_eq!(
        app.pending_action,
        Some(PendingAction::LaunchTool {
            id: "bulwark".to_owned(),
            name: "Bulwark".to_owned(),
        })
    );
    assert_eq!(runner.calls, 0, "arming must not spawn a process");
}

#[test]
fn activate_on_launcher_arms_proto_as_a_foreground_tool() {
    // Proto's bare command owns the protocol picker and needs the real terminal,
    // so RexOps must arm it as a foreground LaunchTool, not a background RunJob.
    // The command is pinned so this tests the enabled launch path, not disabled UX.
    let mut app = launcher_app();
    app.modify_config(|cfg| {
        cfg.adapters.insert(
            "proto".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/tmp/proto".to_owned()),
                timeout_secs: None,
                ..Default::default()
            },
        );
    });
    select_tool(&mut app, "proto");
    let mut runner = FakeRunner { calls: 0 };

    app.on_action(Action::Activate, &mut runner);

    assert_eq!(
        app.pending_action,
        Some(PendingAction::LaunchTool {
            id: "proto".to_owned(),
            name: "Proto".to_owned(),
        }),
        "Proto must run in the foreground so its picker has a TTY"
    );
    assert_eq!(runner.calls, 0, "arming must not spawn a process");
}

#[test]
fn activate_on_disabled_launcher_entry_does_not_open_confirm() {
    // A catalog tool with no resolvable command is "disabled": Enter must not
    // spawn or open the confirm modal. Administratively disabling proto's
    // adapter (`enabled: false`) guarantees it never resolves — independent of
    // whether a `proto` binary is on the dev PATH (a disabled adapter is refused
    // by resolve_command even when its binary exists).
    let mut app = launcher_app();
    app.modify_config(|cfg| {
        cfg.adapters.insert(
            "proto".to_owned(),
            rexops_core::AdapterConfig {
                enabled: false,
                binary: None,
                timeout_secs: None,
                ..Default::default()
            },
        );
    });
    select_tool(&mut app, "proto");
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
        .any(|e| e == "Proto: disabled (no launch command)"));
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
        .any(|e| e == "Bulwark exited successfully"));
}

#[test]
fn confirm_proto_uses_the_foreground_runner() {
    // Confirming Proto must hand the terminal to bare `proto`, not stream a
    // non-interactive `proto run` job with stdin nulled.
    let mut app = launcher_app();
    app.modify_config(|cfg| {
        cfg.adapters.insert(
            "proto".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/tmp/proto".to_owned()),
                timeout_secs: None,
                ..Default::default()
            },
        );
    });
    select_tool(&mut app, "proto");
    let mut runner = FakeRunner { calls: 0 };

    app.on_action(Action::Activate, &mut runner); // arm LaunchTool
    let quit = app.on_action(Action::Activate, &mut runner); // confirm

    assert!(!quit);
    assert_eq!(runner.calls, 1, "Proto must use the foreground runner");
    assert!(app.pending_action.is_none(), "pending must be cleared");
    assert!(app
        .recent_events
        .iter()
        .any(|e| e == "Proto exited successfully"));
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
                ..Default::default()
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
    let last = crate::tools::launchable().len() - 1;

    // Down advances, then wraps from the last entry back to 0.
    app.on_action(Action::Down, &mut runner);
    assert_eq!(app.selected_tool, 1);
    for _ in 1..crate::tools::launchable().len() {
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
    // Proto is interactive and must arm a foreground LaunchTool.
    let mut app = launcher_app();
    app.modify_config(|cfg| {
        cfg.adapters.insert(
            "proto".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/tmp/proto".to_owned()),
                timeout_secs: None,
                ..Default::default()
            },
        );
    });
    select_tool(&mut app, "proto");
    let launchable = crate::tools::launchable();
    let entry = launchable[app.selected_tool];
    let mut runner = FakeRunner { calls: 0 };

    app.on_action(Action::Activate, &mut runner);

    assert_eq!(
        app.pending_action,
        Some(PendingAction::LaunchTool {
            id: entry.id.to_owned(),
            name: entry.name.to_owned(),
        }),
        "Activate must arm the selected tool"
    );
    assert_eq!(runner.calls, 0, "arming must not spawn a process");
}

#[test]
fn is_tool_available_requires_resolvable_and_not_unavailable() {
    use rexops_core::AdapterHealth;
    let mut app = launcher_app();
    // Make `proto` resolvable via the cache, holding config+PATH constant so the
    // test isolates the health contribution.
    app.set_tool_launchable("proto", true);
    let proto = rexops_core::AdapterId::new("proto").expect("test adapter id");

    app.snapshot
        .set_adapter_health(&proto, AdapterHealth::Unavailable);
    assert!(
        !app.is_tool_available("proto"),
        "resolvable + Unavailable must be unavailable"
    );

    for h in [
        AdapterHealth::Healthy,
        AdapterHealth::Degraded,
        AdapterHealth::Unknown,
    ] {
        app.snapshot.set_adapter_health(&proto, h);
        assert!(
            app.is_tool_available("proto"),
            "resolvable + {h:?} must stay available"
        );
    }

    // Not resolvable → never available, regardless of health.
    app.set_tool_launchable("proto", false);
    app.snapshot
        .set_adapter_health(&proto, AdapterHealth::Healthy);
    assert!(
        !app.is_tool_available("proto"),
        "an unresolvable tool is never available even when Healthy"
    );
}

#[test]
fn arm_tool_refuses_an_unavailable_tool_and_opens_no_confirm() {
    use rexops_core::AdapterHealth;
    // A tool can resolve (pinned binary) yet have its adapter report Unavailable.
    // Arming it must NOT open the confirm gate — matching the launcher's
    // "· unavailable" tag — and must log why.
    let mut app = launcher_app();
    app.modify_config(|cfg| {
        cfg.adapters.insert(
            "proto".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/tmp/proto".to_owned()),
                timeout_secs: None,
                ..Default::default()
            },
        );
    });
    let proto = rexops_core::AdapterId::new("proto").expect("test adapter id");
    app.snapshot
        .set_adapter_health(&proto, AdapterHealth::Unavailable);

    app.arm_tool("proto".to_owned(), "Proto".to_owned());

    assert!(
        app.pending_action.is_none(),
        "an Unavailable tool must not open the confirm gate"
    );
    assert!(
        app.recent_events.iter().any(|e| e.contains("unavailable")),
        "arming an Unavailable tool must log why"
    );
}

#[test]
fn start_job_reports_a_spawn_failure_and_leaves_clean_state() {
    // The spawn-failure branch of start_job (manager.rs) was only ever hit
    // incidentally by a foreground-runner test. Cover it directly: a tool whose
    // command resolves (pinned config binary) but cannot exec (path is not an
    // executable) must log "failed to start", leave NO job handle, and not
    // switch to the Jobs screen — and the app must remain able to start a job
    // afterwards (the failure left no half-state).
    let mut app = launcher_app();
    app.modify_config(|cfg| {
        cfg.adapters.insert(
            "scripts".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/nonexistent/definitely-not-a-binary".to_owned()),
                timeout_secs: None,
                ..Default::default()
            },
        );
    });
    let screen_before = app.current_screen;

    // Drive the manager entry point the audit flagged, not the UI action, so
    // this test pins the failure handling regardless of how launches are armed.
    app.start_job("scripts", "Scripts");

    assert!(
        app.job.is_none(),
        "a failed spawn must leave no dangling job handle"
    );
    assert_eq!(
        app.current_screen, screen_before,
        "a failed spawn must not switch to the Jobs screen"
    );
    assert!(
        app.recent_events
            .iter()
            .any(|e| e.contains("Scripts: failed to start")),
        "the spawn failure must be reported to the user: {:?}",
        app.recent_events
    );

    // The failed start left clean state: starting another job still works. Pin
    // `true` (a real, always-present executable) and confirm a handle appears.
    app.modify_config(|cfg| {
        cfg.adapters
            .get_mut("scripts")
            .expect("scripts adapter")
            .binary = Some("true".to_owned());
    });
    app.start_job("scripts", "Scripts");
    assert!(
        app.job.is_some(),
        "after a failed spawn the app must still be able to start a job"
    );
}

// --- command palette ----------------------------------------------------

#[test]
fn launcher_list_is_exactly_the_registry_launchable_set() {
    // The Phase D invariant: there is ONE launch source. The Launcher's list must
    // equal the registry components with a LaunchSpec, in registry order — if a
    // future row gains/loses a launch, the screen follows with no second list to
    // update.
    let screen: Vec<&str> = crate::tools::launchable().iter().map(|c| c.id).collect();
    let registry: Vec<&str> = rexops_core::COMPONENTS
        .iter()
        .filter(|c| c.launch.is_some())
        .map(|c| c.id)
        .collect();
    assert_eq!(
        screen, registry,
        "Launcher list must equal the registry launch set"
    );
    // And the two Phase D promotions are present.
    assert!(screen.contains(&"scriptvault"));
    assert!(screen.contains(&"toolfoundry"));
}
