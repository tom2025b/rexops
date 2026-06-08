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

use std::io::{self, stdout, Write};
use std::process::{Command, ExitStatus};
use std::sync::mpsc;
use std::time::Duration;

use crossterm::{
    cursor::{Hide, Show},
    execute,
    style::ResetColor,
    terminal::{
        disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use ratatui::{backend::CrosstermBackend, Terminal};

use rexops_app::load_config;
use rexops_core::OpsSnapshot;
use suite_ui::{ColorChoice, Theme, ThemeChoice};

mod action;
mod app;
mod event;
mod health;
mod keymap;
mod launcher;
mod screens;
mod ui;
mod widgets;

use app::App;
use launcher::{ChildExit, ForegroundRunner};

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

    // Resolve the shared suite theme once: cyan accent, colour on unless
    // NO_COLOR is set (Auto). RexOps has no --color/--theme flag, so this is the
    // single place the suite's NO_COLOR-safe palette enters the TUI.
    let theme = Theme::resolve(ColorChoice::Auto, ThemeChoice::Cyan);

    // Run the main event/draw loop. Any error from the loop is turned into
    // an Err so we still restore the terminal in the caller.
    let result = run_app(&mut terminal, &mut app, &rx, theme);

    // Always restore the terminal, even on error.
    restore_terminal()?;

    result
}

/// Configure the terminal for full-screen TUI use.
fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, Hide)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

/// Restore the terminal to its previous state (called on normal exit and
/// from the panic hook).
fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(stdout(), ResetColor, Show, LeaveAlternateScreen)?;
    Ok(())
}

/// Temporarily give the user's real terminal to a foreground child process.
///
/// The TUI must leave raw mode and the alternate screen before spawning, then
/// restore both even when the child fails to start.
fn run_foreground_child(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    program: &str,
) -> io::Result<ExitStatus> {
    if let Err(err) = suspend_terminal_for_child(terminal) {
        let _ = resume_terminal_after_child(terminal);
        return Err(err);
    }

    let child_result = Command::new(program).status();
    let resume_result = resume_terminal_after_child(terminal);

    match (child_result, resume_result) {
        (_, Err(err)) => Err(err),
        (result, Ok(())) => result,
    }
}

impl ForegroundRunner for Terminal<CrosstermBackend<io::Stdout>> {
    fn run_foreground(&mut self, command: &str) -> io::Result<ChildExit> {
        let status = run_foreground_child(self, command)?;
        if status.success() {
            Ok(ChildExit::Success)
        } else {
            Ok(ChildExit::Status(status.to_string()))
        }
    }
}

/// Leave RexOps' full-screen terminal mode before launching a specialist.
fn suspend_terminal_for_child(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> io::Result<()> {
    terminal.show_cursor()?;
    terminal.backend_mut().flush()?;
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        ResetColor,
        Show,
        LeaveAlternateScreen
    )?;
    Ok(())
}

/// Re-enter RexOps' full-screen terminal mode after a specialist exits.
fn resume_terminal_after_child(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> io::Result<()> {
    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        Clear(ClearType::All),
        Hide
    )?;
    terminal.clear()?;
    Ok(())
}

/// The core loop: draw → handle background results → poll input → handle keys via Event/Action.
/// The 100ms poll timeout is a good balance: responsive keys + we get to
/// drain the mpsc channel frequently without busy-looping.
fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    rx: &mpsc::Receiver<OpsSnapshot>,
    theme: Theme,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        // Draw the current frame. This must be fast; all heavy work happens
        // off the UI thread.
        terminal.draw(|f| ui::render(f, app, theme))?;

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
                        if app.on_action(action, terminal) {
                            // Action indicated we should quit.
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
}
