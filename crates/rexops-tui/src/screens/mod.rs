//! screens/mod.rs — Collection of top-level screens for the TUI.
//!
//! Screens are modular views. Each screen knows how to render itself given the
//! current App state.

pub mod adapters;
pub mod cockpit;
pub mod jobs;
pub mod launchpad;
pub mod scripts;
pub mod system;
pub mod tools;

pub use adapters::render_adapters;
pub use cockpit::render_cockpit;
pub use jobs::render_jobs;
pub use launchpad::render_launcher;
pub use scripts::render_scripts;
pub use system::render_system;
pub use tools::render_tools;
