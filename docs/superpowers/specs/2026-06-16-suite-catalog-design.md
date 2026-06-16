# RexOps Suite Catalog — Design

**Date:** 2026-06-16
**Status:** Approved design, ready for implementation plan
**Goal:** Turn RexOps into the central TUI for the whole linux-ops-suite — comfortably launch every major tool (Bulwark, ScriptVault, Proto, Workstate, ToolFoundry) from one clean, categorised interface.

---

## 1. Background: how the catalog works today

The launcher catalog is a single static array in `crates/rexops-tui/src/tools/catalog.rs`:

```rust
pub const CATALOG: &[ToolEntry] = &[ /* Bulwark, Proto */ ];

pub struct ToolEntry {
    pub id:            &'static str,            // "bulwark" — keys `which <id>` AND adapter config
    pub name:          &'static str,            // display name
    pub description:   &'static str,
    pub run_mode:      RunMode,                 // Foreground (seizes terminal) | Background (Jobs)
    pub launch_args:   &'static [&'static str], // e.g. ["tui"]
    pub refresh_after: bool,                    // re-probe adapters when a bg job finishes?
}
```

Every consumer already fans out from this one array, which is why adding tools is low-risk:

| Consumer | Uses the catalog for |
| --- | --- |
| `screens/launchpad.rs` | renders one Launcher row per entry + the detail pane |
| `commands/palette.rs` | one `run <id>` palette command per entry |
| `commands/dispatch.rs` | `arm_tool` → confirm gate → `launch_tool` / `start_job` |
| `app/state.rs` | builds the launch-availability cache by resolving every entry |
| `jobs/manager.rs` | background run path + `refreshes_after()` |
| `tools/launcher.rs` | `resolve_launch_command(id)` |

**Launch resolution** (`tools/launcher.rs`), unchanged by this design except where noted:

1. if the adapter is `enabled: false` in config → not launchable;
2. else if `config.adapters[id].binary` is set → use it (an explicit admin pin always wins);
3. else `which <binary>` on PATH → use that;
4. else → "no launch command yet" (the row renders inert/disabled);
5. then append the catalog `launch_args`.

The same resolver feeds the dry-run preview, the foreground launch, and the background job, so they can never disagree.

**Key architectural fact (preserved):** *adapters* and *tools* are two different axes that share an id namespace. Adapters are probed **data sources** (`bulwark`, `system`, `workstate`) that produce health badges; the catalog lists launchable **programs** (`bulwark`, `proto`). Bulwark is both; Proto is launch-only; `system`/`workstate` are probe-only. Sharing the id lets a launchable tool reuse its adapter's `binary`/`enabled` config when one exists.

---

## 2. Problems this design fixes

Adding tools today means appending a struct literal — fine for two tools, rough at ten:

- **A. PATH lookup is hardwired to `which <id>`.** When a tool's id differs from its binary name, the only escape is pinning `binary` in every user's config. The catalog cannot carry a default binary name distinct from its id.
- **B. No grouping.** A flat column of ten security/scripts/system/inventory tools reads as noise.
- **C. Launcher-only tools have no obvious config home.** `enabled`/`binary` live under `adapters:` in YAML; a pure launcher tool with no probe (e.g. ToolFoundry) technically resolves via `adapters[id]`, but that key reads wrong.

This design adopts the **categorised static catalog** (chosen over "just add them" and over a config-driven YAML registry). It keeps the static, compile-checked, already-wired model — no plugin layer, no YAML tool registry — and adds only the two things scale needs: a binary name distinct from id, and visual grouping.

---

## 3. The new `ToolEntry`

Two new fields; everything else is unchanged.

