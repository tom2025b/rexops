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
            ..Default::default()
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

#[test]
fn drill_and_back_round_trip() {
    let mut app = cockpit_app();
    let mut r = runner();
    // Focus Workstate (not launchable) → Enter drills into detail.
    app.on_action(Action::Activate, &mut r);
    assert_eq!(app.current_screen, Screen::CockpitDetail);
    // Esc backs out to the cockpit, focus preserved.
    app.on_action(Action::Cancel, &mut r);
    assert_eq!(app.current_screen, Screen::Dashboard);
    assert_eq!(app.selected_component.as_deref(), Some("workstate"));
}

#[test]
fn drill_key_opens_detail_for_a_launchable_card_too() {
    let mut app = cockpit_app();
    let mut r = runner();
    app.on_action(Action::Down, &mut r); // focus Bulwark (launchable)
    app.on_action(Action::Drill, &mut r); // `g` drills even though it's launchable
    assert_eq!(app.current_screen, Screen::CockpitDetail);
    assert_eq!(app.selected_component.as_deref(), Some("bulwark"));
}

#[test]
fn every_flippable_tool_is_live_launchable_and_no_row_stays_planned() {
    let launchable: Vec<&str> = rexops_core::launchable_components()
        .iter()
        .map(|c| c.id)
        .collect();
    assert!(
        launchable.contains(&"pulse"),
        "pulse must be launchable: {launchable:?}"
    );

    // All four Probe+launch tools are launchable. With rex-forge flipped, the
    // black-box and factory rows are wired and nothing remains Planned.
    for id in ["rex-check", "tripwire", "rewind", "rex-forge"] {
        assert!(
            launchable.contains(&id),
            "{id} must be launchable: {launchable:?}"
        );
    }

    // The registry has reached the Live ceiling: no row is Planned anymore.
    assert!(
        rexops_core::COMPONENTS
            .iter()
            .all(|c| c.maturity != rexops_core::Maturity::Planned),
        "no registry row may stay Planned once every flippable tool is Live"
    );

    // Pulse's health source is StatusCommand and its maturity is Live.
    let pulse = rexops_core::component_by_id("pulse").unwrap();
    assert!(matches!(
        pulse.health,
        rexops_core::HealthSource::StatusCommand { .. }
    ));
    assert_eq!(pulse.maturity, rexops_core::Maturity::Live);

    // The four Probe+launch tools are Live via Probe (binary presence), not StatusCommand.
    // The four Probe+launch tools are Live via Probe (binary presence), not
    // StatusCommand. Pin the FULL contract per tool so a regression in any field
    // trips: maturity Live, a Probe source whose binary matches the id and whose
    // version_args are exactly `--help` (the presence check the snapshot builder
    // spawns — silently emptying these would break health detection), and a
    // Foreground LaunchSpec.
    for id in ["rex-check", "tripwire", "rewind", "rex-forge"] {
        let c = rexops_core::component_by_id(id).unwrap();
        assert_eq!(c.maturity, rexops_core::Maturity::Live, "{id} must be Live");
        match c.health {
            rexops_core::HealthSource::Probe {
                binary,
                version_args,
            } => {
                assert_eq!(binary, id, "{id} probe binary must be the tool id");
                assert_eq!(
                    version_args,
                    &["--help"],
                    "{id} probe must use --help for presence"
                );
            }
            other => panic!("{id} health must be Probe, got {other:?}"),
        }
        let launch = c
            .launch
            .unwrap_or_else(|| panic!("{id} must have a LaunchSpec"));
        assert_eq!(
            launch.run_mode,
            rexops_core::RunMode::Foreground,
            "{id} must launch in the foreground"
        );
    }
}

#[test]
fn probe_tool_card_gates_launch_on_resolved_health() {
    // ITEM-3 launch-behavior guard at the cockpit level: a Probe tool's card is
    // armable only when its resolved health is not Unavailable. Build two cards —
    // a Healthy tripwire (launchable) and an Unavailable rewind (blocked) — with
    // both launch commands forced resolvable via a configured binary, so
    // launchability turns purely on health. This proves the registry walk's
    // `health != Unavailable` gate reaches the UI's `is_tool_available`.
    let (tx, _rx) = mpsc::channel();
    let mut cfg = AppConfig::default();
    for id in ["tripwire", "rewind"] {
        cfg.adapters.insert(
            id.to_owned(),
            AdapterConfig {
                enabled: true,
                binary: Some(format!("/tmp/{id}")),
                timeout_secs: None,
                ..Default::default()
            },
        );
    }
    let mut app = App::new(tx, cfg, None);

    let mut snap = OpsSnapshot::new();
    snap.push_component(comp("tripwire", "Tripwire", "black box", "live", true));
    snap.push_component(ComponentStatus {
        id: "rewind".into(),
        name: "Rewind".into(),
        group: "black box".into(),
        maturity: "live".into(),
        health: AdapterHealth::Unavailable,
        freshness: None,
        vital: Some("not installed".into()),
        launchable: false,
    });
    let tw = rexops_core::AdapterId::new("tripwire").unwrap();
    let rw = rexops_core::AdapterId::new("rewind").unwrap();
    snap.set_adapter_health(&tw, AdapterHealth::Healthy);
    snap.set_adapter_health(&rw, AdapterHealth::Unavailable);
    app.apply_snapshot(snap);

    assert!(
        app.is_tool_available("tripwire"),
        "a Healthy Probe tool must be available to launch"
    );
    assert!(
        !app.is_tool_available("rewind"),
        "an Unavailable Probe tool must be blocked from launch"
    );
}

// Learning Notes
// - The guard test locks the three-field flip (health, launch, maturity) as a
//   permanent invariant: CI will catch any accidental rollback to Planned, an
//   emptied version_args, or a tool dropping out of the launchable set.
// - probe_tool_card_gates_launch_on_resolved_health proves the launch GATE: a
//   Probe tool follows its resolved health (Healthy launches, Unavailable is
//   blocked) — the behavioral half of the Probe-wiring fix, at the UI boundary.
