# RexOps TUI Design (Phase 2 start)

Minimal, keyboard-first TUI using `ratatui` + `crossterm`. Built on top of `rexops-core` (for `OpsSnapshot`, health, registries) and `rexops-adapters` (for live probes). Never duplicates domain logic.

## Goals for Initial Shell + Dashboard
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
│   └── tools.rs       # render_tools (ToolFoundry ownership/lifecycle demo)
└── (widgets/ for future reusable components)
```

Keep initial files small. Split further only when a file approaches 200-250 LOC.

## High-Level Architecture (Non-Blocking)
- Main thread owns the `ratatui::Terminal<CrosstermBackend>` and draws at ~10-15 fps or on events.
- Key events read via `crossterm::event::poll(timeout)` + `read()` (short timeout so we can also `try_recv`).
- On user action that needs I/O ("r" refresh):
  - Set `app.refreshing = true`
  - `spawn` a std thread (cheap for our use case)
  - Thread does the work: construct `BulwarkAdapter`, call health/version, build `OpsSnapshot` (same pattern as cli)
  - Send the result over a `std::sync::mpsc::channel` (or a small `Message` enum later)
- Main loop: after draw or on tick, `while let Ok(msg) = rx.try_recv() { app.apply(msg); }`
- This guarantees the draw loop stays responsive even if a probe hits the 30s timeout.

No tokio/async in the first TUI iteration (matches the "keep adapters phase simple" spirit and avoids pulling in a runtime until we have a clear need for many concurrent things).

## Dashboard Screen (Initial View)
Single view for Phase 2 start. Layout (top to bottom):

1. Title / header: "RexOps" + current timestamp or "last refresh"
2. Adapters section:
   - Table or list: Name | Health (colored) | Version | Notes
   - Color: Green=Healthy, Yellow=Degraded, Red=Unavailable, Gray=Unknown
   - Symbol prefix: ✓ ! ✗ ?
3. Risk summary (from snapshot.risk): counts by severity + should_block flag
4. Notes / messages area (scrollable later): adapter notes + "Refreshing..." + errors
5. Status bar (bottom, full width):
   - Left: "RexOps TUI  |  q quit  r refresh  ? help"
   - Right: "adapters: 1/3 healthy" or similar
   - Center or overlay: "Refreshing..." spinner text when active

When no adapters registered or all unavailable:
- Prominent banner: "No healthy adapters detected."
- Helpful text: "Press 'r' to retry. Run `rexops status` from CLI for details. Install bulwark with `cargo install bulwark-inspect` if needed."

## Keymap (Start Small, Consistent)
- `q`, `Esc`, `Ctrl-C` — Quit (always, even while refreshing)
- `r` — Refresh / re-probe adapters (idempotent; ignored while already refreshing)
- `?` or `h` — Toggle simple help text in the messages area
- `1` — Switch to Dashboard screen
- `2` — Switch to Adapters screen (keyboard selectable list + side detail/preview pane)
- `3` — Switch to System screen (structured SystemInfo from snapshot: hostname, kernel, uptime, disk + health)
- `4` — Switch to Scripts screen (structured ScriptVaultInfo: list of scripts with favorites ★, total counts)
- `5` — Switch to Tools screen (structured ToolFoundryInfo: list of tools with owner, per-tool health, symlink status)
- In Adapters: j/k or up/down arrows to move selection, enter to activate (surfaces in notes + updates detail)
- Live filter: type printable chars (non-command letters) to filter the adapters list live; backspace edits; esc clears filter (or quits if empty)
- Status bar shows context-sensitive hints per screen.
- `?` / `h` shows a nice centered popup help overlay (press again to close); also shows in messages.
- Future: more screens (reports, jobs), mouse support, explicit '/' focus filter, etc. (Action + keymap + screens separation makes extension easy). Tools screen (5) added as part of the ToolFoundryAdapter increment.

All keys are handled in one place (event.rs or app.rs) so behavior is uniform.

## State (App)
```rust
pub struct App {
    pub snapshot: OpsSnapshot,
    pub refreshing: bool,
    pub last_message: Option<String>,   // "refreshed at ...", errors, etc.
    // later: current_screen: Screen, filter: String, selected: usize, ...
}
```

`OpsSnapshot` comes from core and is the only "live" data. UI derives everything else.

## Theming / Styling (Minimal)
- Use ratatui's `Style`, `Color::Green` etc. directly at first.
- Later (if grows): `theme.rs` with a `Theme` struct + helpers like `health_style(h: AdapterHealth) -> Style`.
- Borders: `Block::bordered().title("Adapters")`
- Widgets extracted to `widgets/` (HealthBadge, AdapterItem, LogLine) for reuse across screens. Compose `Table`, `Paragraph`, `Gauge`, `Clear` for overlays. See `src/widgets/`.

## Error & Degradation Handling
- Adapter calls never panic the TUI.
- Timeouts, missing binaries → reflected in `AdapterHealth` + note in snapshot.
- Draw errors (very rare) → log to stderr after restore, or show in last_message.
- On panic in app code: best-effort terminal restore via `std::panic::set_hook`.

## Startup Flow
1. Parse any future args (e.g. --config) — none for v0.
2. Create channel (tx, rx).
3. Setup terminal (enable mouse? no for keyboard-first; raw, alternate screen).
4. Create initial `App { snapshot: OpsSnapshot::new(), refreshing: false, ... }`
5. Optional: kick off an initial background refresh on start (or wait for user 'r').
6. Enter event/draw loop.
7. On exit: restore terminal, then propagate any error.

## Testing Strategy (Light for UI)
- Unit test pure functions: health color mapping, snapshot merging (already in core).
- No heavy integration UI tests in first pass (hard without a real terminal or snapshot testing libs).
- Manual dogfood: run, press r repeatedly, kill bulwark mid-run, resize, etc.
- The four cargo gates still apply (clippy will be noisy on ratatui; allow a few targeted pedantic lints in ui code like adapters did).

## Future Increments (Not for This Step)
- Multiple screens (Reports, Jobs, detail panes). Tools (5) + Scripts (4) + System (3) + Adapters (2) now implemented.
- Live auto-refresh ticker (background thread that periodically sends new snapshots)
- Search/filter, vim keys, mouse support (optional)
- Help overlay as real popup widget
- Theming + config driven colors
- Extract common widgets (HealthBadge, RiskBar)
- Use `rexops-app` layer (done): snapshot building is now shared; CLI + TUI are thin (see the rexops-app increment).

## Non-Goals (Keep It Simple)
- No webview, no ratatui + tokio full async executor yet.
- No persistence of UI state across runs.
- No fancy animations or 60fps.

See ARCHITECTURE.md for how TUI fits: it consumes core types and calls adapters; it does not own data or execution policy.

Start small: get a clean dashboard + reliable non-blocking refresh + perfect quit behavior. Then iterate.
