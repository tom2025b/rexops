//! widgets/mod.rs — Reusable ratatui widgets for the TUI.
//!
//! Per the architecture plan, this is where we extract common UI components
//! (HealthBadge, adapter list items, etc.) so screens can compose without
//! duplicating rendering logic.
//!
//! Keep it simple: small, focused widgets that take data from core models
//! (e.g. AdapterHealth) and produce ratatui primitives (Spans, Cells, etc.).
//! No stateful widgets yet unless needed.

pub mod adapter_item;
pub mod health_badge;
pub mod log_line;

// Re-export for convenience in screens/ui.
pub use adapter_item::render_adapter_item;
pub use health_badge::render_health_badge;
pub use log_line::render_log_line;

// Future:
// pub mod adapter_row;
// pub mod log_line;
// etc.

// Learning Notes:
// - Extracting to widgets/ avoids god-files in screens/ and makes the UI
//   easier to theme/test later.
// - Widgets here are "dumb" renderers — they don't own data or handle input
//   (that's in keymap/app/screens).
// - We still use the central theme.rs for colors so widgets stay consistent.
