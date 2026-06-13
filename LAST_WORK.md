# Last Work

## 2026-06-13 — Restore the TUI as the default `cargo run` action

**Merged:** `fix/restore-tui-default` → `main` (fast-forward, commit `06a60c5`).
Local only — not pushed to `origin/main`.

### What changed
`cargo run` (and `rexops` with no subcommand) had been reaching only a thin CLI
with `status`/`adapters`; the interactive TUI was reachable solely via
`cargo run -p rexops-tui`. The TUI is now the default action again.

- **`crates/rexops-tui/src/lib.rs`** (new): the whole launch sequence (stdin
  capture, the `suite_ui::Tui` terminal guard, the refresh channel, config load,
  initial probe, theme, event loop) plus the `ForegroundRunner for Tui` impl
  moved here verbatim behind a single public `rexops_tui::run()`.
- **`crates/rexops-tui/src/main.rs`**: reduced to a thin shim calling
  `rexops_tui::run()`, so the standalone binary and the CLI drive identical code.
- **`crates/rexops-cli`**: depends on `rexops-tui`; the clap subcommand is now
  `Option<Commands>`. No subcommand → `rexops_tui::run()`; `status` / `adapters`
  (and `--json` on them) are unchanged.
- **`Cargo.toml`**: refreshed the `default-members` comment to note that the
  default `rexops` binary launches the TUI when given no subcommand.

### Verification (on `main`, after merge + `git pull`)
- `cargo run` (no args), driven under a sized PTY: enters the alternate screen,
  renders the full TUI (Dashboard title, Adapters table, Risk Summary pane), and
  on `q` exits with code 0 and restores the terminal. No old thin-CLI text.
- `rexops status` / `adapters` / `--json status` / `--help` behave as before
  (`--help` now shows `Usage: rexops [OPTIONS] [COMMAND]` — subcommand optional).
- `cargo test --workspace` → **green** (174 unit tests: app 7, core 49, tui 118).
- `cargo clippy --all-targets -- -D warnings` → **green** (exit 0, no warnings).
