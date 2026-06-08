//! action.rs — High-level actions that the TUI can perform.
//!
//! The event loop (main.rs) turns raw crossterm key events (via keymap) into
//! these Actions. The App then handles them in on_action.
//!
//! This separation keeps keybindings in keymap.rs without touching app logic.

/// Actions the user or system can trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Quit the application (q, Esc, Ctrl-C, etc.).
    Quit,

    /// Request a background refresh of the snapshot from adapters.
    Refresh,

    /// Toggle the help text / overlay.
    ToggleHelp,

    /// Switch to the main dashboard view.
    SwitchToDashboard,

    /// Switch to the adapters view.
    SwitchToAdapters,

    /// Switch to the system info screen (using SystemAdapter data).
    SwitchToSystem,

    /// Switch to the scripts screen.
    SwitchToScripts,

    /// Switch to the tools/inventory screen.
    SwitchToTools,

    /// Switch to the Launcher screen (pick a tool and launch it).
    SwitchToLauncher,

    /// Navigate up in lists (k or up arrow).
    Up,

    /// Navigate down in lists (j or down arrow).
    Down,

    /// Activate/Select current item (enter) for detail or action.
    Activate,

    /// Cancel current mode (e.g. clear filter) or quit.
    Cancel,

    /// Printable char input (for filter in adapters screen).
    InputChar(char),

    /// Backspace for editing filter.
    Backspace,
}
