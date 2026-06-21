# REXOPS COCKPIT REDESIGN — Design Document

> **Status:** Approved design (brainstorming output) — north star for incremental implementation.
> **Date:** 2026-06-20
> **Scope:** Turn RexOps from a 7-screen adapter inspector into the *cockpit of the entire Linux Ops Suite* — a beautiful central dashboard that summarizes live suite state and launches any tool with one keypress.
> **Author:** design session (grounded in the real `rexops-*` crates as of `f61adaa`).

---

## 0. The Metaphor (the contract this design serves)

The suite has one coherent mental model. RexOps is the **cockpit** — the seat you sit in to see and fly the whole thing.

```
  Workstate ............. brain        (state of record)
  RexOps ................ cockpit      ← THIS DOCUMENT
  Pulse ................. heartbeat    (liveness monitor)
  Rewind / Tripwire ..... black box + alarm
  ScriptVault ........... field tool   (scripts)
  ToolFoundry ........... field tool   (tool lifecycle)
  Bulwark ............... field tool   (security inspection)
  suite-ui .............. common face  (shared widgets)
  rex-check / RexDoctor . mechanics    (diagnostics)
  rex-forge ............. tool factory
```

A cockpit does three things and nothing else:

1. **Shows you the state of the aircraft at a glance** — every instrument visible without hunting.
2. **Tells you immediately when something is wrong** — a warning light you cannot miss.
3. **Lets you act with one motion** — flip a switch, don't fill out a form.

Everything below is in service of those three. If a proposed feature does not make the suite *more glanceable*, *more honest about failure*, or *faster to act on*, it does not belong in this redesign.

---

## 1. Where RexOps Is Today (honest baseline)

This redesign is an evolution of a real, working, well-tested codebase — not a rewrite. The current state:

**Architecture (workspace crates, all real today):**
- `rexops-core` — pure data. `OpsSnapshot`, `RiskSummary`, `AdapterHealth`, `Freshness`, `AdapterRegistry`, ids. No I/O.
- `rexops-adapters` — the only crate that does I/O. Thin `Adapter` trait, `AdapterOutput<T>`/`AdapterHealth`. Three real adapters: `bulwark` (live probe), `system` (host facts), `workstate` (v3 snapshot feed consumer). Denies `unwrap`/`expect`.
- `rexops-app` — orchestration. `build_snapshot` / `build_snapshot_with_piped` is the **single source of truth**, called by both front-ends. `build_adapter_registry` for the registry view.
- `rexops-cli` — thin clap shell: `status`, `adapters` (+`--json`); no subcommand → launches the TUI.
- `rexops-tui` — the interactive cockpit.

**The TUI today (7 screens, keyboard-first):**
- `1` Dashboard · `2` Adapters · `3` System · `4` Scripts · `5` Tools · `6` Launcher · `7` Jobs.
- Command palette (`Ctrl-P` / `:`), help overlay (`?`), toasts, an activity log, a confirm modal.
- **P8 confirmation gate**: launching a tool is a *modal, two-keypress* action (arm → confirm). `launch_tool` has exactly one caller, reachable only through the gate. This is the suite's safe-mutating-action layer and the redesign MUST preserve it.
- **Jobs subsystem**: background processes stream output into screen `7`, cancellable with `x` (kills the whole process group).
- **Launch catalog** (`tools/catalog.rs`): a static `CATALOG` of *launchable programs only* — today just `bulwark` and `proto`. Read-only data sections were deliberately removed from it.
- **`availability_tag`** (`app/state.rs`): the single 3-state truth (`streams`/`interactive` · `unavailable` · `disabled`) shared by every run surface, so the Launcher and palette can never disagree about what's runnable.

