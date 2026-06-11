//! keymap.rs — event polling plus the key → Action bindings.
//!
//! This is the single place that defines "what does pressing this key do?"
//! It keeps the main event loop and App free of magic key constants, and
//! wraps crossterm's event types so the rest of the code doesn't depend on
//! them directly.
//!
//! The TUI uses a timeout-based poll so the main loop can regularly:
//! - redraw the screen
//! - drain mpsc results from background workers
//! - without blocking on user input.

use std::io;
use std::time::Duration;

use crossterm::event::{
    self as crossterm_event, Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers,
};

use crate::input::Action;

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

/// Convert a key press into an Action, if it matches a binding.
///
/// Only Press events should be passed here (we filter in next_event).
///
/// The command palette (`Ctrl-P` / `:`) is detected first via the shared
/// `suite_ui::keys::is_palette` so the suite's bindings stay consistent across
/// tools. `:` would otherwise fall through to `InputChar`, so this MUST precede
/// the generic `Char(c)` arm.
pub fn handle_key(key: KeyEvent) -> Option<Action> {
    if suite_ui::keys::is_palette(key) {
        return Some(Action::OpenPalette);
    }
    match key.code {
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('r') => Some(Action::Refresh),
        KeyCode::Char('?') | KeyCode::Char('h') => Some(Action::ToggleHelp),
        KeyCode::Char('1') => Some(Action::SwitchToDashboard),
        KeyCode::Char('2') => Some(Action::SwitchToAdapters),
        KeyCode::Char('3') => Some(Action::SwitchToSystem),
        KeyCode::Char('4') => Some(Action::SwitchToScripts),
        KeyCode::Char('5') => Some(Action::SwitchToTools),
        KeyCode::Char('6') => Some(Action::SwitchToLauncher),
        KeyCode::Char('7') => Some(Action::SwitchToJobs),
        KeyCode::Char('x') => Some(Action::CancelJob),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::Quit),
        KeyCode::Char('j') | KeyCode::Down => Some(Action::Down),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::Up),
        KeyCode::Enter => Some(Action::Activate),
        KeyCode::Esc => Some(Action::Cancel),
        KeyCode::Backspace => Some(Action::Backspace),
        KeyCode::Char(c) => Some(Action::InputChar(c)),
        _ => None,
    }
}
