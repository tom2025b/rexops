# RexOps TUI Design

Minimal, keyboard-first TUI using `ratatui` + `crossterm`. It calls `rexops-app` for config loading and snapshot refresh, uses `rexops-core` models for state, and never duplicates domain logic.

## Goals
- Fast startup (< 100ms ideal when no work).
- Never freezes the UI during adapter calls (which can take seconds or timeout).
- Excellent empty, error, and degraded states (banners, not crashes or blank screens).
- Clear visual language for `AdapterHealth` (colors + symbols).
- Consistent, discoverable keybindings.
- `q` to quit always works; terminal restored cleanly on any exit/panic.

## Crate Layout (inside crates/rexops-tui/)
```
src/
├── main.rs          # entry: setup terminal, run loop, restore
├── app.rs           # App struct + Screen enum + on_action
├── action.rs        # Action enum (Quit, Refresh, ToggleHelp, SwitchTo*)
├── event.rs         # Event enum + next_event(timeout) wrapper
├── keymap.rs        # handle_key(KeyEvent) -> Option<Action>
├── theme.rs         # health_style, title_style, border_style, etc.
├── ui.rs            # thin outer layout + dispatch to current screen
├── screens/
│   ├── mod.rs
│   ├── dashboard.rs
│   ├── adapters.rs
│   ├── system.rs
│   ├── scripts.rs
│   └── tools.rs       # render_tools
└── widgets/
```

Keep files small. Split further only when a file approaches 200-250 LOC.

## High-Level Architecture (Non-Blocking)
- Main thread owns the `ratatui::Terminal<CrosstermBackend>` and draws at ~10-15 fps or on events.
- Key events read via `crossterm::event::poll(timeout)` + `read()` (short timeout so we can also `try_recv`).
- On user action that needs I/O ("r" refresh):
  - Set `app.refreshing = true`
  - `spawn` a std thread (cheap for our use case)
  - Thread does the work through `rexops_app::build_snapshot`
  - Send the result over a `std::sync::mpsc::channel`
- Main loop: after draw or on tick, `while let Ok(msg) = rx.try_recv() { app.apply(msg); }`
- This guarantees the draw loop stays responsive even if a probe hits the 30s timeout.

No tokio/async in the TUI; the current workload is covered by a short-lived refresh thread.

## Dashboard Screen
Layout (top to bottom):

1. Search / filter bar (`suite_ui::SearchBar`): a one-line live filter for the
   adapters table below it. Shows a dim placeholder when empty and the query +
   match count once typing starts. Type to narrow; `esc` clears. Backed by the
   shared `App::filter` string (the same one the Adapters screen uses), so a
   query carries consistently between the two.
2. Adapters section (filtered by the search bar):
   - Table or list: Name | Health (colored) | Version | Notes
   - Color: Green=Healthy, Yellow=Degraded, Red=Unavailable, Gray=Unknown
   - Symbol prefix: ✓ ! ✗ ?
   - When a filter matches nothing, a single dim "(no matches)" row replaces the
     table body rather than leaving it blank.
3. Risk summary (from snapshot.risk): counts by severity + should_block flag
4. Notes / messages area: adapter notes + "Refreshing..." + errors
5. Status bar (bottom, full width):
   - Left: context-sensitive keybind hints, rendered by the shared
     `suite_ui::KeyHints` widget from a per-screen `(key, label)` slice (keys
     accented, labels dim). Confirm and palette modes replace the per-screen
     hints with their own while they own input.
   - Middle: the shared `suite_ui::StatusBar` job-status segment
     (running / done / failed / cancelled / idle)
   - Right: adapter availability badge ("adapters available" / "all unavailable")

When no adapters registered or all unavailable:
- Prominent banner: "No healthy adapters detected."
- Helpful text: "Press 'r' to retry. Run `rexops status` from CLI for details. Install bulwark with `cargo install bulwark-inspect` if needed."

## Launcher Screen
Three stacked rounded panes (`suite_ui::pane`):

