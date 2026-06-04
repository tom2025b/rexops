# RexOps — Starting State & Launcher Handover

> **Purpose of this file.** A clean, factual snapshot of where RexOps actually is
> right now, plus the agreed vision, rules, and a concrete starting point for the
> next AI agent (likely in a fresh context window). Everything in the "Current
> State" section was verified by reading the repo on **2026-06-03**. The
> "Launcher Vision" section is forward-looking design we agreed on — it is
> labelled as such and is **not** yet built.

---

## 0. TL;DR for the next agent

- RexOps is a **read-only ops cockpit** (TUI + CLI) that summarizes the health and
  inventory of specialist tools (Bulwark, ScriptVault, ToolFoundry) via thin
  **adapters**. Phase 1 foundation is complete and green on all gates.
- The **Launcher is now in staged implementation**. Stage 2 is complete:
  RexOps has an unbound launch intent plus terminal suspend/run/restore plumbing
  that can hand off to `scriptvault` and return cleanly.
- **Core rule, non-negotiable:**
  > **RexOps may launch and summarize specialist tools. RexOps must not absorb
  > specialist tools.**
- Build it in **small, user-tested stages** with **checkpoint branches** and an
  explicit **commit protocol** (see §4). One stage at a time. Stop after each.
- Start at §6 ("Next AI agent starting point").

---

## Current Status

- Active branch: `launcher/phase`
- Latest completed stage: **Stage 2 — Terminal Handoff Stub**
- Latest checkpoint branch: `launcher/stage-2-terminal-handoff` (created during
  the Stage 2 commit protocol)
- Launcher state: `Action::Launch` is still unbound; when triggered internally,
  it requests a hardcoded `scriptvault` foreground launch to prove terminal
  handoff and restore.
- Next stage: Stage 3 has not started.

---

## 1. Current State (verified — what actually exists today)

### 1.1 Repository

- Path: `/home/tom/projects/rexops`
- Git: branch `main`, in sync with `origin/main`
  (`github.com/tom2025b/rexops`). Latest commit:
  `88230df Complete RexOps Phase 1 foundation, TUI, and rexops-app shared layer`.
- Rust workspace, `resolver = "2"`, `edition = "2021"`, `rust-version = "1.75"`,
  `license = "MIT OR Apache-2.0"`.
- Docs on disk: `docs/ARCHITECTURE.md`, `docs/ROADMAP.md`,
  `docs/ERROR_HANDLING.md`, `docs/TUI_DESIGN.md`. Example config:
  `examples/config.yaml`. Top-level `README.md` is current and detailed.

> ⚠️ **Repo hygiene note (not mine to fix without asking):** there is a stray
> directory in the repo root literally named `› Backup destination (e.g. ` owned
> by `root`. It looks like fallout from a `~/bin/backup-home` run where an empty/
> unquoted prompt variable was passed to a path. It is **not** part of RexOps and
> should probably be removed (`sudo rm -rf` — it is root-owned), but I left it
> untouched. Flagging it so it doesn't get committed by accident.

### 1.2 Workspace crates (5)

All five compile and pass the gates. Boundaries are strict and deliberate.

| Crate | Role | Key contents (verified) |
|-------|------|-------------------------|
| **rexops-core** | Pure domain models + transforms. No UI, no process exec. | `models.rs` (`OpsSnapshot`, `RiskSummary`, health types), `ids.rs` (`ToolId`, `AdapterId` newtypes), `config.rs` (`AppConfig`), `registry.rs` (`AdapterRegistry`/`ToolRegistry`), `error.rs`. |
| **rexops-adapters** | Synchronous `Adapter` trait + concrete adapters. The only place that shells out. Graceful degradation; never panics the caller. | `adapter.rs` (trait), `bulwark.rs` (**real**, parses `bulwark inspect scan` JSON; fixture-backed), `system.rs` (lightweight real), `scriptvault.rs` (**stub/demo data**), `toolfoundry.rs` (**stub/demo data**), `exec.rs` (`run_optional`, `DEFAULT_TIMEOUT`), `types.rs` (`AdapterHealth`, `AdapterOutput<T>`), `error.rs`. |
| **rexops-app** | Shared thin orchestration. The single (de-duplicated) implementation used by **both** CLI and TUI. No UI. | `config.rs` (`load_config`), `snapshot.rs` (`build_snapshot`), `lib.rs` (`build_adapter_registry`). |
| **rexops-cli** | `rexops` binary. Thin clap shell → delegates to rexops-app. | `main.rs`. Commands: `status`, `adapters`; `--json` / human output. |
| **rexops-tui** | Keyboard-first ratatui TUI. Thin: consumes core types, calls rexops-app. | See §1.3. |

