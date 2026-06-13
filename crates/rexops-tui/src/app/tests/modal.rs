use super::*;
use crate::app::Modal;
use crate::commands::PendingAction;

/// Force a pending confirm onto the app (the same shape arm_tool produces).
fn arm(app: &mut App) {
    app.pending_action = Some(PendingAction::LaunchTool {
        id: "bulwark".to_owned(),
        name: "Bulwark".to_owned(),
    });
}

#[test]
fn active_modal_encodes_the_precedence_help_over_confirm_over_palette() {
    // active_modal is the SINGLE source of truth both render (layout.rs) and
    // input (on_action) branch on, so the overlay drawn on top is always the one
    // capturing keys. This pins the precedence those two share: Help > Confirm >
    // Palette > None. If render and input ever disagreed again, it would be
    // because one stopped reading this — so guarding the order here guards both.
    let mut app = bare_app();
    assert_eq!(app.active_modal(), Modal::None, "no state → no modal");

    app.palette_open = true;
    assert_eq!(app.active_modal(), Modal::Palette, "palette alone");

    // Confirm outranks the palette.
    arm(&mut app);
    assert_eq!(
        app.active_modal(),
        Modal::Confirm,
        "a pending confirm outranks an open palette"
    );

    // Help outranks everything.
    app.show_help = true;
    assert_eq!(
        app.active_modal(),
        Modal::Help,
        "help is the outermost overlay"
    );

    // Peeling back top-down returns to the next-highest each time.
    app.show_help = false;
    assert_eq!(app.active_modal(), Modal::Confirm);
    app.pending_action = None;
    assert_eq!(app.active_modal(), Modal::Palette);
    app.palette_open = false;
    assert_eq!(app.active_modal(), Modal::None);
}

#[test]
fn help_captures_input_even_with_a_palette_or_confirm_behind_it() {
    // The render path draws Help on top when active_modal == Help; on_action must
    // also let Help capture the key (dismiss + swallow) rather than the modal
    // behind it acting. Co-existing state is the case the old inverted ordering
    // would have mis-handled.
    let mut app = bare_app();
    app.palette_open = true;
    arm(&mut app);
    app.show_help = true;

    let mut runner = FakeRunner { calls: 0 };
    let quit = app.on_action(Action::Activate, &mut runner);

    assert!(!quit);
    assert!(!app.show_help, "the key dismissed help, not confirmed the pending");
    assert!(
        app.pending_action.is_some(),
        "the pending confirm behind help must be untouched (Enter went to help)"
    );
    assert_eq!(runner.calls, 0, "nothing ran");
}
