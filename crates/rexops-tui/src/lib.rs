//! rexops-tui — Ratatui + crossterm TUI for RexOps, exposed as a library.
//!
//! The whole launch sequence lives behind a single public [`run`] entrypoint so
//! both surfaces drive the *same* code:
//! - the crate's own `rexops-tui` binary (`src/main.rs` is a thin shim), and
//! - the `rexops` CLI, which launches the TUI when invoked with no subcommand.
//!
//! Responsibilities of [`run`] (thin shell only):
//! - Capture any piped stdin ONCE, before touching the terminal.
//! - Set up and tear down the terminal via the shared `suite_ui::Tui` guard
//!   (raw mode + alternate screen + a panic hook that restores the terminal so
//!   a crash never leaves it broken). `Drop` guarantees restore on every exit
//!   path (clean return, `?`-error, or panic).
//! - Create the mpsc channel that background refresh workers use to deliver
//!   completed `OpsSnapshot` values back to the UI thread without blocking draw.
//! - Hand off to the runtime event/draw loop.
//!
//! All domain knowledge lives in rexops-core (OpsSnapshot, AdapterHealth, etc.);
//! all probing lives in rexops-adapters (reached via rexops-app's snapshot
//! builders). This crate owns no domain logic or execution beyond launching the
//! tools the catalog defines.

// This crate drives the terminal and spawns child processes, so an unhandled
// unwrap/expect here panics the whole UI mid-session. Hold it to the same
// no-panic floor the library crates (core/adapters/app) already enforce. The
// single known-safe production site is annotated with a scoped #[allow] +
// justification. Tests are exempt: a panicking unwrap there fails the test
// loudly, which is the desired behaviour.
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::io;
use std::process::{Command, ExitStatus};
use std::sync::mpsc;
use std::time::Duration;

use crossterm::event;
use rexops_app::{load_config, read_piped_stdin};
use suite_ui::{ColorChoice, Theme, ThemeChoice, Tui, TuiOptions};

mod app;
mod commands;
mod input;
mod jobs;
mod runtime;
mod screens;
mod tools;
mod ui;

use app::App;
use tools::{ChildExit, ForegroundRunner, LaunchCommand};

/// Launch the RexOps TUI and run it until the user quits.
///
/// Returns a `Result` so any setup error is reported after we have done our best
/// to restore the terminal. The shared `Tui` guard is dropped on every exit path
/// (including `?`-error), which guarantees the terminal is restored.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Capture any piped stdin ONCE, before we touch the terminal. A Workstate
    // snapshot can be fed in via a pipe (`workstate snapshot | rexops`); stdin is
    // consume-once, so the TUI reads it a single time here and hands the captured
    // bytes to every refresh thread, rather than re-reading per refresh (which
    // would drain it after the first probe, or block forever on a pipe that never
    // closes). Done first, in cooked mode, so the blocking read never races
    // terminal setup. `None` when stdin is a tty / empty.
    let piped_stdin = read_piped_stdin();

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
    let mut app = App::new(tx, config, piped_stdin);
    app.request_refresh(); // initial probe on startup

    // Resolve the shared suite theme once: cyan accent, colour on unless
    // NO_COLOR is set (Auto). RexOps has no --color/--theme flag, so this is the
    // single place the suite's NO_COLOR-safe palette enters the TUI.
    let theme = Theme::resolve(ColorChoice::Auto, ThemeChoice::Cyan);

    // Run the main event/draw loop. `run_app` borrows the whole guard so it can
    // both draw (via `tui.terminal()`) and hand the guard to the launcher as the
    // `ForegroundRunner` (which uses `Tui::suspended`). On `?`-error here, `tui`
    // still drops and restores the terminal.
    runtime::run(&mut tui, &mut app, &rx, theme)

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
    fn run_foreground(&mut self, command: &LaunchCommand) -> io::Result<ChildExit> {
        let status: ExitStatus =
            self.suspended(|| Command::new(&command.program).args(&command.args).status())??;
        drain_pending_events()?;
        if status.success() {
            Ok(ChildExit::Success)
        } else {
            Ok(ChildExit::Status(status.to_string()))
        }
    }
}

fn drain_pending_events() -> io::Result<()> {
    while event::poll(Duration::from_millis(0))? {
        let _ = event::read()?;
    }
    Ok(())
}