1. **Launcher** header — a one-line dim subtitle ("Pick a tool with ↑/↓, then Enter…").
2. **Tools** list — one row per `CATALOG` entry, rendered by `render_launcher_row`:
   - The selected row shows the suite accent rail `▌` (`Theme::selected_rail`) and a
     tinted name (`Theme::selection`); other rows use a plain gutter. This is the
     same selection look the rest of the suite chrome uses, replacing the old ad-hoc
     "▶ " + bold prefix.
   - The tool name is padded to a fixed column (`NAME_COL`) so the health badges and
     tags line up.
   - A dim run-mode / availability tag ends each row: `· interactive` (foreground
     hand-over), `· streams` (background job), or `· disabled` when no command
     resolves — derived from `app::is_streamable` and
     `launcher::resolve_launch_command` (read-only; nothing is spawned to render
     the screen).
3. **Detail** — the full description of the currently selected tool (so a long one is
   never clipped in its row), plus enabled/disabled launch availability.

Navigation (↑/↓ over `app.selected_tool`) is presentation-only. Launch activation
is guarded in `arm_tool`: disabled rows log a status message instead of opening
the confirmation modal.

## Keymap (Start Small, Consistent)
- `q`, `Esc`, `Ctrl-C` — Quit (always, even while refreshing)
- `r` — Refresh / re-probe adapters (idempotent; ignored while already refreshing)
- `?` or `h` — Toggle simple help text in the messages area
- `1` — Switch to Dashboard screen
- `2` — Switch to Adapters screen (keyboard selectable list + side detail/preview pane)
- `3` — Switch to System screen (structured SystemInfo from snapshot: hostname, kernel, uptime, disk + health)
- `4` — Switch to Scripts screen (Workstate scripts section)
- `5` — Switch to Tools screen (Workstate tools section)
- In Adapters: j/k or up/down arrows to move selection, enter to activate (surfaces in notes + updates detail)
- Live filter: on the **Dashboard and Adapters** screens, type printable chars (non-command letters) to filter the adapters view live; backspace edits; esc clears the filter (or, when already empty, quits / goes back). Other screens leave those keys for their own bindings. The set of filter-accepting screens is defined in one place: `App::filter_screen()`.
- Status bar shows context-sensitive hints per screen via the shared `suite_ui::KeyHints` widget. The per-screen hint lists live in one place: `ui::screen_hints()`.
- `?` / `h` shows a nice centered popup help overlay (press again to close); also shows in messages.
All keys are handled in one place (event.rs or app.rs) so behavior is uniform.

## State (App)
```rust
pub struct App {
    pub snapshot: OpsSnapshot,
    pub refreshing: bool,
    pub last_message: Option<String>,   // "refreshed at ...", errors, etc.
    pub current_screen: Screen,
    pub filter: String,
    pub selected_adapter: usize,
}
```

`OpsSnapshot` comes from core and is the only "live" data. UI derives everything else.

## Theming / Styling
- Use `theme.rs` helpers for shared styles such as `health_style(h: AdapterHealth) -> Style`.
- Borders: `Block::bordered().title("Adapters")`
- Widgets extracted to `widgets/` (HealthBadge, AdapterItem, LogLine) for reuse across screens. Compose `Table`, `Paragraph`, `Gauge`, `Clear` for overlays. See `src/widgets/`.

## Error & Degradation Handling
- Adapter calls never panic the TUI.
- Timeouts, missing binaries → reflected in `AdapterHealth` + note in snapshot.
- Draw errors (very rare) → log to stderr after restore, or show in last_message.
- On panic in app code: best-effort terminal restore via `std::panic::set_hook`.

## Startup Flow
1. Load config through rexops-app.
2. Create channel (tx, rx).
3. Setup terminal (enable mouse? no for keyboard-first; raw, alternate screen).
4. Create initial `App { snapshot: OpsSnapshot::new(), refreshing: false, ... }`
5. Kick off refresh from user input.
6. Enter event/draw loop.
7. On exit: restore terminal, then propagate any error.

