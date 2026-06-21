//! Cockpit interaction tests: focus movement, marker actuation, and the
//! confirm-gate handoff — driven through the real `App::on_action` path.
//!
//! This submodule is declared from `app/tests/mod.rs` (`mod cockpit;`) and gets
//! that module's shared imports + helpers via `use super::*` — including the
//! existing `FakeRunner` (a no-op `ForegroundRunner`), `App`, `Screen`, and
//! `Action`. We reuse `FakeRunner` rather than defining another runner.

use super::*; // App, Screen, Action, FakeRunner, mpsc, rexops_core::{AppConfig, OpsSnapshot}

use rexops_core::{AdapterConfig, AdapterHealth, ComponentStatus};

/// A fresh no-op runner. (`FakeRunner` from the parent module counts calls; we
/// only need a runner that succeeds, so either works — this names it locally.)
fn runner() -> FakeRunner {
    FakeRunner { calls: 0 }
}

fn comp(id: &str, name: &str, group: &str, maturity: &str, launchable: bool) -> ComponentStatus {
    ComponentStatus {
        id: id.into(),
        name: name.into(),
        group: group.into(),
        maturity: maturity.into(),
        health: AdapterHealth::Healthy,
        freshness: None,
        vital: None,
        launchable,
    }
}

/// An app on the cockpit with two cards: Workstate (brain, not launchable) and
/// Bulwark (field tool, launchable). Bulwark's launch command is forced
/// resolvable via the config-binary fallback so `arm_tool` opens the gate
/// without depending on a `bulwark` binary on the dev PATH.
fn cockpit_app() -> App {
    let (tx, _rx) = mpsc::channel();
    let mut cfg = AppConfig::default();
    cfg.adapters.insert(
        "bulwark".to_owned(),
        AdapterConfig {
            enabled: true,
            binary: Some("/bin/true".to_owned()),
            timeout_secs: None,
        },
    );
    let mut app = App::new(tx, cfg, None);
    let mut snap = OpsSnapshot::new();
    snap.push_component(comp("workstate", "Workstate", "brain", "live", false));
    snap.push_component(comp("bulwark", "Bulwark", "field tool", "live", true));
    app.apply_snapshot(snap);
    app
}

#[test]
fn snapshot_selects_the_first_card() {
    let app = cockpit_app();
    assert_eq!(app.selected_component.as_deref(), Some("workstate"));
}

#[test]
fn down_moves_focus_to_the_next_card_and_wraps() {
    let mut app = cockpit_app();
    let mut r = runner();
    app.on_action(Action::Down, &mut r);
    assert_eq!(app.selected_component.as_deref(), Some("bulwark"));
    app.on_action(Action::Down, &mut r); // wrap back to the first
    assert_eq!(app.selected_component.as_deref(), Some("workstate"));
}

#[test]
fn pressing_a_launchable_cards_letter_opens_the_confirm_gate() {
    let mut app = cockpit_app();
    let mut r = runner();
    // Bulwark is the 2nd visited card → marker 's'.
    app.on_action(Action::CardKey('s'), &mut r);
    assert!(
        app.pending_action.is_some(),
        "a launchable card's letter arms it"
    );
    assert_eq!(
        app.selected_component.as_deref(),
        Some("bulwark"),
        "and focuses it"
    );
}

#[test]
fn pressing_a_non_launchable_cards_letter_does_not_open_the_gate() {
    let mut app = cockpit_app();
    let mut r = runner();
    // Workstate (brain) is not launchable → 'a' must NOT open the confirm gate.
    app.on_action(Action::CardKey('a'), &mut r);
    assert!(
        app.pending_action.is_none(),
        "a non-launchable card cannot be armed"
    );
}

#[test]
fn enter_on_a_launchable_focused_card_opens_the_gate() {
    let mut app = cockpit_app();
    let mut r = runner();
    app.on_action(Action::Down, &mut r); // focus Bulwark
    app.on_action(Action::Activate, &mut r);
    assert!(
        app.pending_action.is_some(),
        "Enter arms the focused launchable card"
    );
}

#[test]
fn focus_survives_a_reordering_refresh() {
    let mut app = cockpit_app();
    let mut r = runner();
    app.on_action(Action::Down, &mut r); // focus Bulwark
                                         // A refresh arrives with the components in a different order.
    let mut snap = OpsSnapshot::new();
    snap.push_component(comp("bulwark", "Bulwark", "field tool", "live", true));
    snap.push_component(comp("workstate", "Workstate", "brain", "live", false));
    app.apply_snapshot(snap);
    assert_eq!(
        app.selected_component.as_deref(),
        Some("bulwark"),
        "focus tracks the id, not the slot"
    );
}

#[test]
fn focus_falls_back_when_the_focused_card_disappears() {
    let mut app = cockpit_app();
    let mut r = runner();
    app.on_action(Action::Down, &mut r); // focus Bulwark
                                         // Bulwark vanishes from the next snapshot.
    let mut snap = OpsSnapshot::new();
    snap.push_component(comp("workstate", "Workstate", "brain", "live", false));
    app.apply_snapshot(snap);
    assert_eq!(
        app.selected_component.as_deref(),
        Some("workstate"),
        "falls back to the first card"
    );
}
