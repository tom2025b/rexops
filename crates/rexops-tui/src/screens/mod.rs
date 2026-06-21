//! screens/mod.rs — Collection of top-level screens for the TUI.
//!
//! Screens are modular views. Each screen knows how to render itself given the
//! current App state.

pub mod adapters;
pub mod cockpit;
pub mod cockpit_nav;
pub mod jobs;
pub mod launchpad;
pub mod scripts;
pub mod system;
pub mod tools;

pub use adapters::render_adapters;
pub use cockpit::render_cockpit;
// The cockpit nav helpers are re-exported for the App layer (Task 4) and the
// renderer (Task 3); until those consumers land they have no caller, so the
// re-export is added when the first consumer is wired. `GROUP_ORDER` already has
// one consumer (`cockpit.rs` imports it directly from `cockpit_nav`).
pub use jobs::render_jobs;
pub use launchpad::render_launcher;
pub use scripts::render_scripts;
pub use system::render_system;
pub use tools::render_tools;
