//! Application state and state transitions for the RexOps TUI.

mod navigation;
mod state;
mod update;

pub use navigation::Screen;
pub use state::App;

#[cfg(test)]
mod tests;