Dependency direction (no cycles):
`core` ← `adapters` ← `app` ← {`cli`, `tui`}.
(Note: `core` currently has a path dep on `adapters` for shared types — fine
inside the workspace.)

### 1.3 TUI — current screens & structure (verified)

Crate `rexops-tui`. Elm-like architecture: raw key → `Action` (intent) →
`App::on_action` → state change → `ui.rs` renders the current screen.

- **Files:** `main.rs` (terminal setup, event/draw loop, restore + panic hook),
  `app.rs` (`App` struct + `Screen` enum + `on_action`), `action.rs` (`Action`
  enum), `event.rs` (event wrapper), `keymap.rs` (`KeyEvent → Option<Action>`),
  `theme.rs`, `ui.rs` (outer layout + dispatch), `screens/` (one file per
  screen), `widgets/` (`health_badge.rs`, `adapter_item.rs`, `log_line.rs`).

- **Screens (`Screen` enum):** `Dashboard` (1), `Adapters` (2, navigable list +
  live type-to-filter), `System` (3), `Scripts` (4, ScriptVault demo data with ★
  favorites), `Tools` (5, ToolFoundry demo data: owner / health / symlinks).

- **`Action` enum today (exhaustive):** `Quit`, `Refresh`, `ToggleHelp`,
  `SwitchToDashboard`, `SwitchToAdapters`, `SwitchToSystem`, `SwitchToScripts`,
  `SwitchToTools`, `Up`, `Down`, `Activate`, `Cancel`, `InputChar(char)`,
  `Backspace`.
  **→ There is no `Launch` / `Run` / `Invoke` action. `Activate` (Enter) only
  adds a note on the Adapters screen. This is the gap the Launcher fills.**

- **Keys today:** `q`/`Esc`/`Ctrl-C` quit (always, even mid-refresh),
  `r` refresh (non-blocking: spawns a std thread that calls
  `rexops_app::build_snapshot` and returns via `mpsc` — UI never freezes),
  `?`/`h` help, `1`–`5` switch screens, `j`/`k` + arrows navigate the Adapters
  list, type-to-filter on Adapters (`InputChar`/`Backspace`, `Esc` clears).

- **Data model:** everything rendered derives from a single `OpsSnapshot`
  (`app.snapshot`). All other `App` fields are transient UI state.

### 1.4 The adapters' current relationship to the specialist tools

- **Bulwark** — real adapter; runs and parses `bulwark inspect scan --format
  json` (fixture: `crates/rexops-adapters/fixtures/bulwark/scan_sample.json`,
  marked PROVISIONAL).
- **ScriptVault** — **stub only.** `ScriptVaultAdapter::check_available()`
  returns `true` unconditionally; `info()` returns hard-coded demo scripts
  (`deploy-prod.sh` ★, `backup-db.sh`, `cleanup-logs.py`). No real `scriptvault`
  binary is probed yet.
- **ToolFoundry** — **stub only** (demo tools / owners / symlinks).
- **System** — lightweight real (hostname/kernel/uptime/disk).

**Implication for the Launcher:** the adapters are, today, *summarizers*. They
read/observe; they do not *invoke*. The Launcher is genuinely new surface area.
(The real ScriptVault project lives separately at `/home/tom/projects/scriptvault`
— binary `scriptvault` — and is the first natural launch target.)

---

## 2. The Launcher Vision & Philosophy (agreed — NOT yet built)

### 2.1 The one rule that governs everything

> **RexOps may launch and summarize specialist tools.
> RexOps must not absorb specialist tools.**

RexOps is the **cockpit**, not the engine. It is the single pane of glass from
which you *see* the state of your tooling surface and *hand off* into the right
specialist — then come back. It never reimplements what a specialist already
does well.

### 2.2 What "launch" means here

From the RexOps TUI, the user selects a specialist (or a specific item it
surfaced — a script, a tool, a scan) and RexOps **invokes that tool's own
interface**, e.g.:

- Open **ScriptVault's** TUI/CLI (its own search, favorites, run, edit).
- Open **ToolFoundry** for a selected tool's lifecycle/ownership.
- Re-run a **Bulwark** scan and view it in Bulwark's own surface.

