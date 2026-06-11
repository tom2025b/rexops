//! Command palette and confirmation flows.

mod dispatch;
pub mod palette;

pub use dispatch::PendingAction;
pub use palette::{Command, PaletteCommand};
