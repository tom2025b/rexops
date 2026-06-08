//! keymap.rs — Maps keyboard input to high-level Actions.
//!
//! This is the single place that defines "what does pressing this key do?"
//! It keeps the main event loop and App free of magic key constants.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::Action;

/// Convert a key press into an Action, if it matches a binding.
///
/// Only Press events should be passed here (we filter in the loop).
pub fn handle_key(key: KeyEvent) -> Option<Action> {
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