```rust
/// The product area a tool belongs to. Drives Launcher grouping only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Security,
    Scripts,
    System,
    Inventory,
}

impl Category {
    /// Section header text + stable render order (Security, Scripts, System, Inventory).
    pub fn title(self) -> &'static str { /* ... */ }
    pub const ORDER: [Category; 4] = [Self::Security, Self::Scripts, Self::System, Self::Inventory];
}

pub struct ToolEntry {
    pub id:             &'static str,
    pub name:           &'static str,
    pub description:    &'static str,
    pub category:       Category,               // NEW — grouping only
    pub default_binary: &'static str,           // NEW — binary name when id differs; usually == id
    pub run_mode:       RunMode,
    pub launch_args:    &'static [&'static str],
    pub refresh_after:  bool,
}
```

**Resolution change (small, surgical):** in `tools/launcher.rs`, the PATH fallback switches from `which <id>` to `which <default_binary>`. Config still wins over PATH; `enabled: false` still blocks. For every tool in this first cut `default_binary == id`, so behaviour is identical today — the field exists so a future tool whose binary ≠ id needs no per-user config. This directly retires problem **A**.

---

## 4. The catalog (all five tools, grounded in the real binaries)

Verified against the installed suite on PATH: `bulwark`, `proto`, `scriptvault`, `workstate`, `toolfoundry` all resolve, and each tool's launch profile below comes from its actual `--help`.

| id | name | category | default_binary | run_mode | launch_args | refresh_after |
| --- | --- | --- | --- | --- | --- | --- |
| `bulwark` | Bulwark | Security | `bulwark` | Foreground | `["tui"]` | n/a* |
| `scriptvault` | ScriptVault | Scripts | `scriptvault` | Foreground | `[]` | n/a* |
| `proto` | Proto | Scripts | `proto` | Background | `["run"]` | `false` |
| `workstate` | Workstate | System | `workstate` | Background | `[]` | **`true`** |
| `toolfoundry` | ToolFoundry | Inventory | `toolfoundry` | Foreground | `["tui-catalog"]` | n/a* |

\* `refresh_after` is only meaningful for Background tools; Foreground tools refresh via the launcher's return path (`LaunchReport::should_refresh`), so the field is `false` and unused for them.

Per-tool rationale (each grounded in `--help`):

