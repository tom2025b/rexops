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
    // Redraw only when something could have changed, not on every tick. An idle
    // TUI (no key, no snapshot, no job output) would otherwise full-render and
    // diff the whole buffer ~10×/s forever — wasted CPU and a flicker risk. We
    // draw once up front, then set `dirty` whenever a snapshot is applied, the
    // job produces output / finishes, or a keypress is handled. The 100 ms poll
    // still bounds streaming-output latency; nothing here needs a timer-based
    // redraw (toasts clear on the next action, not on a clock).
    let mut dirty = true;

    loop {
        if dirty {
            tui.terminal().draw(|f| ui::render(f, app, theme))?;
            dirty = false;
        }

        while let Ok(snapshot) = rx.try_recv() {
            app.apply_snapshot(snapshot);
            dirty = true;
        }

        if app.poll_job() {
            dirty = true;
        }

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
                        // A handled key may have mutated any state; repaint next tick.
                        dirty = true;
                    }
                }
            }
        }
    }
}
