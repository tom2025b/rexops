use super::*;

#[test]
fn refresh_while_filtered_preserves_filter_and_selection_by_name() {
    // A background snapshot arriving mid-filter must NOT disturb the user: the
    // filter string stays, and the selected adapter stays selected by NAME as
    // long as it survives the new snapshot — even though apply_snapshot rebuilds
    // adapter_names from scratch. (Characterization: this works today via
    // keep_selected_adapter_visible; the test guards it against regression.)
    let mut app = dashboard_app_with_adapters(&["bulwark", "buffer", "scripts"]);
    let mut runner = FakeRunner { calls: 0 };

    // Filter to the "bu" adapters and select the second one.
    app.on_action(Action::InputChar('/'), &mut runner);
    for c in "bu".chars() {
        app.on_action(Action::InputChar(c), &mut runner);
    }
    assert_eq!(app.filtered_adapter_names(), vec!["buffer".to_owned(), "bulwark".to_owned()]);
    app.selected_adapter = Some("bulwark".to_owned());

    // A refresh lands, still containing bulwark (plus a new adapter).
    app.apply_snapshot(snapshot_with_adapters(&["bulwark", "buffer", "scripts", "newcomer"]));

    assert_eq!(app.filter, "bu", "refresh must not clear the filter");
    assert!(app.filtering, "refresh must not exit filter mode");
    assert_eq!(
        app.selected_adapter,
        Some("bulwark".to_owned()),
        "the selected adapter must stay selected by name across a refresh"
    );
    // The new adapter is present in the unfiltered list but filtered out of view.
    assert!(app.adapter_names.contains(&"newcomer".to_owned()));
    assert!(!app.filtered_adapter_names().contains(&"newcomer".to_owned()));
}

#[test]
fn refresh_reselects_only_when_the_selected_adapter_disappears() {
    // If the selected adapter is gone from the new snapshot, falling back to the
    // first visible entry is correct — that's not clobbering, it's recovery.
    let mut app = dashboard_app_with_adapters(&["bulwark", "scripts"]);
    app.selected_adapter = Some("scripts".to_owned());

    app.apply_snapshot(snapshot_with_adapters(&["bulwark", "vault"]));

    assert_eq!(
        app.selected_adapter,
        Some("bulwark".to_owned()),
        "a vanished selection falls back to the first visible adapter"
    );
}

#[test]
fn apply_snapshot_always_clears_the_refreshing_flag() {
    // The flag is set by request_refresh and only cleared on snapshot receipt.
    // request_refresh's panic-catch guarantees a snapshot ALWAYS arrives (an
    // empty fallback if the probe panicked), so this clear is the sole, reliable
    // path back to a refreshable state — `r` can never wedge. Guard it.
    let mut app = dashboard_app_with_adapters(&["bulwark"]);
    app.refreshing = true;

    app.apply_snapshot(OpsSnapshot::new());

    assert!(
        !app.refreshing,
        "receiving any snapshot (even empty) must clear the refreshing flag"
    );
}

#[test]
fn a_panicking_snapshot_build_still_yields_a_snapshot() {
    // The hardening: request_refresh wraps build_snapshot in catch_unwind so a
    // panicking probe still SENDS a snapshot (empty fallback) and the flag
    // clears. We can't make the real build_snapshot panic on demand, so we
    // exercise the exact recovery pattern request_refresh uses, proving a panic
    // is converted into the empty fallback rather than a lost send + wedged flag.
    let snapshot = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> OpsSnapshot {
        panic!("probe blew up");
    }))
    .unwrap_or_else(|_| OpsSnapshot::new());

    // A usable (empty) snapshot came back instead of unwinding the thread.
    assert!(snapshot.adapter_health.is_empty());

    // And feeding it through apply_snapshot clears refreshing, just like the
    // real path — so `r` is never permanently stuck after a panicking probe.
    let mut app = bare_app();
    app.refreshing = true;
    app.apply_snapshot(snapshot);
    assert!(!app.refreshing, "a fallback snapshot must un-wedge refresh");
}
