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

/// Which interpretation the app wants for the next keypress.
///
/// The app is the only thing that knows whether the user is navigating or
/// typing into a text field (the command palette today; search/filter fields
/// later), so it tells the keymap via this mode. Resolving keys globally
/// without it is the bug this fixes: bound command letters (`q`, `r`, digits…)
/// were claimed as commands *before* the palette could receive them as text,
/// so the palette could never accept those characters as input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Lists/screens: the global command bindings apply.
    Navigation,
    /// A text field is focused: printable keys are literal input; only a small
    /// control whitelist (Esc/Enter/arrows/Backspace + the global Ctrl escapes)
    /// maps to actions.
    Text,
}

/// Convert a key press into an Action for the given input `mode`, if it matches.
///
/// Only Press events should be passed here (we filter in next_event).
///
/// `Ctrl-C` (quit) and `Ctrl-P`/`:` (palette) are GLOBAL escapes — they work in
/// both modes so the user is never trapped in a text field. Everything else is
/// mode-dependent: in `Text` mode a printable key is always literal `InputChar`
/// (so `q`, `r`, `1`… type into the field), and only navigation/edit keys act;
/// in `Navigation` mode the command bindings apply.
pub fn handle_key(key: KeyEvent, mode: InputMode) -> Option<Action> {
    // Global escapes — identical in every mode, checked before mode handling.
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Action::Quit);
    }
    if suite_ui::keys::is_palette(key) {
        return Some(Action::OpenPalette);
    }

    match mode {
        InputMode::Text => handle_key_text(key),
        InputMode::Navigation => handle_key_navigation(key),
    }
}

/// Key bindings while a text field is focused. Printable keys are literal input;
/// only the control keys a text field needs map to actions. Note `j`/`k` as
/// CHARACTERS type normally here — only the arrow keys navigate while typing.
fn handle_key_text(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc => Some(Action::Cancel),
        KeyCode::Enter => Some(Action::Activate),
        KeyCode::Up => Some(Action::Up),
        KeyCode::Down => Some(Action::Down),
        KeyCode::Backspace => Some(Action::Backspace),
        KeyCode::Char(c) => Some(Action::InputChar(c)),
        _ => None,
    }
}

/// Global command bindings, active only in navigation mode. (`h` is NOT a help
/// toggle — it collided with vim-style `j`/`k` navigation, throwing the help
/// overlay at users reaching for "left". Use `?` for help.)
fn handle_key_navigation(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('r') => Some(Action::Refresh),
        KeyCode::Char('?') => Some(Action::ToggleHelp),
        KeyCode::Char('1') => Some(Action::SwitchToDashboard),
        KeyCode::Char('2') => Some(Action::SwitchToAdapters),
        KeyCode::Char('3') => Some(Action::SwitchToSystem),
        KeyCode::Char('4') => Some(Action::SwitchToScripts),
        KeyCode::Char('5') => Some(Action::SwitchToTools),
        KeyCode::Char('6') => Some(Action::SwitchToLauncher),
        KeyCode::Char('7') => Some(Action::SwitchToJobs),
        KeyCode::Char('x') => Some(Action::CancelJob),
        KeyCode::Char('j') | KeyCode::Down => Some(Action::Down),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::Up),
        KeyCode::Enter => Some(Action::Activate),
        KeyCode::Esc => Some(Action::Cancel),
        KeyCode::Backspace => Some(Action::Backspace),
        KeyCode::Char(c) => Some(Action::InputChar(c)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn text_mode_keeps_bound_command_letters_as_literal_input() {
        // THE BUG THIS FIXES: in a focused text field, the command letters must
        // type as characters, not fire their global command. Before mode-aware
        // resolution, `q`→Quit, `r`→Refresh, digits→SwitchTo* were claimed
        // globally, so the palette could never receive them as text.
        for c in ['q', 'r', 'x', 'j', 'k', '1', '7', 'h', '?'] {
            assert_eq!(
                handle_key(ch(c), InputMode::Text),
                Some(Action::InputChar(c)),
                "in Text mode, '{c}' must be literal input, not a command"
            );
        }
    }

    #[test]
    fn text_mode_still_maps_the_control_whitelist() {
        // The small whitelist a text field needs still acts in Text mode.
        assert_eq!(handle_key(key(KeyCode::Esc), InputMode::Text), Some(Action::Cancel));
        assert_eq!(handle_key(key(KeyCode::Enter), InputMode::Text), Some(Action::Activate));
        assert_eq!(handle_key(key(KeyCode::Up), InputMode::Text), Some(Action::Up));
        assert_eq!(handle_key(key(KeyCode::Down), InputMode::Text), Some(Action::Down));
        assert_eq!(
            handle_key(key(KeyCode::Backspace), InputMode::Text),
            Some(Action::Backspace)
        );
    }

    #[test]
    fn navigation_mode_resolves_global_commands() {
        assert_eq!(handle_key(ch('q'), InputMode::Navigation), Some(Action::Quit));
        assert_eq!(handle_key(ch('r'), InputMode::Navigation), Some(Action::Refresh));
        assert_eq!(handle_key(ch('x'), InputMode::Navigation), Some(Action::CancelJob));
        assert_eq!(
            handle_key(ch('1'), InputMode::Navigation),
            Some(Action::SwitchToDashboard)
        );
        assert_eq!(handle_key(ch('j'), InputMode::Navigation), Some(Action::Down));
        assert_eq!(handle_key(ch('?'), InputMode::Navigation), Some(Action::ToggleHelp));
    }

    #[test]
    fn h_is_no_longer_a_help_toggle_in_either_mode() {
        // `h` collided with vim-style navigation; it must NOT open help. Help is
        // `?` only. In nav mode `h` is a plain InputChar (no binding); in text
        // mode it types normally.
        assert_eq!(handle_key(ch('h'), InputMode::Navigation), Some(Action::InputChar('h')));
        assert_eq!(handle_key(ch('h'), InputMode::Text), Some(Action::InputChar('h')));
        assert_ne!(handle_key(ch('h'), InputMode::Navigation), Some(Action::ToggleHelp));
    }

    #[test]
    fn ctrl_c_and_palette_are_global_escapes_in_both_modes() {
        // The two escapes that must work even inside a text field, so the user is
        // never trapped: Ctrl-C quits, Ctrl-P opens the palette.
        for mode in [InputMode::Navigation, InputMode::Text] {
            assert_eq!(handle_key(ctrl('c'), mode), Some(Action::Quit), "{mode:?}");
            assert_eq!(handle_key(ctrl('p'), mode), Some(Action::OpenPalette), "{mode:?}");
        }
    }
}
