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

use std::io::{self, stdout};
use std::sync::mpsc;
use std::time::Duration;

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use rexops_app::load_config;
use rexops_core::OpsSnapshot;

mod action;
mod app;
mod event;
mod keymap;
mod screens;
mod theme;
mod ui;
mod widgets;

use app::App;

/// Entry point. We return a Result so that any setup error is reported after
/// we have done our best to restore the terminal.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Install a panic hook that restores the terminal before the default
    // panic handler prints the backtrace. This is critical for TUI apps.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // Best effort — ignore errors during restore in a panic path.
        let _ = restore_terminal();
        original_hook(panic_info);
    }));

    // Set up the terminal (raw mode + alternate screen).
    let mut terminal = setup_terminal()?;

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

    // Run the main event/draw loop. Any error from the loop is turned into
    // an Err so we still restore the terminal in the caller.
    let result = run_app(&mut terminal, &mut app, &rx);

    // Always restore the terminal, even on error.
    restore_terminal()?;

    result
}

/// Configure the terminal for full-screen TUI use.
fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

/// Restore the terminal to its previous state (called on normal exit and
/// from the panic hook).
fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    Ok(())
}

/// The core loop: draw → handle background results → poll input → handle keys via Event/Action.
/// The 100ms poll timeout is a good balance: responsive keys + we get to
/// drain the mpsc channel frequently without busy-looping.
fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    rx: &mpsc::Receiver<OpsSnapshot>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        // Draw the current frame. This must be fast; all heavy work happens
        // off the UI thread.
        terminal.draw(|f| ui::render(f, app))?;

        // Drain any snapshots that background threads have finished producing.
        // try_recv is non-blocking so we never stall the draw loop.
        while let Ok(snapshot) = rx.try_recv() {
            app.apply_snapshot(snapshot);
            app.refreshing = false;
        }

        // Poll for input using our event module (timeout allows regular draws + channel checks).
        if let Some(ev) = event::next_event(Duration::from_millis(100))? {
            match ev {
                event::Event::Key(key) => {
                    if let Some(action) = keymap::handle_key(key) {
                        if app.on_action(action) {
                            // Action indicated we should quit.
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
}

// Learning Notes:
// - Using a short poll timeout + try_recv is the classic "cooperative
//   multitasking" trick for single-threaded TUIs when you have blocking I/O
//   work that must not freeze the screen.
// - The panic hook + explicit restore pair is non-negotiable for any
//   full-screen terminal app. Users hate having to run `reset` after a crash.
// - We keep main.rs small and delegate state + rendering to dedicated modules
//   so the entry point stays easy to audit.
