# rexops-tui Module Layout

Layout after the TUI god-file split (2026-06). The old `app.rs` (1,505 lines),
`ui.rs` (426 lines), and `jobs.rs` were broken into the hierarchy below. The
largest remaining production file is `jobs/process.rs` (354 lines), which owns
OS process mechanics and is intentionally left whole.

## Tree

```text
crates/rexops-tui/src/
  main.rs        thin entry point: Tui guard, config, App construction, theme
  runtime.rs     the event loop: draw, drain refreshes, poll jobs + input, dispatch

  app/           application state and transitions
    state.rs       App struct, initial state, refresh + activity log/toast helpers
    navigation.rs  Screen enum, adapter filtering and selection helpers
    update.rs      Action -> state transition
    tests/         app behavior tests split by feature area

  input/         raw terminal input translation (no state mutation)
    action.rs      high-level Action enum
    keymap.rs      crossterm polling (next_event) + key -> Action mapping

  commands/      command palette and confirmation flow (no rendering, no spawning)
    palette.rs     palette command catalog and query filtering
    dispatch.rs    PendingAction model + palette/confirm dispatch behavior

  tools/         tool catalog and launch behavior
    catalog.rs     static launcher catalog, RunMode policy, streaming metadata
    launcher.rs    command resolution and foreground child launch reporting

  jobs/          background job lifecycle
    process.rs     child spawning, reader threads, cancellation, draining
    manager.rs     one-job-at-a-time policy, polling, history/output caps,
                   JobRecord/LastOutcome and toast mapping

  screens/       one render file per top-level pane (render from App state only)
    dashboard.rs adapters.rs system.rs scripts.rs tools.rs launchpad.rs jobs.rs

  ui/            render-only chrome
    layout.rs      frame layout dispatcher + header
    palette.rs     palette/help/confirm overlay popups
    status_bar.rs  footer status bar + key hints, toast/status composition
    widgets/       health badge, adapter rows, log lines, health_to_suite
    tests.rs       chrome regression tests
```

## Boundaries

- `app/` holds state and transitions; it does not render and only spawns
  foreground tools through `tools/`.
- `input/` translates terminal events to `Action`s; it never mutates state and
  knows nothing about the tool catalog.
- `commands/` arms confirmation-gated actions but does not render modals or own
  spawning internals.
- `tools/` owns catalog data and run-mode policy. `bulwark` is
  `RunMode::Foreground` (it opens its own TUI); background streamability is
  catalog metadata, not app logic.
- `jobs/process.rs` owns OS mechanics; `jobs/manager.rs` owns TUI job state
  policy. Job rendering lives in `screens/jobs.rs`.
- `screens/` and `ui/` are render-only: no command resolution, no spawning, no
  state mutation during render.

## Where the god files went

| Old file | Now |
|---|---|
| `app.rs` | `app/{state,navigation,update}.rs`, `commands/dispatch.rs`, `jobs/manager.rs` |
| `ui.rs` | `ui/{layout,palette,status_bar}.rs` |
| `jobs.rs` | `jobs/{process,manager}.rs` |
| `launcher.rs` | `tools/{catalog,launcher}.rs` |
| `palette.rs` | `commands/{palette,dispatch}.rs` |
| `action.rs`, `event.rs`, `keymap.rs` | `input/{action,keymap}.rs` |
| `health.rs`, `widgets/*` | `ui/widgets/mod.rs` |

## Verification

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked   # 105 tests, 55 in rexops-tui
```
