//! action.rs — High-level actions that the TUI can perform.
//!
//! The event loop (main.rs) turns raw crossterm key events (via keymap) into
//! these Actions. The App then handles them in on_action.
//!
//! This separation makes keybindings easy to change (in keymap.rs) without
//! touching app logic, and makes it simple to add non-key sources of actions
//! later (e.g. mouse, external messages, timers).

/// Actions the user or system can trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Quit the application (q, Esc, Ctrl-C, etc.).
    Quit,

    /// Request a background refresh of the snapshot from adapters.
    Refresh,

    /// Toggle the help text / overlay.
    ToggleHelp,

    /// Request a specialist-tool launch. Stage 1 keeps this intentionally unbound.
    #[allow(dead_code)]
    Launch,

    /// Switch to the main dashboard view.
    SwitchToDashboard,

    /// Switch to a secondary "adapters" focused view (demo of screens/).
    SwitchToAdapters,

    /// Switch to the system info screen (using SystemAdapter data).
    SwitchToSystem,

    /// Switch to the scripts/vault screen (using ScriptVaultAdapter data).
    SwitchToScripts,

    /// Switch to the tools/inventory screen (using ToolFoundryAdapter data for ownership/lifecycle/symlinks).
    SwitchToTools,

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

// Learning Notes:
// - Using a small enum for actions is a classic "Elm/Redux-like" architecture
//   pattern in TUIs and games. It decouples input from behavior.
// - We derive the usual traits so Actions can be logged, matched easily, or
//   queued if we ever want an action queue.
// - Keep this enum small and focused on *intent*, not on *how* the key was
//   pressed.