## Testing Strategy (Light for UI)
- Unit test pure functions: health color mapping, snapshot merging (already in core).
- Keep integration UI tests light unless snapshot testing is added.
- Manual dogfood: run, press r repeatedly, kill bulwark mid-run, resize, etc.
- The four cargo gates still apply (clippy will be noisy on ratatui; allow a few targeted pedantic lints in ui code like adapters did).

## Remaining Increments
- Reports, jobs, and detail panes if the snapshot model grows to need them.
- Live auto-refresh ticker (background thread that periodically sends new snapshots)
- Mouse support if it proves useful.
- Config-driven colors.

## Non-Goals (Keep It Simple)
- No webview, no ratatui + tokio full async executor yet.
- No persistence of UI state across runs.
- No fancy animations or 60fps.

See ARCHITECTURE.md for how TUI fits: it consumes core types and calls adapters; it does not own data or execution policy.

Start small: get a clean dashboard + reliable non-blocking refresh + perfect quit behavior. Then iterate.

---

## Cockpit Interactivity (Phase C)

> The original "Dashboard" (above) was replaced by the **cockpit** landing screen
> in Phase B (a grouped grid of component status cards) and made interactive in
> Phase C. The sections above are kept for history; this is the current behaviour
> of screen 1.

Screen 1 is an interactive cockpit:

- **Card focus:** `j`/`k` (and ↑/↓) move a highlighted card. Focus is keyed by
  component `id` (`App::selected_component`), so it survives a refresh that
  reorders or drops components; applying a snapshot auto-focuses the first card so
  the cockpit is immediately keyboard-navigable.
- **Letter hotkeys:** each card shows a dim `[a]` marker. Pressing that letter in
  Navigation mode arms the component through the **existing** confirm gate
  (`arm_tool → pending_action → confirm_pending`) — no separate launch path. The
  marker alphabet (`cockpit_nav::MARKER_ALPHABET`) is curated to exclude every
  bound nav key (`q r x j k h y n g`) and the digits `1`–`7`, so a card letter can
  never shadow a global key. Marker order and focus order both come from
  `cockpit_nav::cockpit_visit_order`, the single source of truth shared with the
  renderer — "the `a` you see is the `a` that fires."
- **Drill-down:** `g` (any focused card) or `Enter` (on a non-launchable card)
  opens `Screen::CockpitDetail` (`screens/cockpit_detail.rs`), which joins the
  static registry row (`component_by_id`: role, group, whether it launches) with
  the live `ComponentStatus` (health, vital). `Esc` backs out, keeping focus.
  `Enter` on a *launchable* focused card launches it (one-keypress launch); `g`
  is the universal drill key so read-only components are still inspectable.
- **Filter coexists:** the `/` filter still works on the cockpit; while filtering
  (Text mode) every printable key types into the filter, so the card letters never
  collide with filter input.

State note: `App` now also carries `selected_component: Option<String>` (the
focused card's id) alongside `selected_adapter`. Both Enter-arming and the letter
hotkeys funnel into the same `arm_tool` gate the Launcher and palette use, so the
three run surfaces can never disagree about what is launchable.

## Launch Data Source (Phase D)

There is **one** source of launch data: the `rexops_core::COMPONENTS` registry.
Each component's `LaunchSpec` (`run_mode` / `args` / `refresh_after`) plus its
`blurb` description is read by every run surface — `resolve_launch_command`, the
Launcher screen (screen 6), the command palette's `run <tool>` rows, the
launch-availability cache, and `is_streamable`/`refreshes_after`. The old
hand-maintained `tools/catalog.rs::CATALOG`/`ToolEntry` is gone; `tools::catalog`
is now a thin view over the registry (`tools::launchable()` =
`rexops_core::launchable_components()`). Adding a launchable tool is a one-row
registry change, and a guard test
(`launcher_list_is_exactly_the_registry_launchable_set`) locks the Launcher list
to the registry's `launch.is_some()` set so the two can never drift.

Phase D also promoted **ScriptVault** and **ToolFoundry** from data-only cards to
launchable `Live` components (feed health + freshness + a launch). Note this
widened what `live` means: live cards = the adapter roster *plus* feed-backed
launchables; the two cross-source rosters (`status`/`adapters`) are unchanged
because feeds are not adapters.
