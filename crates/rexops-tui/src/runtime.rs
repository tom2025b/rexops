//! Draw/input/runtime loop for the RexOps TUI.

use std::sync::mpsc;
use std::time::Duration;

use rexops_core::OpsSnapshot;
use suite_ui::{Theme, Tui};

use crate::{app::App, input, ui};

pub fn run(
    tui: &mut Tui,
    app: &mut App,
    rx: &mpsc::Receiver<OpsSnapshot>,
    theme: Theme,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        tui.terminal().draw(|f| ui::render(f, app, theme))?;

        while let Ok(snapshot) = rx.try_recv() {
            app.apply_snapshot(snapshot);
        }

        app.poll_job();

        if let Some(ev) = input::keymap::next_event(Duration::from_millis(100))? {
            match ev {
                input::keymap::Event::Key(key) => {
                    // The app decides how keys are read (navigating vs typing into
                    // a text field) — pass that mode so bound letters reach a focused
                    // field as input instead of being claimed as commands.
                    if let Some(action) = input::keymap::handle_key(key, app.input_mode()) {
                        if app.on_action(action, tui) {
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
}
