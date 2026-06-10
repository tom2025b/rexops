//! main.rs — RexOps TUI entry point (ratatui + crossterm).
//!
//! Responsibilities (thin shell only):
//! - Set up and tear down the terminal (raw mode + alternate screen).
//! - Install a panic hook that restores the terminal so a crash never leaves
//!   the user's terminal in a broken state.
//! - Own the event loop: draw the UI, poll for keyboard input (with short
//!   timeout), and drain any results coming back from background worker threads.
//! - Create a single mpsc channel used by refresh workers to send fresh
//!   OpsSnapshot values back to the UI thread without blocking drawing.
//!
//! All domain knowledge lives in rexops-core (OpsSnapshot, AdapterHealth, etc.).
//! All probing lives in rexops-adapters (via the small build_snapshot helper
//! in this crate for the initial dashboard).
//!
//! Non-blocking refresh strategy (see TUI_DESIGN.md):
//! - 'r' sets a flag and spawns a std::thread.
//! - The thread runs the (potentially slow) adapter probes and sends the
//!   resulting snapshot over the channel.
//! - The main loop uses try_recv() so drawing continues at full speed.

use std::io;
use std::process::{Command, ExitStatus};
use std::sync::mpsc;
use std::time::Duration;

use rexops_app::load_config;
use rexops_core::OpsSnapshot;
use suite_ui::{ColorChoice, Theme, ThemeChoice, Tui, TuiOptions};

mod action;
mod app;
mod event;
mod health;
mod jobs;
mod keymap;
mod launcher;
mod palette;
mod screens;
mod ui;
mod widgets;

use app::App;
use launcher::{ChildExit, ForegroundRunner};

/// Entry point. We return a Result so that any setup error is reported after
/// we have done our best to restore the terminal.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Enter TUI mode via the shared suite guard. `Tui` owns terminal setup
    // (raw mode + alternate screen + cursor-hide), installs the panic hook that
    // restores the terminal before the default handler runs, and — via its
    // `Drop` — guarantees the terminal is restored on every exit path (clean
    // return, `?`-error, or panic). This replaces RexOps' hand-rolled
    // setup_terminal/restore_terminal/panic-hook trio.
    let mut tui = Tui::new(TuiOptions {
        hide_cursor: true,
        mouse_capture: false,
        require_tty: false,
    })?;

    // Create the channel that background refresh threads will use to deliver
    // completed OpsSnapshot values. We move the Sender into the App so that
    // App::request_refresh can clone it when spawning workers.
    let (tx, rx) = mpsc::channel();

    // Load config once via the shared rexops-app layer (no more duplication
    // with CLI). The resulting AppConfig is cloned into the App and into
    // each refresh worker thread.
    let config = load_config();

    // Build the application state. We start with a fresh empty snapshot and
    // immediately kick off one background refresh so the user sees live data
    // without having to press 'r' first.
    let mut app = App::new(tx, config);
    app.request_refresh(); // initial probe on startup

    // Resolve the shared suite theme once: cyan accent, colour on unless
    // NO_COLOR is set (Auto). RexOps has no --color/--theme flag, so this is the
    // single place the suite's NO_COLOR-safe palette enters the TUI.
    let theme = Theme::resolve(ColorChoice::Auto, ThemeChoice::Cyan);

    // Run the main event/draw loop. `run_app` borrows the whole guard so it can
    // both draw (via `tui.terminal()`) and hand the guard to the launcher as the
    // `ForegroundRunner` (which uses `Tui::suspended`). On `?`-error here, `tui`
    // still drops and restores the terminal.
    run_app(&mut tui, &mut app, &rx, theme)

    // `tui` drops here → guaranteed terminal restore.
}

/// Run a foreground child program on the user's real terminal.
///
/// The leave→run→re-enter dance (drop out of raw mode + the alternate screen,
/// run the child, then re-enter and clear) is owned by the shared
/// [`suite_ui::Tui::suspended`] guard, which guarantees re-entry even if the
/// child or a step fails — so the terminal is never left suspended. This
/// replaces RexOps' hand-rolled suspend_terminal_for_child /
/// resume_terminal_after_child / run_foreground_child trio.
impl ForegroundRunner for Tui {
    fn run_foreground(&mut self, command: &str) -> io::Result<ChildExit> {
        let status: ExitStatus = self.suspended(|| Command::new(command).status())??;
        if status.success() {
            Ok(ChildExit::Success)
        } else {
            Ok(ChildExit::Status(status.to_string()))
        }
    }
}

/// The core loop: draw → handle background results → poll input → handle keys via Event/Action.
/// The 100ms poll timeout is a good balance: responsive keys + we get to
/// drain the mpsc channel frequently without busy-looping.
fn run_app(
    tui: &mut Tui,
    app: &mut App,
    rx: &mpsc::Receiver<OpsSnapshot>,
    theme: Theme,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        // Draw the current frame. This must be fast; all heavy work happens
        // off the UI thread.
        tui.terminal().draw(|f| ui::render(f, app, theme))?;

        // Drain any snapshots that background threads have finished producing.
        // try_recv is non-blocking so we never stall the draw loop.
        while let Ok(snapshot) = rx.try_recv() {
            app.apply_snapshot(snapshot);
            app.refreshing = false;
        }

        // Drain the running background job's output (and finish it on exit). Like
        // the snapshot drain, this is non-blocking so the draw loop never stalls.
        app.poll_job();

        // Poll for input using our event module (timeout allows regular draws + channel checks).
        if let Some(ev) = event::next_event(Duration::from_millis(100))? {
            match ev {
                event::Event::Key(key) => {
                    if let Some(action) = keymap::handle_key(key) {
                        if app.on_action(action, tui) {
                            // Action indicated we should quit.
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
}