RexOps suspends its terminal, runs the child with the real TTY, and on exit
restores its own screen and (optionally) refreshes its snapshot. This mirrors
the suspend/run/restore pattern already proven in the ScriptVault TUI for
editor/run actions.

### 2.3 What the Launcher must NOT become (the "do not absorb" line)

- ❌ Do **not** reimplement ScriptVault search/run/edit inside RexOps.
- ❌ Do **not** reimplement Bulwark's policy engine or ToolFoundry's lifecycle.
- ❌ Do **not** turn adapters into mutating engines that duplicate specialist
  logic.
- ✅ **Do** observe, summarize, and **hand off**. RexOps owns *orchestration and
  presentation*; specialists own *the work*.

If a feature request would make RexOps a better version of a specialist tool,
that is the signal to **stop** and put the feature in the specialist instead.

### 2.4 Design principles (carried from the repo's existing philosophy)

- **Keep It Simple** — smallest thing that works; no speculative abstraction.
- **Read-only by default; mutation/invocation is explicit and confirmed.**
- **Graceful degradation** — a missing/unavailable specialist must never crash
  RexOps; the launch entry simply reports "unavailable" and offers a hint.
- **Non-blocking UI** — never freeze the draw loop; long work goes to a thread
  (refresh already does this). A *launch* instead suspends the TUI, runs the
  child on the real terminal, then restores — clean terminal restore is
  mandatory (panic hook + explicit restore already exist).
- **Strict crate boundaries** — invocation policy belongs in the adapter/app
  layer, not smeared into the renderer. The TUI emits an *intent*; the
  app/adapter layer decides *how* to invoke.

> **Open design points the next agent should settle with the user (not assumed
> here):** the exact launch keybinding(s) and which screens expose them; whether
> launching is per-adapter (open the whole tool) or per-item (open the tool
> focused on the selected script/tool/scan); how the child command is resolved
> (PATH lookup vs. config-declared path in `examples/config.yaml`); and what
> RexOps does on return (auto-refresh vs. nothing). These were *not* pinned down
> and must not be invented — confirm them in Stage 0/1.

---

## 3. Strict Staged Approach (how we build the Launcher)

We build in **small, safe, individually testable stages**. This is the same
discipline already in use on the ScriptVault polish work and it is mandatory
here.

**Rules:**

1. **One stage at a time. Only.** Do not start the next stage until the user
   explicitly says to proceed.
2. **After finishing a stage, STOP completely.** Show the exact changes (diffs or
   full functions). Do not begin the next stage on your own.
3. **The user tests each stage in a real terminal** before it is committed.
4. **No big monolithic changes.** If a stage feels large, split it.
5. **Stay grounded** — extend the existing crates/patterns; do not invent
   structure that isn't there. New code matches the repo's style (small files
   <300 LOC / prefer <200, educational comments, Learning Notes, `Result<T,
   CrateError>`, zero `unwrap`/`expect` in non-test lib code).

---

## Progress Log

### Stage 0 — Launcher Design Decisions

- **What was done:** Added a concise design-decision document that records the
  Launcher questions still open before behavior is expanded.
- **Key files changed:** `docs/LAUNCHER_DESIGN.md`
- **Date:** 2026-06-04

### Stage 1 — Launch Action Stub

- **What was done:** Added an unbound `Action::Launch` intent and a minimal
  `App::on_action` handler that logs a no-op launch request. No keybinding was
  added.
- **Key files changed:** `crates/rexops-tui/src/action.rs`,
  `crates/rexops-tui/src/app.rs`
- **Date:** 2026-06-04

### Stage 2 — Terminal Handoff Stub

- **What was done:** Added a foreground terminal handoff path that can suspend
  the RexOps TUI, run `scriptvault` on the real terminal, then restore the TUI
  and log the child result. No keybinding was added.
- **Key files changed:** `crates/rexops-tui/src/app.rs`,
  `crates/rexops-tui/src/main.rs`
- **Date:** 2026-06-04

---

## 4. Commit Protocol (per stage)

