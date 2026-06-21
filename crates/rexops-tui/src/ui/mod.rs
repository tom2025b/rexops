//! Rendering layer for the RexOps TUI: frame layout, footer status bar,
//! modal overlays (palette / help / confirm), and reusable widgets.

pub mod cockpit_widgets;
mod layout;
mod palette;
mod status_bar;
pub mod widgets;

pub use layout::render;

#[cfg(test)]
mod tests;