- **Bulwark** — unchanged. Its `tui` subcommand opens an interactive TUI → Foreground.
- **ScriptVault** — *"With no subcommand, ScriptVault opens its interactive TUI."* → `launch_args: []`, Foreground.
- **Proto** — unchanged. `proto run` executes a checklist that emits output and exits → Background (the catalog's existing streamed example). Self-contained run, so `refresh_after: false`.
- **Workstate** — **not interactive.** `workstate [OUTPUT]` is a state compiler: it writes `snapshot.json` (the shared RexOps feed) and exits. So it streams into Jobs (Background) **and** is the catalog's first `refresh_after: true` entry — finishing it rewrites the very feed RexOps probes, so the cockpit should re-read state. `launch_args: []` (default output path is the shared feed).
- **ToolFoundry** — the suite's tool-lifecycle/inventory program (binary `toolfoundry`; "Toolbox"/"ToolFoundry" are the same tool — there is no `toolbox` binary). Its interactive surface is the `tui-catalog` subcommand → `launch_args: ["tui-catalog"]`, Foreground.

**Explicitly excluded — RexOps itself.** A "Launch RexOps" row inside RexOps is redundant and recursive, and the `rexops` binary is the headless CLI (`status`/`adapters`) with no TUI subcommand to open. RexOps launches *other* tools, never itself. No self-reference in the catalog.

---

## 5. Launcher screen: grouped rendering

Grouping is a **render concern only** — the catalog stays a flat array; the screen projects it into sections.

- `render_launcher_list` walks `Category::ORDER`; for each category it emits a dim header line (e.g. `Security`) then every catalog row whose `category` matches, in catalog order. Empty categories are skipped.
- Row content is unchanged: accent rail on the selected row, name padded to the name column, health badge, and the 3-state availability tag (`interactive` / `streams` / `unavailable` / `disabled`) from `App::availability_tag` — still the single source of truth shared with the palette.
- **Selection stays a flat index over the catalog** (`app.selected_tool`), exactly as today. Headers are non-selectable display rows; ↑/↓ moves between tool rows only. This keeps `update.rs` navigation (`selected_tool` wrap over `CATALOG.len()`) and every existing launchpad test's index model intact — only the rendered output gains headers.

```
Launcher  — Pick a tool with ↑/↓; Enter confirms enabled tools.
──────────────────────────────────────────────────────────────
Security
  ▌ Bulwark      ● ·  interactive
Scripts
    ScriptVault  ○ ·  disabled
    Proto        ○ ·  streams
System
    Workstate    ○ ·  streams
Inventory
    ToolFoundry  ○ ·  disabled
```

The **command palette stays a flat `run <id>` list** (no grouping) — fast fuzzy launch is the palette's job; categories are the Launcher's. No palette change beyond the new entries appearing automatically.

---

## 6. Config home for launcher-only tools (problem C)

No schema change. We resolve C by **convention + documentation**, keeping KISS:

- A tool that is *also* a probed adapter (Bulwark) continues to use its existing `adapters.<id>` block for `enabled`/`binary`.
- A *launcher-only* tool (ScriptVault, Proto, Workstate-as-launch, ToolFoundry) is pinned, when needed, under the **same `adapters.<id>` key** — the resolver already reads `config.adapters[id].binary` for any id. We document in `examples/config.yaml` that `adapters:` is the control surface for *both* probed adapters and launchable tools (it is, today, the "per-tool admin pin" surface). Since every tool here has `default_binary == id` and all five are on PATH, **no config is required to launch any of them** — pinning is the override, not the norm.

This avoids inventing a parallel `tools:` config section (and its validation/migration) for zero current benefit. If third-party tools ever need registration without a rebuild, revisit a config-driven catalog then — explicitly out of scope now.

---

## 7. What changes, file by file

- `tools/catalog.rs` — add `Category` enum; add `category` + `default_binary` to `ToolEntry`; replace the 2-entry array with the 5-entry catalog from §4. Keep `by_id` / `is_streamable` / `refreshes_after`.
- `tools/launcher.rs` — PATH fallback uses `default_binary` instead of `id` (one function, `command_from_path` call site). Behaviour identical for this catalog; future-proofs id≠binary.
- `tools/mod.rs` — export `Category`.
- `screens/launchpad.rs` — grouped rendering in `render_launcher_list` (headers per `Category::ORDER`); rows + detail + selection model unchanged.
- `examples/config.yaml` — document the five tools and that `adapters:` pins launchable tools too; add commented stanzas.
- Tests — extend launchpad render tests for headers + the new rows; a catalog test asserting every `default_binary`/`launch_args`/`run_mode` matches §4 and ids stay unique; confirm Workstate is the `refresh_after: true` Background entry.

**Untouched:** `commands/palette.rs` (new rows appear automatically), `commands/dispatch.rs`, `jobs/manager.rs`, `app/state.rs` availability cache, `rexops-core` config/registry, the whole `rexops-adapters`/`rexops-app` probe path. The confirm-gate, terminal handoff, and refresh-on-return semantics are reused as-is.

---

## 8. Behaviour guarantees (unchanged invariants)

- A tool not installed / not on PATH renders **disabled** and Enter is inert — never a crash (exactly Proto's current behaviour when absent).
- `enabled: false` blocks launch even when the binary is on PATH.
- Preview, foreground launch, and background job always use the one resolver, so the dry-run preview shows exactly what runs.
- Foreground tools suspend/restore the TUI around the child; Background tools stream into Jobs. Only Workstate triggers an auto-refresh on completion (`refresh_after: true`); nothing else re-probes as a surprise side effect.

---

## 9. Out of scope (YAGNI)

- Config-driven / plugin catalog (Option 3).
- A separate `tools:` YAML section distinct from `adapters:`.
- Passing selected-item context (a specific script/finding) into a launched tool — the catalog launches whole tools; item-level launch is a later, separate design.
- Any change to RexOps's own CLI or a self-launch entry.
