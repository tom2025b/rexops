//! event.rs — Crossterm event polling and Event type.
//!
//! The TUI uses a timeout-based poll so the main loop can regularly:
//! - redraw the screen
//! - drain mpsc results from background workers
//! - without blocking on user input.
//!
//! We wrap crossterm's Event into our own small enum so the rest of the
//! code doesn't depend directly on crossterm event types.

use std::io;
use std::time::Duration;

use crossterm::event::{self as crossterm_event, Event as CrosstermEvent, KeyEvent};

/// Event type for inputs RexOps handles.
#[derive(Debug)]
pub enum Event {
    /// A keyboard key was pressed (we only care about Press kind in keymap).
    Key(KeyEvent),
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
            // Ignore mouse, paste, resize, focus, and other non-key events.
            _ => Ok(None),
        }
    } else {
        Ok(None)
    }
}
