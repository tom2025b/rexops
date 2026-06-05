# Phase 7 — Launcher screen (TUI) — design

**Date:** 2026-06-04
**Status:** Approved, ready to implement
**Repo:** rexops only

## Goal

Add a 6th TUI screen ("Launcher") that lists the available tools with short
descriptions, lets the user navigate with arrow keys, press Enter to launch the
selected tool (real subprocess when a binary resolves, graceful message
otherwise), and Esc to go back to the Dashboard.

This is a working-launcher milestone. Per-tool argv, async launching, and a
config-driven catalog are explicitly future phases.

## Tool catalog (static)

A small static list lives in the new screen module — the simplest fit for a
fixed, known toolset:

| id          | display name | description                                |
|-------------|--------------|--------------------------------------------|
| bulwark     | Bulwark      | Content/security inspection (live scan)    |
| scriptvault | ScriptVault  | Script inventory & launcher                |
| toolfoundry | ToolFoundry  | Tool ownership & lifecycle (feed)          |
| workstate   | Workstate    | Per-project repo health (feed)             |

Each entry is `(id, name, description)`. ToolFoundry and Workstate are read-only
feed consumers with no executable — that is expected and handled at launch time
(see below), not by hiding them.

## Launch resolution (generalize, don't duplicate)

Rename `launcher::launch_scriptvault` → `launcher::launch_tool(tool_id, name,
config, runner)`, reusing the existing resolution (`which` on PATH → config
`binary`), `LaunchReport`, and `ForegroundRunner`:

- **Command resolves** → real foreground subprocess, suspending/restoring the
  TUI (exactly as ScriptVault does today). Report refreshes on return.
- **No command** → `LaunchReport::no_refresh("<Name> has no launch command
  yet")`. The runner is never called.

The existing `Action::Launch` routes through `launch_tool("scriptvault",
"ScriptVault", …)` so the two current launcher tests stay green.

## State & navigation

- `app.rs`: add `Screen::Launcher` and `selected_tool: usize` (index into the
  static catalog — no filtering, so an index is simpler than the name-based
  selection the Adapters screen uses). Init `selected_tool: 0`.
- `on_action` arms (all gated on `Screen::Launcher`):
  - `Up` / `Down` → move `selected_tool`, wrapping over the catalog length.
  - `Activate` (Enter) → `launch_tool` on the selected tool; log the report;
    `request_refresh()` if the report says so.
  - `Cancel` (Esc) → **new arm**: in `Screen::Launcher`, switch to Dashboard
    and return `false` (today Esc quits — this satisfies the "Esc to go back"
    requirement the current code does not meet).

## Wiring (the established add-a-screen path) — exhaustive

1. `action.rs` — `Action::SwitchToLauncher`.
2. `keymap.rs` — `'6'` → `SwitchToLauncher`.
3. `ui.rs` — **three edits**: dispatch match arm; status-bar `left` string for
   `Screen::Launcher`; help-popup text mentions the Launcher.
4. `app.rs` — `Screen::Launcher` variant, `selected_tool` field + init,
   Up/Down/Activate/Cancel arms.
5. `screens/launchpad.rs` (named **launchpad** to avoid confusion with the
   existing `crate::launcher` orchestration module) exporting `render_launcher`;
   add `pub mod launchpad;` + `pub use launchpad::render_launcher;` to
   `screens/mod.rs`.

## Rendering

Mirror `scripts.rs`: a bordered list, the selected row highlighted via
`widgets::render_adapter_item(name, health, description, is_selected)`. Health
per row comes from `snapshot.adapter_health` (so the badge is meaningful — a
non-probed tool shows Unknown). Status bar for the screen:
`q quit • ↑/↓ nav • enter launch • esc back • 1 dashboard`.

## Testing

- `launch_tool` resolves a configured binary and launches via a `FakeRunner`
  (generalize the existing `launch_scriptvault_reports_success_and_refreshes`).
- `launch_tool` on a tool with no binary returns a no-refresh "no launch command
  yet" report **without** calling the runner.
- `on_action(Up)` / `on_action(Down)` in `Screen::Launcher` move `selected_tool`
  and wrap at the ends.
- `on_action(Cancel)` in `Screen::Launcher` returns `false` (does not quit) and
  sets `current_screen == Dashboard`.
- The pre-existing `Action::Launch` test still passes.

## Verification

`cargo build` (TUI), `cargo test` (workspace), `cargo clippy --all-targets`
clean. The TUI is interactive, so navigation/launch are covered by the
`on_action` unit tests above rather than a live driver.