**Health model (already coherent — the redesign builds on it, doesn't replace it):**
- `AdapterHealth` = `Healthy` / `Degraded` / `Unavailable` / `Unknown` — for things with a real probe.
- `Freshness` (Fresh/Stale/etc.) — for *data sections* (scripts/tools/findings inside the Workstate snapshot). Stale is **neutral**, not a fault.
- `REAL_ADAPTERS` is the single roster; `status` and `adapters` are guaranteed to agree.

**What is NOT wired today:** Pulse, Rewind, Tripwire, ScriptVault (as a launchable/probed tool — only its data arrives via the Workstate feed), ToolFoundry (feed-only), rex-check/RexDoctor, rex-forge. The vision names nine components; four have real integration. **Closing that gap, cleanly, is the heart of this redesign.**

---

## 2. The One Big Idea: a Component Registry

Today the suite lives in two places that each know a hand-coded subset of tools:
- `REAL_ADAPTERS` / `snapshot.rs` knows the three health sources (hard-coded `if` blocks).
- `CATALOG` knows the two launch targets.

To become the cockpit of *nine* components without nine bespoke code paths, we unify these into **one declarative model**: the **Component Registry**.

> A **Component** is one box in the metaphor diagram. It has an identity, an optional way to report health, an optional way to be launched, and an optional data feed. Every panel in the cockpit is a view over the same list of Components.

This is the architectural spine of the redesign. It is the chosen approach for treating unwired components (decision #4): **design the full nine now, implement the wiring incrementally** — a component that isn't ready yet is simply a registry entry whose health source is `Planned` and whose launch command doesn't resolve. It still appears in the cockpit (so the suite map is complete and honest), rendered as a dimmed "planned" card — never a broken or fake-green one.

### 2.1 The Component model (lives in `rexops-core`, pure data)

```rust
/// One box in the suite metaphor. Pure data; no I/O. Describes a component and
/// HOW the cockpit should learn about it — the adapters/app layer does the work.
pub struct Component {
    pub id: ComponentId,            // stable kebab id, e.g. "pulse"
    pub name: &'static str,         // "Pulse"
    pub role: &'static str,         // "heartbeat monitor" — the metaphor line
    pub group: ComponentGroup,      // Brain / Monitor / FieldTool / Mechanic / Factory / Face
    pub health: HealthSource,       // how to probe it (see below)
    pub launch: Option<LaunchSpec>, // how to run it, if runnable
    pub feed: Option<FeedSpec>,     // contract-feed file, if it publishes one
    pub maturity: Maturity,         // Live / FeedReady / Planned
}

/// How the cockpit learns a component's health. This is the unification of the
/// three patterns already proven in the codebase, plus the honest "not yet".
pub enum HealthSource {
    /// Binary presence + version, like today's Bulwark probe.
    Probe { binary: &'static str, version_args: &'static [&'static str] },
    /// A live `status` subcommand that prints health (Pulse-style liveness).
    StatusCommand { binary: &'static str, args: &'static [&'static str] },
    /// A contract-feed file the component publishes, like today's Workstate.
    Feed { contract: &'static str },        // -> $XDG_DATA_HOME/rexops/feeds/<x>.json
    /// Derived from the host itself (the existing `system` adapter).
    Host,
    /// Designed but not wired yet. Always renders as a neutral "planned" card.
    Planned,
}
```

`ComponentGroup` and `Maturity` are small enums (KISS — match arms, not trait objects, mirroring the existing `PendingAction` decision). `LaunchSpec` is the catalog entry generalized (`run_mode`, `args`, `refresh_after`); `FeedSpec` names the contract file. The full nine are a single `pub const COMPONENTS: &[Component]` table — the suite map in one screenful, the way `CATALOG` is today.

### 2.2 Why this is the right altitude

- It **reuses** everything already coherent: `AdapterHealth`, `Freshness`, the `Adapter` trait, the feed-consumer pattern, `availability_tag`, the confirm gate, the jobs subsystem. No parallel universe.
- It **collapses** the two hard-coded rosters (`REAL_ADAPTERS`, `CATALOG`) into one, so the "status and adapters must agree" guarantee extends naturally to "the dashboard, the launcher, and the palette all read the same nine."
- It makes "add ScriptVault as a real probed+launchable tool later" a **data change** (one table row gains a `Probe`/`LaunchSpec`), not a code change across three files.
- It keeps the unwired six **visible and honest**: `maturity: Planned` → a dimmed card that says "planned," never a fake instrument.

### 2.3 The mapping (the nine, as registry rows)

| Component | Group | Health source (target) | Launch? | Feed? | Maturity now |
|---|---|---|---|---|---|
| Workstate | Brain | `Feed{workstate}` (live) | view snapshot | yes (live) | **Live** |
| Bulwark | FieldTool | `Probe{bulwark}` (live) | yes (live) | scan feed | **Live** |
| (System) | — | `Host` (live) | — | — | **Live** |
| Proto | FieldTool | derive from feed/probe | yes (live) | session feed | **Live** |
| ScriptVault | FieldTool | `Feed{scriptvault.export}` | yes (planned) | yes (contract exists) | **FeedReady** |
| ToolFoundry | FieldTool | `Feed{toolfoundry}` | yes (planned) | yes (contract exists) | **FeedReady** |
| Pulse | Monitor | `StatusCommand{pulse status}` | yes | — | **Planned** |
| Rewind | BlackBox | `StatusCommand` / `Feed` | yes | — | **Planned** |
| Tripwire | BlackBox | `StatusCommand` (alarms) | yes | — | **Planned** |
| rex-check / RexDoctor | Mechanic | `StatusCommand` | yes | — | **Planned** |
| rex-forge | Factory | `Probe` | yes | — | **Planned** |
| suite-ui | Face | (library — no card; it's the *medium*) | — | — | n/a |

> suite-ui is intentionally **not** a cockpit card: it is the common face the cockpit is *painted with* (decision #1), not an instrument. Its presence in this redesign is the shared widget layer in §5, not a status row.

Contracts already on disk (`linux-ops-suite/contracts/`) confirm several feeds exist or are specified: `scriptvault.export`, `toolfoundry.workstate-feed`, `proto.session`, `bulwark.scan`, `workstate.snapshot`, and `rexops.snapshot` — so "FeedReady" is grounded, not aspirational.

---

## 3. The Cockpit Dashboard (decision #2: landing + drill-down)

Screen `1` is reborn as the **Command Center**: the state of the entire suite, at a glance, with one-keypress launch — and every card drills down into the detail screens that already exist.

### 3.1 Layout

```
┌─ RexOps · Cockpit ───────────────────────────── suite: 7/9 live · 2 alerts ─┐
│ host: rex-laptop · kernel 6.17 · up 4d 2h                  2026-06-20 21:14Z │  ← identity banner (suite-ui)
├──────────────────────────────────────────────────────────────────────────┤
│  BRAIN                          MONITORS                                     │
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐                 │
│  │● Workstate      │  │◍ Pulse          │  │○ Tripwire       │   ← status     │
│  │  brain          │  │  heartbeat      │  │  alarm          │     cards      │
│  │  3/3 fresh      │  │  ♥ ▁▂▅▇▅▂ 12ms  │  │  planned        │   (the grid)   │
│  │  [w] view       │  │  [p] open       │  │                 │                 │
│  └────────────────┘  └────────────────┘  └────────────────┘                 │
│  FIELD TOOLS                                                                  │
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐                 │
│  │● Bulwark        │  │◍ ScriptVault    │  │◍ ToolFoundry    │                 │
│  │  security       │  │  scripts        │  │  tool lifecycle │                 │
│  │  ! 1 crit 1 high│  │  feed-ready     │  │  2 need review  │                 │
│  │  [b] scan ▸     │  │  [s] open ▸     │  │  [t] open ▸     │                 │
│  └────────────────┘  └────────────────┘  └────────────────┘                 │
├──────────────────────────────────────────────────────────────────────────┤
│ RISK  crit 1 · high 1 · med 0 · low 3   ⚠ BLOCK         │ activity / jobs ▸ │  ← risk + log strip
├──────────────────────────────────────────────────────────────────────────┤
│ 1 Cockpit  2 Adapters  3 System  4 Scripts  5 Tools  6 Launch  7 Jobs  ? help│  ← nav (existing)
└──────────────────────────────────────────────────────────────────────────┘
```

### 3.2 The status card (the new core widget)

Each card is one Component. It carries, top to bottom:
- **Status light** — `●` healthy (green), `◍` degraded / needs-attention (yellow), `○` planned/unknown (dim), `✗` unavailable (red). One glyph, one color: the warning light you can't miss.
- **Name + role line** — `Bulwark` / `security` (the metaphor word).
- **One-line vital** — the single most important number for that component: Workstate → `3/3 fresh`; Bulwark → `1 crit 1 high`; Pulse → a heartbeat sparkline + latency; ToolFoundry → `2 need review`; a Planned card → just `planned`.
- **Action hint** — `[b] scan ▸`. The bracketed letter is a **direct hotkey** (press `b` on the cockpit → arm Bulwark launch). The `▸` means "Enter drills into this component's detail screen."

Cards are grouped by `ComponentGroup` (Brain / Monitors / Field Tools / Mechanics / Factory) with quiet section labels — the metaphor becomes the layout.

### 3.3 Drill-down

The cockpit is the *index*; the existing screens are the *detail*:
- Enter on a card (or its number) → its detail screen. Bulwark/findings → a Findings detail; ScriptVault → screen `4`; ToolFoundry → screen `5`; Workstate → an adapters/sections view; Pulse → a Pulse detail (live heartbeat history) once wired.
- This means **no detail work is thrown away** — screens 2–7 stay, gaining a "back to cockpit" (`Esc`) and a clear "you are drilling into X" header. The cockpit just becomes the front door that was missing.

### 3.4 One-keypress launch (the cockpit's reason to exist)

The vision's literal ask: "launch any of the other tools with one keypress." Implementation:
- Each launchable card advertises a **mnemonic hotkey** (`b` Bulwark, `p` Pulse, `s` ScriptVault, …), derived once from the registry and shown in the card. These are **cockpit-screen-local** bindings (they don't collide with global `1`–`7`/`q`/`r` because the cockpit owns its keyspace, exactly as the Launcher screen owns navigation today).
- Pressing the hotkey **arms** the existing P8 confirm modal (`PendingAction::LaunchTool`) — it does **not** fire. The cockpit gets the same two-keypress safety as the Launcher: arm → confirm. This is non-negotiable; it reuses the proven gate verbatim.
- For a `Planned` component the hotkey is inert and the hint reads `(planned)` — pressing it shows a toast ("Pulse isn't wired yet — see the roadmap"), never an error.

---

## 4. Health, Freshness, and the Heartbeat (decision #3: unified probe+feed registry)

The cockpit's honesty depends entirely on this layer. We extend the existing `Adapter` pattern rather than inventing a monitor.

### 4.1 One driver, three (now four) sources

`rexops-app` gains a single function that walks `COMPONENTS` and, for each, resolves its `HealthSource` into an `AdapterHealth` (+ optional vital):
- `Probe` → the existing Bulwark-style presence/version spawn.
- `StatusCommand` → spawn `<binary> status` with the **configured timeout** (reusing `adapter_timeout`), parse a tiny contract (exit code + optional `{"healthy":bool,"detail":"…"}` line). This is how Pulse/Tripwire/rex-check report.
- `Feed` → the existing feed-consumer read (`with_text` → `with_path` → `standard_path`), yielding health **and** the `Freshness` of the data.
- `Host` → the existing `system` adapter.
- `Planned` → resolves to `Unknown`/dimmed with zero I/O (no spawn, no file read).

This replaces the three hard-coded `if real_adapter_enabled(...)` blocks in `snapshot.rs` with one registry walk. `REAL_ADAPTERS` becomes "the rows whose `HealthSource` is not `Planned`," derived from the table — one roster, as today, but now data-driven.

**Critical invariants carried forward unchanged:**
- **stdin is a process singleton** — read once at startup, fed to every refresh (the `piped_stdin` capture in `app/state.rs` stays exactly as is). No component health source may read stdin.
- **Graceful degradation everywhere** — unknown schema, missing feed, missing binary, timed-out status command → a *note* + a neutral/`Unavailable` card, never a panic or a fake-green light.
- **Configured timeouts bound every spawn** — the `StatusCommand` source MUST honor `adapter_timeout` so a hung tool can't freeze the cockpit (already proven for probes by `configured_timeout_bounds_a_hanging_adapter_binary`).
- **Freshness ≠ health.** A stale feed is a *neutral* yellow-ish "stale" badge on the card, not a red fault — preserving the model-coherence fix (a fresh install is not all-yellow).

### 4.2 Pulse, specifically (the heartbeat)

Pulse is the namesake of "heartbeat monitor," so its card earns the one genuinely novel widget: a **heartbeat sparkline** (`♥ ▁▂▅▇▅▂ 12ms`). RexOps keeps a small ring buffer of Pulse's recent liveness samples (last N refreshes) and renders them as a sparkline + latest latency. Implemented as a **shared suite-ui widget** (§5) so any tool can show a heartbeat. Until Pulse is wired, the card is a normal `Planned` card — the sparkline appears only when real samples exist.

### 4.3 Refresh & responsiveness

The existing background-refresh model is kept verbatim: `r` triggers a non-blocking refresh on a worker thread; the UI stays responsive; `catch_unwind` guarantees the `refreshing` flag always clears even if a probe panics. The registry walk runs *inside* that same worker. A future enhancement (out of scope for v1) is per-component staggered auto-refresh; v1 stays manual-`r` + on-launch refresh, matching today.

---

## 5. The Common Face: suite-ui widgets (decision #1: elevate via suite-ui)

The cockpit must be beautiful, and that beauty must be **shared** — suite-ui is the common face, so the cockpit's new widgets live in `linux-ops-suite/crates/suite-ui`, not locally in `rexops-tui`. Every tool in the suite then inherits the same instruments. This is the explicit decision and it shapes where code lands.

**New shared widgets to add to suite-ui (reusable across the whole suite):**
1. **`StatusCard`** — the grouped status card from §3.2 (light + name/role + vital + action hint). The single most important new widget; every suite tool's dashboard can use it.
2. **`StatusLight`** — the one-glyph health lamp (`● ◍ ○ ✗`) with the canonical color mapping, so "green means healthy" is identical everywhere.
3. **`Heartbeat`** — the sparkline widget for Pulse-style liveness (§4.2).
4. **`IdentityBanner`** — the top bar (host · kernel · uptime · clock · "7/9 live · 2 alerts" summary).
5. **`CardGrid`** — lays out cards in responsive columns by group, degrading gracefully on narrow terminals (stacks to one column).

**Kept from suite-ui as-is:** `pane`, `SearchBar`, `Theme`/`theme.health(...)`, `keys::is_palette`/`is_cancel`, `ToastKind`. The redesign extends the palette of the common face; it does not fork it.

**Aesthetic principles (the "cockpit feel"):**
- **Calm by default, loud on trouble.** A healthy suite is mostly quiet greens and dim text; a problem is the one bright thing on screen. The confirm modal stays deliberately loud (yellow `⚠ CONFIRM`), consistent with the alarm metaphor.
- **The metaphor is the layout.** Grouping cards by Brain/Monitors/Field Tools makes the diagram in §0 literally visible.
- **One number per instrument.** Cards show the single vital; depth is one keypress away. No wall of debug text on the landing screen (the same discipline that removed duplicated notes from `status`).
- **Consistent glyphs/colors** drawn from `Theme`, never ad-hoc — so the whole suite reads identically.

---

## 6. CLI Surface (the cockpit from a script)

The thin CLI grows to match the registry, staying a pure dispatch+format shell:
- `rexops` (no args) → the cockpit TUI (unchanged default).
- `rexops status` → keep, but render the **component roster** (nine rows: light · name · role · vital · maturity) instead of just adapters. `--json` emits the full snapshot incl. components.
- `rexops adapters` → unchanged (the probed-source view), now derived from the registry.
- `rexops components` (**new**) → list the registry: id · group · maturity · launch? · feed? (+`--json`). The machine-readable suite map.
- `rexops launch <id>` (**new, gated**) → launch a component non-interactively *only* with an explicit `--yes` (the CLI analogue of the confirm gate; without `--yes` it prints the resolved command and exits — a dry-run, mirroring the modal's preview line). Honors `availability_tag`: refuses `disabled`/`unavailable` with a clear message.

All heavy lifting stays in `rexops-app`; the CLI never grows logic, only formatting (the existing `print_status_human` discipline).

---

## 7. Components, Boundaries, and Testability

Each unit keeps one clear purpose (the existing crate discipline, files < ~300 LOC, Learning-Notes footers):

| Unit | Purpose | Depends on | Tested by |
|---|---|---|---|
| `core::component` | Pure `Component`/`HealthSource`/`Maturity` model + `COMPONENTS` table | nothing | table invariants (every id unique; every Live row has a resolvable source) |
| `core` (existing) | `OpsSnapshot` gains `components: Vec<ComponentStatus>` | nothing | serde round-trip |
| `adapters` (existing) | gains a `StatusCommand` adapter (spawn+timeout+parse) | libc/timeout only | timeout-bounds test (mirror the bulwark hang test) |
| `app::registry_walk` | resolve every `HealthSource` → `ComponentStatus`; the new single source of truth | adapters + core | "status/adapters/components agree on the roster"; graceful-degrade per source kind |
| `suite-ui` widgets | `StatusCard`/`StatusLight`/`Heartbeat`/`IdentityBanner`/`CardGrid` | ratatui only | buffer-to-text render tests (the existing dashboard test pattern) |
| `tui::screens::cockpit` | render the card grid from `OpsSnapshot.components`; own its hotkey keyspace | suite-ui + app | render test (cards present, planned dimmed); hotkey→arms-confirm test |
| `tui::commands` (existing) | the P8 confirm gate — **unchanged**; cockpit hotkeys arm `PendingAction::LaunchTool` | — | existing confirm suite + a cockpit-arm test |

**Isolation check:** the cockpit screen reads only `OpsSnapshot.components` (already resolved) — it does no I/O, so it's a pure render function, unit-testable off-screen exactly like `render_dashboard` is today. The registry walk is the only new place that does work, and it's a pure function of `(config, components, piped_stdin)` — testable by passing inputs, like `build_snapshot_with_piped`.

---

## 8. Safety (non-negotiable, carried from P8)

The cockpit makes launching *easier* (one keypress from the landing screen), so the safety layer matters more, not less:
- Every launch — from a card hotkey, the Launcher, the palette, or `rexops launch` — routes through the **single** `confirm_pending` → `launch_tool` path. No new arm-and-fire shortcut. (The safety invariant, verified in the current code: `launch_tool` has exactly one non-test caller — `dispatch.rs`'s `confirm_pending` arm. Cockpit hotkeys add a `PendingAction::LaunchTool` *producer*, never a new caller of `launch_tool`. `PendingAction` already carries more than one variant today — `LaunchTool` and `RunJob` — so the cockpit adds an arming site, not a new mutation path.)
- The modal stays loud and modal: while pending, Enter confirms, Esc cancels, every other key (incl. quit) is swallowed.
- `Planned` components are inert: their hotkey can't arm anything because they have no `LaunchSpec` to resolve.
- The CLI `launch` requires `--yes`; bare invocation is a dry-run preview. No surprise mutation from a script.
- Graceful degradation is a safety property here: a hung/missing tool yields a dim card + note, never a frozen cockpit.

---

## 9. Phasing (full hub now, wiring incrementally — decision #4)

The design covers all nine. Implementation lands in honest increments; each phase ships a coherent, tested cockpit.

- **Phase A — Registry spine.** Introduce `Component`/`HealthSource`/`COMPONENTS` in core; rewrite `snapshot.rs`'s hard-coded blocks as a registry walk over the *currently-live* sources (Bulwark/Workstate/System/Proto). No UI change yet; prove `status`/`adapters`/new `components` agree. Net behavior identical, internals unified.
- **Phase B — suite-ui cockpit widgets.** Add `StatusLight`/`StatusCard`/`CardGrid`/`IdentityBanner` to suite-ui (render-tested).
- **Phase C — Cockpit screen.** Replace screen `1` with the card grid reading `OpsSnapshot.components`; wire card hotkeys → existing confirm gate; existing screens become drill-downs (`Esc` → cockpit). All nine appear; the six unwired render as `Planned` cards.
- **Phase D — FeedReady tools.** Light up ScriptVault + ToolFoundry as full cards via their existing contracts (health + freshness + launch), promoting them from data-only to first-class components.
- **Phase E — Monitors & mechanics.** Add the `StatusCommand` adapter; wire Pulse (+ `Heartbeat` widget), then Tripwire/Rewind/rex-check/rex-forge as each grows a stable `status`/launch contract. Each is one registry row + a contract, no new screens required. ✅ *Pulse slice done (2026-06-21): `StatusCommand` health path + the suite-ui `Heartbeat` widget shipped; Pulse is a `Live`, launchable card with a heartbeat vital (rollup 6/11). Tripwire/Rewind/rex-check/rex-forge remain `Planned` — each a future one-row flip.*
- **Phase F — CLI parity.** `rexops components` + gated `rexops launch`.

Each phase is a spec-commit + `feat(rexops): … (Phase X)` impl-commit, matching the project's established cadence, with the four cargo gates (build/test/clippy/fmt) green at every step.

---

## 10. Explicit Non-Goals (YAGNI)

- **No plugin system / dynamic discovery.** The suite is a known, fixed set of nine — a static `COMPONENTS` table is the right altitude, exactly as the static `CATALOG` is today. (If the suite ever becomes user-extensible, that's a separate spec.)
- **No mouse-first redesign.** Keyboard-first stays; the cockpit is for an operator's hands on the home row.
- **No remote/multi-host cockpit.** One host, like today. The `Host`/identity banner is local.
- **No live auto-refresh polling in v1.** Manual `r` + on-launch refresh, as today. Staggered auto-refresh is a future enhancement, not part of this redesign.
- **No new persistence/DB.** Feeds + probes are the inputs; RexOps stays stateless across runs (only the in-memory heartbeat ring buffer is transient).
- **suite-ui is not a card.** It's the medium, not an instrument (see §2.3).

---

## 11. Success Criteria

The redesign is done when:
1. Running `rexops` opens on a **single screen that shows all nine components**, grouped by the metaphor, each with a health light and one vital — the suite's state at a glance.
2. A problem anywhere (a critical finding, a down tool, a stale brain) is the **one bright thing** on that screen.
3. Any launchable tool launches in **one keypress + one confirm** from the cockpit, through the unchanged safety gate.
4. Adding a not-yet-wired tool's real integration is a **table-row change**, not a cross-file code change.
5. The new instruments live in **suite-ui**, so the next tool's dashboard inherits them for free.
6. `status` / `adapters` / `components` can **never disagree** about the roster (one registry, guarded by a test).
7. All four cargo gates stay green; the cockpit screen and registry walk are **unit-tested off-screen** like the code they replace.

---

### Appendix: file-level change map (orientation, not a plan)

- `crates/rexops-core/src/component.rs` *(new)* — `Component`, `HealthSource`, `ComponentGroup`, `Maturity`, `COMPONENTS`.
- `crates/rexops-core/src/models.rs` — `OpsSnapshot` gains `components: Vec<ComponentStatus>`.
- `crates/rexops-adapters/src/status_cmd.rs` *(new)* — the `StatusCommand` adapter (spawn + timeout + tiny parse).
- `crates/rexops-app/src/snapshot.rs` — hard-coded probe blocks → one `registry_walk`; `build_*` unchanged at the boundary.
- `linux-ops-suite/crates/suite-ui/...` — `StatusCard`, `StatusLight`, `Heartbeat`, `IdentityBanner`, `CardGrid`.
- `crates/rexops-tui/src/screens/cockpit.rs` *(new, replaces dashboard as screen 1)* — card-grid render + drill-down + hotkeys.
- `crates/rexops-tui/src/tools/catalog.rs` — folded into / derived from `COMPONENTS` (launch specs become a view over the registry).
- `crates/rexops-cli/src/main.rs` — `components` subcommand + gated `launch`; `status` renders the roster.
```
