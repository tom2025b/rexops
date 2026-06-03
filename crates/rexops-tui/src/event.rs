//! event.rs — Crossterm event polling and Event type.
//!
//! The TUI uses a timeout-based poll so the main loop can regularly:
//! - redraw the screen
//! - drain mpsc results from background workers
//! - without blocking on user input.
//!
//! We wrap crossterm's Event into our own small enum so the rest of the
//! code doesn't depend directly on crossterm event types (easier to test
//! or swap input sources later).

use std::io;
use std::time::Duration;

use crossterm::event::{self as crossterm_event, Event as CrosstermEvent, KeyEvent};

/// Our own event type (only what we care about for now).
#[derive(Debug)]
pub enum Event {
    /// A keyboard key was pressed (we only care about Press kind in keymap).
    Key(KeyEvent),
    // Future: Resize(u16, u16), Tick, Mouse(...), etc.
}

/// Poll for the next event, waiting up to `timeout`.
///
/// Returns Ok(Some(event)) if one arrived, Ok(None) on timeout,
/// Err on I/O failure.
pub fn next_event(timeout: Duration) -> io::Result<Option<Event>> {
    if crossterm_event::poll(timeout)? {
        match crossterm_event::read()? {
            CrosstermEvent::Key(key) => {
                // We only care about actual key *presses* for actions.
                // Release and Repeat are ignored for simplicity (keyboard-first TUI).
                if key.kind == crossterm_event::KeyEventKind::Press {
                    Ok(Some(Event::Key(key)))
                } else {
                    Ok(None)
                }
            }
            // Ignore mouse, paste, resize, focus etc. for the initial shell.
            // We can add Resize handling later to force a redraw if needed.
            _ => Ok(None),
        }
    } else {
        Ok(None)
    }
}

// Learning Notes:
// - poll(timeout) + read() is the recommended non-blocking way to integrate
//   crossterm with a game/TUI loop that also needs to do other work (like
//   checking channels).
// - By returning our own Event enum we hide crossterm details from app.rs
//   and ui.rs. This also makes unit testing the app logic easier (we can
//   feed fake Events without a real terminal).
// - We currently drop other event kinds; this is deliberate for "keep it
//   simple" keyboard-first TUI.
