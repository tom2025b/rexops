//! screens/mod.rs — Collection of top-level screens for the TUI.
//!
//! Per the architecture plan, screens are modular views (Dashboard,
//! Adapters/Status, Tools/Inventory, etc.). Each screen knows how to render
//! itself given the current App state.
//!
//! For the initial shell + dashboard we only have one screen, but the
//! structure is in place so adding `adapters_status.rs`, `tools.rs` etc.
//! will be natural.

pub mod adapters;
pub mod dashboard;
pub mod scripts;
pub mod system;
pub mod tools;

pub use adapters::render_adapters;
pub use dashboard::render_dashboard;
pub use scripts::render_scripts;
pub use system::render_system;
pub use tools::render_tools;

// Widgets are re-exported at crate root level for now; screens import directly
// via `crate::widgets` to keep things explicit.

// Learning Notes:
// - Using a screens/ module with submodules keeps the top level of src/
//   clean and mirrors the plan ("tui/screens/ (dashboard.rs, ...)").
// - Each screen can later own its own local state (e.g. selected row,
//   filter text, scroll offset) if needed, while the global snapshot and
//   cross-cutting state (refreshing, show_help) stay in App.
