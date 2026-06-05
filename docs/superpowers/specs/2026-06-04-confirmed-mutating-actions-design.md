# Phase 8 — Safe, Confirmed Mutating Actions (design)

Date: 2026-06-04
Status: approved
Scope: `rexops` repo only (crate `rexops-tui`)

## Goal

Add the foundation for actions that actually *do* something — starting with
launching a specialist tool — while keeping everything safe. Every mutating
action must pass through an explicit, hard-to-miss confirmation step. Nothing
destructive is introduced; this phase builds the safety/confirmation layer.

## Non-goals

- No destructive operations (no delete, no overwrite, no run-arbitrary-script).
- No trait-object "confirmable action" framework. A small enum is enough for a
  known, fixed action set (KISS / YAGNI).
- No config changes, no new keybindings.

## Architecture

The confirmation flow is a **state machine that lives entirely in `App`**, so it
is unit-testable without a real terminal — the same way the existing launch flow
is tested with `FakeRunner`. Rendering is a thin overlay in `ui.rs`. Input is
gated at the top of `App::on_action`: while an action is pending, the modal
consumes keys and they never reach the underlying screen.

Reused existing infrastructure:
- `launcher::launch_tool` / `ForegroundRunner` — unchanged execution path.
- `ui.rs` `centered_rect` + `Clear` + `render_help_popup` pattern — the confirm
  modal mirrors it.
- `Action::Activate` (Enter) and `Action::Cancel` (Esc) — no new keys needed.

## Components

### 1. `PendingAction` enum (`app.rs`) — the reusable core

```rust
pub enum PendingAction {
    LaunchTool { id: String, name: String },
}
```

Single variant for now (per user direction; expand later for real mutating
actions). Two methods:

- `prompt(&self) -> String` → e.g. `"Launch ScriptVault?"`
- `preview(&self, config: &AppConfig) -> String` → the resolved command, e.g.
  `"Will run: /usr/bin/scriptvault"`, or `"No launch command yet (nothing will run)"`
  for feed-only tools.

Future mutating actions add a variant + match arms. That *is* the reusable
pattern — no abstraction tax.

### 2. State field (`App`)

```rust
pub pending_action: Option<PendingAction>,
```

Initialized `None` in `App::new`.

### 3. Input gate (top of `App::on_action`)

Before any screen dispatch, if `pending_action.is_some()`:

- `Action::Activate` (Enter) → **confirm**: take the pending action, execute it,
  clear `pending_action`, log the result, request refresh if the report says so.
- `Action::Cancel` (Esc) → **cancel**: clear `pending_action`, log
  `"<name>: launch cancelled"`. Never quits the app.
- any other action → **ignored** (the modal is modal; nothing leaks through).

Returns `false` (never quits) in all pending-state branches.

### 4. Request path (single gated door)

- `Activate` on the **Launcher** screen no longer calls `launch_tool` directly.
  It now **sets** `pending_action = Some(LaunchTool { id, name })` for the
  selected catalog tool and logs `"<name>: confirm launch (Enter) or cancel (Esc)"`.
- The dead `Action::Launch` arm is **removed** along with the enum variant — it
  was an unconfirmed second door. After this change there is exactly one launch
  path, and it is gated.

### 5. Preview / dry-run

- `launcher::resolve_command` is promoted to `pub(crate)` so
  `PendingAction::preview` can show the resolved command **without spawning**.
- The modal's "Will run:" line is the dry-run: the user always sees exactly what
  would execute (or that nothing would) before confirming.

### 6. Rendering (`ui.rs`)

- `render_confirm_popup(f, app, area)` drawn when `pending_action.is_some()`,
  reusing `centered_rect` + `Clear`.
- Drawn **after** (on top of) screen content and taking precedence over the help
  popup, so confirmation is never hidden.
- Visually explicit / hard to miss (per user direction): bright bordered box,
  bold title `⚠ CONFIRM`, the prompt, the "Will run:" preview line, and a clear
  `[ Enter = YES ]   [ Esc = no ]` affordance.
- Status bar shows a confirm hint while pending.

## Data flow

```
Launcher screen, tool selected
  │  Enter (Activate)
  ▼
App.on_action → set pending_action = LaunchTool{..}   (no spawn)
  │
  ▼
ui.render → render_confirm_popup (overlay, shows preview = dry-run)
  │
  ├─ Enter (Activate) ─► take pending, launch_tool(...) via ForegroundRunner,
  │                       clear pending, log report, maybe refresh
  └─ Esc   (Cancel)   ─► clear pending, log "cancelled", no spawn
```

## Safety properties

- No process spawns without an explicit Enter on the modal.
- Feed-only tools (ToolFoundry, Workstate) preview `"No launch command yet"` and,
  if confirmed, degrade gracefully with no spawn — unchanged behavior.
- The only mutating action is launching a tool (same effect as before), now
  behind confirmation. Nothing destructive added.
- Exactly one launch code path remains; the unconfirmed `Launch` action is gone.

## Tests

`app.rs`:
- Activate-on-Launcher sets `pending_action` and does **not** spawn.
- Enter while pending executes (spawns via `FakeRunner`), clears pending, and
  requests refresh.
- Esc while pending cancels (no spawn), clears pending, does not quit.
- A non-confirm key (e.g. `Down`) while pending is swallowed (pending unchanged,
  no navigation, no spawn).

`launcher.rs` / `app.rs`:
- `PendingAction::preview` returns the resolved command for a config-pinned tool
  and `"No launch command yet"` for a feed-only tool.

## Files touched

- `crates/rexops-tui/src/app.rs` — `PendingAction`, state field, gate, request
  path, tests.
- `crates/rexops-tui/src/launcher.rs` — `resolve_command` → `pub(crate)`.
- `crates/rexops-tui/src/action.rs` — remove dead `Launch` variant.
- `crates/rexops-tui/src/ui.rs` — `render_confirm_popup` + status hint.
- `crates/rexops-tui/src/screens/launchpad.rs` — hint line wording.