**Before every commit**, run the full RexOps quality gate (all four must pass):

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo build --all
```

When the user says **"commit stage X"**, do this in order:

1. Create a checkpoint branch named `launcher/stage-X-<short-name>`
   (example: `launcher/stage-1-launch-action`).
2. Commit with a clear message, e.g.
   `launcher: stage 1 – add Launch action + suspend/restore plumbing`.
3. Confirm the branch and commit were created.
4. Then ask: **"Stage X committed. Ready for Stage X+1?"**

**Branch model (learned the hard way on the ScriptVault pass — apply it here):**

- Do the staged work on a single integration branch, e.g.
  `launcher/phase` (create it off `main`).
- Each `commit stage X` makes a checkpoint branch **from the integration
  branch's current tip**, so stages **stack** (Stage 2 contains Stage 1).
- If a checkpoint branch is created and the integration branch is left behind,
  **fast-forward the integration branch up to the checkpoint** before starting
  the next stage — otherwise stages silently fork and later need a conflict-prone
  merge.
- Always verify ancestry before any destructive git op
  (`git merge-base --is-ancestor`); use `git branch -d` (safe), never force-push.

**Commit-message honesty:** if a gate is red for a *pre-existing, unrelated*
reason (e.g. a clippy lint from a toolchain bump in code this stage didn't
touch), say so in the commit body and confirm the **stage's own** code is clean
and tests pass. Do not silently paper over a red gate, and do not "fix" unrelated
lint without the user's say-so.

---

## 5. Quality Bar (repo-wide, non-negotiable)

- The 4 gate commands above stay green after every change.
- Files well under 300 lines (prefer <200); split when approaching the limit.
- Every fallible public fn returns `Result<T, CrateError>`.
- Zero `unwrap()` / `expect()` in non-test library code (`#![deny]`).
- Tests written alongside implementation (happy path **and** error paths).
- Educational comments throughout + a "Learning Notes" block at file bottom
  (this repo's established convention — match it).

---

## 6. Next AI agent — starting point

**You are picking up RexOps to build the Launcher.** Do this:

1. **Orient (don't trust this doc blindly — verify):**
   - `git -C /home/tom/projects/rexops status` (expect clean `main`).
   - Skim `README.md`, `docs/ARCHITECTURE.md`, `docs/ROADMAP.md`,
     `docs/TUI_DESIGN.md`.
   - Read `crates/rexops-tui/src/{action.rs,app.rs,keymap.rs,main.rs}` — confirm
     there is still **no** launch/invoke action (the seam you'll add).
   - Read `crates/rexops-adapters/src/{adapter.rs,exec.rs,scriptvault.rs}` —
     `exec.rs` already knows how to run child processes (`run_optional`); the
     launch path will be a *foreground/suspend* sibling to that, not a captured
     one.

2. **Confirm the open design points in §2.4 with the user** before coding
   (keybinding, per-tool vs per-item, command resolution, post-return behavior).
   Do **not** assume them.

3. **Set up the branch:** create `launcher/phase` off `main`. Work there. Use the
   §4 protocol for every stage.

4. **Suggested first stages (small; confirm/adjust with the user — these are a
   proposal, not a fixed plan):**
   - **Stage 0 (optional):** record decisions from step 2 in `docs/` (a short
     `LAUNCHER_DESIGN.md`). No behavior change.
   - **Stage 1:** add a `Launch` intent to the `Action` enum + a keybinding in
     `keymap.rs`, wired to a no-op/stub in `on_action` that just logs an event.
     Pure plumbing; fully testable; nothing actually spawns yet.
   - **Stage 2:** implement suspend-terminal → run child → restore-terminal in
     `main.rs`/app layer (mirror ScriptVault's editor/run suspend pattern;
     restore the screen + clear on return). Launch a trivial known command first
     to prove the TTY handoff and clean restore.
   - **Stage 3:** resolve the real target (start with ScriptVault: open its
     TUI/CLI). Honor graceful degradation when the binary is absent
     (entry shows "unavailable" + hint; never crash).
   - **Stage 4+:** per-item launches (selected script/tool/scan), config-declared
     tool paths, optional auto-refresh on return.

5. **Always honor the core rule (§2.1).** When in doubt, RexOps hands off; it
   does not absorb. If a requested feature would duplicate a specialist, stop and
   raise it.

---

## 7. Cross-project note (ScriptVault — paused, not abandoned)

A separate visual-polish effort is in flight on the **ScriptVault** repo at
`/home/tom/projects/scriptvault` (branch family `polish/…`). It is **paused** to
do this RexOps handover. That work has its own staged plan and is unrelated to
RexOps except that **ScriptVault is the Launcher's first real launch target**.
Nothing in this file requires touching the ScriptVault repo.

---

*Generated 2026-06-03 from a direct read of the repository. The "Current State"
section is factual as of commit `88230df`. The "Launcher Vision" is agreed design
and is explicitly not-yet-implemented; verify against the live code before
building.*
