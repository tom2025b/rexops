//! main.rs — RexOps TUI binary entry point.
//!
//! A thin shim: the entire launch sequence (terminal setup/teardown, the
//! refresh channel, config load, and the event/draw loop) lives behind
//! [`rexops_tui::run`] in this crate's library, so both this binary and the
//! `rexops` CLI (which launches the TUI when given no subcommand) drive exactly
//! the same code.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    rexops_tui::run()
}
