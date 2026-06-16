use super::*;

fn run_job_to_completion(app: &mut App, name: &str, command: &str) {
    app.job = Some(spawn(name, name, command, &[]).expect("spawn test job"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.job.is_some() {
        app.poll_job();
        assert!(
            std::time::Instant::now() < deadline,
            "job did not finish in time"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn finishing_a_job_records_history_and_flashes_a_toast() {
    use suite_ui::ToastKind;

    let mut app = bare_app();
    assert!(app.job_history.is_empty());

    // A clean exit → one Success history entry + a Success toast.
    run_job_to_completion(&mut app, "true", "true");
    assert_eq!(app.job_history.len(), 1, "a finished job is recorded");
    let rec = &app.job_history[0];
    assert_eq!(rec.name, "true");
    assert!(rec.outcome.ok && !rec.outcome.cancelled);
    assert!(matches!(app.toast, Some((_, ToastKind::Success))));

    // A non-zero exit → a second entry + a Failure toast.
    run_job_to_completion(&mut app, "false", "false");
    assert_eq!(app.job_history.len(), 2, "history accumulates");
    let rec = &app.job_history[1];
    assert!(!rec.outcome.ok && !rec.outcome.cancelled);
    assert!(matches!(app.toast, Some((_, ToastKind::Failure))));
}

#[test]
fn history_is_capped_and_rolls_off_oldest_first() {
    let mut app = bare_app();
    // Pre-fill at the cap with sentinel records, then push one more via a real
    // finished job; the oldest must roll off and the newest land at the end.
    for i in 0..JOB_HISTORY_CAP {
        app.job_history.push_back(JobRecord {
            name: format!("old-{i}"),
            outcome: LastOutcome {
                name: format!("old-{i}"),
                ok: true,
                cancelled: false,
            },
            summary: format!("old-{i}: finished (exit 0)"),
        });
    }
    run_job_to_completion(&mut app, "newest", "true");
    assert_eq!(
        app.job_history.len(),
        JOB_HISTORY_CAP,
        "history stays capped"
    );
    assert_eq!(
        app.job_history.front().unwrap().name,
        "old-1",
        "the oldest entry rolled off"
    );
    assert_eq!(
        app.job_history.back().unwrap().name,
        "newest",
        "the new entry is appended last"
    );
}

#[test]
fn job_output_is_a_rolling_buffer_capped_at_job_output_cap() {
    let mut app = bare_app();
    // Push well past the cap; the buffer must stay bounded and keep the
    // newest lines (the oldest roll off the front).
    for i in 0..(JOB_OUTPUT_CAP + 250) {
        app.push_job_output(JobOutput::Stdout(format!("line-{i}")));
    }
    assert_eq!(app.job_output.len(), JOB_OUTPUT_CAP, "buffer stays capped");
    assert_eq!(
        app.job_output.front(),
        Some(&JobOutput::Stdout("line-250".to_owned())),
        "the oldest retained line is exactly cap-from-the-end"
    );
    assert_eq!(
        app.job_output.back(),
        Some(&JobOutput::Stdout(format!("line-{}", JOB_OUTPUT_CAP + 249))),
        "the newest line is kept at the back"
    );
}

#[test]
fn any_key_dismisses_a_lingering_toast() {
    let mut app = bare_app();
    let mut runner = FakeRunner { calls: 0 };
    app.toast = Some(("backup — done".to_owned(), suite_ui::ToastKind::Success));
    // A harmless key (refresh) goes through `on_action`, which clears the toast
    // up front regardless of what the action itself does.
    app.on_action(Action::Refresh, &mut runner);
    assert!(app.toast.is_none(), "any key must dismiss the toast");
}

#[test]
fn toast_for_maps_each_outcome_to_its_kind() {
    use suite_ui::ToastKind;
    let ok = LastOutcome {
        name: "j".into(),
        ok: true,
        cancelled: false,
    };
    let fail = LastOutcome {
        name: "j".into(),
        ok: false,
        cancelled: false,
    };
    let cancelled = LastOutcome {
        name: "j".into(),
        ok: false,
        cancelled: true,
    };
    assert!(matches!(toast_for(&ok), (_, ToastKind::Success)));
    assert!(matches!(toast_for(&fail), (_, ToastKind::Failure)));
    assert!(matches!(toast_for(&cancelled), (_, ToastKind::Cancelled)));
    // Cancelled takes precedence over `ok` (a cancel can race a clean exit).
    let cancelled_but_ok = LastOutcome {
        name: "j".into(),
        ok: true,
        cancelled: true,
    };
    assert!(matches!(
        toast_for(&cancelled_but_ok),
        (_, ToastKind::Cancelled)
    ));
}

#[test]
fn job_state_maps_outcome_to_the_shared_status_enum() {
    use suite_ui::JobState;

    // Fresh app, no job ever run → Idle.
    let mut app = bare_app();
    assert_eq!(app.job_state(), JobState::Idle);

    // A clean finish → Done { ok: true }.
    app.last_outcome = Some(LastOutcome {
        name: "backup".to_owned(),
        ok: true,
        cancelled: false,
    });
    assert_eq!(
        app.job_state(),
        JobState::Done {
            name: "backup",
            ok: true
        }
    );

    // A non-zero exit → Done { ok: false }.
    app.last_outcome = Some(LastOutcome {
        name: "rescan".to_owned(),
        ok: false,
        cancelled: false,
    });
    assert_eq!(
        app.job_state(),
        JobState::Done {
            name: "rescan",
            ok: false
        }
    );

    // A cancel/signal → Cancelled, regardless of `ok`.
    app.last_outcome = Some(LastOutcome {
        name: "deploy".to_owned(),
        ok: false,
        cancelled: true,
    });
    assert_eq!(app.job_state(), JobState::Cancelled { name: "deploy" });
}

#[test]
fn a_live_job_outranks_the_last_outcome_in_the_status_bar() {
    use suite_ui::JobState;

    // `job_state` reports Running whenever a job handle is present, regardless
    // of any recorded last outcome. Spawning `sleep` (which won't exit during
    // the test) gives a present handle deterministically; we kill it after the
    // assertion so the test leaves no lingering process behind.
    let mut app = bare_app();
    app.last_outcome = Some(LastOutcome {
        name: "old".to_owned(),
        ok: true,
        cancelled: false,
    });
    app.job = Some(spawn("live-tool", "live-tool", "sleep", &[]).expect("spawn sleep"));
    assert_eq!(app.job_state(), JobState::Running { name: "live-tool" });
    if let Some(job) = app.job.as_mut() {
        job.cancel();
    }
}

// --- Jobs output scrollback (Option A: simple from-bottom offset, auto-follow) ---

fn seed_output(app: &mut App, n: usize) {
    for i in 0..n {
        app.push_job_output(JobOutput::Stdout(format!("line-{i}")));
    }
}

#[test]
fn scroll_starts_at_bottom_and_down_stays_clamped() {
    let mut app = bare_app();
    seed_output(&mut app, 50);
    assert_eq!(app.jobs_scroll, 0, "default follows the bottom");
    // Down (newer) at the bottom is a no-op, never underflows.
    app.scroll_jobs_output(false);
    assert_eq!(app.jobs_scroll, 0);
}

#[test]
fn scroll_up_clamps_to_the_buffer() {
    let mut app = bare_app();
    seed_output(&mut app, 5);
    // Scroll up far more than the buffer; it must clamp to len-1, never beyond.
    for _ in 0..100 {
        app.scroll_jobs_output(true);
    }
    assert_eq!(app.jobs_scroll, 4, "up clamps at len-1 (5 lines)");
    // And coming back down returns to the bottom.
    for _ in 0..100 {
        app.scroll_jobs_output(false);
    }
    assert_eq!(app.jobs_scroll, 0);
}

#[test]
fn starting_a_job_resets_scroll_to_the_bottom() {
    let mut app = bare_app();
    seed_output(&mut app, 20);
    app.scroll_jobs_output(true);
    app.scroll_jobs_output(true);
    assert_eq!(app.jobs_scroll, 2);
    // `false` is not a real binary, so start_job fails to spawn — but it still
    // runs its reset bookkeeping (clears output, resets scroll) first. Use a real
    // no-op binary so the spawn succeeds and the reset path is exercised cleanly.
    app.start_job("true", "true");
    assert_eq!(app.jobs_scroll, 0, "a fresh job follows the bottom again");
    if let Some(job) = app.job.as_mut() {
        job.cancel();
    }
}

#[test]
fn up_down_scroll_the_jobs_screen_via_on_action() {
    let mut app = bare_app();
    app.current_screen = Screen::Jobs;
    seed_output(&mut app, 30);
    let mut runner = FakeRunner { calls: 0 };

    // Up = older → offset grows; Down = newer → offset shrinks.
    app.on_action(Action::Up, &mut runner);
    app.on_action(Action::Up, &mut runner);
    assert_eq!(app.jobs_scroll, 2, "Up scrolls toward older output");
    app.on_action(Action::Down, &mut runner);
    assert_eq!(app.jobs_scroll, 1, "Down scrolls back toward newest");
}

#[test]
fn poll_job_reports_no_change_when_idle() {
    // The dirty-flag redraw loop relies on this: with no running job, a tick
    // must report "nothing changed" so the runtime skips the repaint. A loop
    // that always redrew would mask a regression here, so assert it directly.
    let mut app = bare_app();
    assert!(app.job.is_none());
    assert!(!app.poll_job(), "an idle tick must not request a repaint");
}

#[test]
fn poll_job_reports_change_when_a_job_finishes() {
    // The tick that drains the last output and reaps the child must return true
    // (header → idle, a history row + toast appear). `false` exits immediately
    // with no output; we drive ticks until the job clears, and the final tick —
    // the one that observed completion — must have reported a change.
    let mut app = bare_app();
    app.job = Some(spawn("false", "false", "false", &[]).expect("spawn test job"));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut last = false;
    while app.job.is_some() {
        last = app.poll_job();
        assert!(
            std::time::Instant::now() < deadline,
            "job did not finish in time"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        last,
        "the tick that finished the job must request a repaint"
    );
    assert_eq!(app.job_history.len(), 1, "completion was recorded");
}
