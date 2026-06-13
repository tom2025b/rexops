//! Esc is a "back out one level" key, never a quit. These guard the footgun
//! the old top-level fallback created: Esc returning quit meant Esc from the
//! Dashboard exited the app, and — since quitting kills a running job without a
//! confirm — Esc on the Jobs screen mid-job killed the job AND dropped the app
//! in one keystroke. Quit is `q` / Ctrl-C only.

use super::*;

#[test]
fn esc_at_top_level_is_a_no_op_not_a_quit() {
    // Dashboard, no filter, no modal: there is nothing to back out of. Esc must
    // NOT quit (it used to). It leaves the screen untouched and returns false.
    let mut app = bare_app();
    app.current_screen = Screen::Dashboard;
    let mut runner = FakeRunner { calls: 0 };

    let quit = app.on_action(Action::Cancel, &mut runner);

    assert!(
        !quit,
        "Esc at the top level must not quit — q / Ctrl-C does"
    );
    assert_eq!(
        app.current_screen,
        Screen::Dashboard,
        "Esc with nothing to cancel must not move the user off their screen"
    );
}

#[test]
fn esc_on_the_jobs_screen_with_a_running_job_neither_quits_nor_kills_it() {
    // The headline footgun: a live job present, user on the Jobs screen, presses
    // Esc. It must NOT quit (which would drop the app) and must leave the job
    // handle in place (quitting would kill it via Drop). Cancelling a job is `x`,
    // a separate explicit key — Esc here does nothing.
    let mut app = bare_app();
    app.current_screen = Screen::Jobs;
    // `sleep` won't exit during the test, so the handle is provably still live.
    app.jobs.job = Some(spawn("live-tool", "sleep", &[]).expect("spawn sleep"));
    let mut runner = FakeRunner { calls: 0 };

    let quit = app.on_action(Action::Cancel, &mut runner);

    assert!(!quit, "Esc mid-job must not quit the app");
    assert!(
        app.jobs.job.is_some(),
        "Esc must not cancel/kill the running job — that is `x`"
    );

    // Clean up the lingering child so the test leaves nothing behind.
    if let Some(job) = app.jobs.job.as_mut() {
        job.cancel();
    }
}

#[test]
fn esc_still_clears_an_applied_filter_before_reaching_the_top_level() {
    // The nested "back out" behaviour must survive: with a filter applied (not
    // actively typing), the FIRST Esc clears the filter (not a quit, not a
    // no-op), and only a SECOND Esc reaches the now-empty top level as a no-op.
    let mut app = dashboard_app_with_adapters(&["bulwark", "scripts"]);
    app.filter = "bul".to_owned();
    app.select_first_visible_adapter();
    let mut runner = FakeRunner { calls: 0 };

    let quit = app.on_action(Action::Cancel, &mut runner);
    assert!(!quit, "clearing a filter must not quit");
    assert!(app.filter.is_empty(), "first Esc clears the applied filter");

    let quit = app.on_action(Action::Cancel, &mut runner);
    assert!(
        !quit,
        "the follow-up top-level Esc is a no-op, still not a quit"
    );
}
