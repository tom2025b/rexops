//! screens/mod.rs — Collection of top-level screens for the TUI.
//!
//! Screens are modular views. Each screen knows how to render itself given the
//! current App state.

pub mod adapters;
pub mod cockpit;
pub mod cockpit_detail;
pub mod cockpit_nav;
pub mod jobs;
pub mod launchpad;
pub mod scripts;
pub mod system;
pub mod tools;

pub use adapters::render_adapters;
pub use cockpit::render_cockpit;
pub use cockpit_detail::render_cockpit_detail;
pub use cockpit_nav::{cockpit_visit_order, component_for_marker};
pub use jobs::render_jobs;
pub use launchpad::render_launcher;
pub use scripts::render_scripts;
pub use system::render_system;
pub use tools::render_tools;
