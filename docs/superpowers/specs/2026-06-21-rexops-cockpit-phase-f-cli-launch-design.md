# RexOps Cockpit — Phase F: CLI parity (`rexops launch`)

2026-06-21. The final phase of the cockpit redesign (roadmap §9). `rexops
components` already shipped in Phase A; this phase adds the headline missing
surface: a **gated `rexops launch <tool>`** that mirrors the TUI's
confirm-before-run flow, so anything launchable from the cockpit is launchable
from the CLI without the two paths diverging.

## Goal

`rexops launch <tool>` resolves a component's launch command exactly as the TUI
does, confirms before running, and execs it — with scriptable escapes.

## Why a resolver move

`resolve_launch_command` + `LaunchCommand` currently live in `rexops-tui`
(`tools/launcher.rs`). The CLI must not depend on the TUI crate to launch. Both
`rexops-cli` and `rexops-tui` already depend on `rexops-app`, and the resolver is
pure (registry + `AppConfig` + `std::process::Command` for `which`), so it moves
cleanly with no new dependency edges or cycles.

## Architecture

```
rexops-app::launch   ← resolve_launch_command + LaunchCommand (pure, moved here)
      ^         ^
rexops-cli    rexops-tui (re-exports resolve_launch_command/LaunchCommand;
   |            unchanged call sites)
   └── Commands::Launch → resolve → confirm gate → ForegroundRunner
```

- **Move** into a new `rexops-app/src/launch.rs`: `LaunchCommand`,
  `resolve_launch_command`, and its private helpers (`resolve_command`,
  `command_from_path`, `command_from_config`). It reads launch args from the
  registry via `rexops_core::component_by_id(..).launch` (no dependency on the
  tui `catalog` shim — call core directly).
- **`rexops-tui`** re-exports `pub use rexops_app::launch::{resolve_launch_command,
  LaunchCommand};` from `tools/launcher.rs` (or `tools/mod.rs`) so every existing
  tui call site and test compiles unchanged. The `ForegroundRunner`/`ChildExit`
  runner trait and the TUI's modal stay in rexops-tui (UI concerns).
- **`rexops-cli`** gains `Commands::Launch { tool, yes, dry_run }`, resolves via
  `rexops_app::launch`, runs the gate, and execs with a foreground spawn
  (`std::process::Command::status`) — the CLI owns a terminal already, so it does
  not need the tui runner; a tiny inline foreground exec suffices.

## The gate (parity with the TUI confirm modal)

The TUI arms a tool then routes through a confirm modal before running. The CLI
mirrors this:

```
rexops launch <tool>
  1. resolve_launch_command(tool, config)
       None  → eprintln "rexops: '<tool>' is not launchable (not on PATH or
               no launch spec)"; exit 1.   (mirrors arm_tool refusing unavailable)
  2. confirm:
       --dry-run → print "would run: <program> <args…>"; exit 0; run nothing.
       --yes     → skip prompt.
       else      → prompt "Run: <program> <args…>  [y/N] " on stderr; read a line
                   from stdin; proceed only on y/Y/yes. Anything else → "aborted",
                   exit 0 (not an error — the user declined).
                   If stdin is not a TTY and --yes was not given → refuse:
                   "refusing to launch without confirmation; pass --yes"; exit 1.
  3. exec foreground: Command::new(program).args(args).status();
       propagate the child's exit code (success → 0, non-zero → that code).
```

`--dry-run` reuses the same resolved argv that would run (no preview/run
divergence, CR-2). No `RunMode` branching is needed for v1: every launch is
gated identically, which is the safe default and matches the TUI's "every armed
launch confirms" behaviour. (`RunMode::Background` tools are out of scope for the
CLI in v1 — the launchable set today is all foreground TUIs.)

## CLI shape

```
rexops launch <TOOL> [--yes] [--dry-run]
  <TOOL>      component id (e.g. bulwark, proto, scriptvault, toolfoundry, pulse)
  --yes, -y   skip the confirmation prompt (for scripts)
  --dry-run   print the exact command and exit without running
```

`rexops launch --help` documents the above. README gains a `launch` line.

## Error handling

- Unknown/unlaunchable tool → exit 1 with a clear message (one source: resolver
  returns `None` for not-on-PATH, no-launch-spec, or disabled adapter).
- User declines at the prompt → exit 0, "aborted" (declining is not failure).
- Non-TTY without `--yes` → exit 1 (don't hang waiting for input; don't run blind).
- Child process exit code is propagated so scripts can react.

## Testing (TDD)

Pure resolver tests move with the code into `rexops-app` (bulwark/proto resolve;
disabled → None; missing → None). New CLI-gate tests cover the decision logic by
factoring the gate into a pure function:

```
fn decide(resolved: Option<LaunchCommand>, yes: bool, dry_run: bool,
          is_tty: bool, answer: Option<&str>) -> GateOutcome
  GateOutcome ∈ { Refuse(msg, code), DryRun(cmd), Run(cmd), Aborted }
```

Tests: unlaunchable→Refuse(1); dry_run→DryRun (no run); yes→Run; tty+"y"→Run;
tty+"n"→Aborted; non-tty + !yes→Refuse(1). The actual exec + stdin read are thin
wrappers around `decide`, kept out of unit tests.

## Scope / non-goals

- No `RunMode::Background` / detached launches on the CLI in v1.
- No new binaries, wrappers, or aliases (bare-binary preference).
- The 4 still-`Planned` tools (tripwire/rewind/rex-check/rex-forge) are Phase E
  tail, not Phase F — `launch` works for them automatically once they gain a
  `LaunchSpec`.

## Quality gates

`cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy
--workspace --all-targets -- -D warnings`, `cargo fmt --all --check` green at
every commit. One `feat(rexops): … (Phase F)` impl-commit on a branch → PR →
merge, matching the A–E cadence. The pre-existing red "hub schema" CI check
(fixture `schema_version` 3 vs umbrella schema 4) is bumped to v4 in this branch
so the PR lands on fully-green CI.
