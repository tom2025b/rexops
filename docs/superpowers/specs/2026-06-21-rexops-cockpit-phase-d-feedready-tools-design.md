# RexOps Cockpit — Phase D: FeedReady Tools — Design

**Status:** Approved (brainstorm 2026-06-21). Next: implementation plan.

## 1. Goal

Promote **ScriptVault** and **ToolFoundry** from data-only cards to full,
**launchable, `Live`** cockpit components — so each launches in one keypress
through the existing confirm gate (its card letter, or `Enter` on the focused
card), exactly like Bulwark and Proto today.

Their feed-derived **health**, **freshness**, and **vital** (`"3 scripts"`,
`"1 need review"`) already render on their cockpit cards (Phase A wired the feed
contracts). The only missing capability is **launch**.

## 2. The real problem — two launch sources that disagree

There are two parallel launch descriptions in the codebase today:

- **`COMPONENTS` registry** (`rexops-core/src/component_table.rs`): each
  `Component` has `launch: Option<LaunchSpec>` (`run_mode`, `args`,
  `refresh_after`). This drives the **`launchable`** flag on the rendered card
  (`rexops-app/src/snapshot.rs`: `launchable = comp.launch.is_some() && enabled &&
  health != Unavailable`). For ScriptVault/ToolFoundry today this is `None`.
- **`CATALOG`** (`rexops-tui/src/tools/catalog.rs`): a separate hand-maintained
  `&[ToolEntry]` (only `bulwark`, `proto`) whose `launch_args` are what
  `resolve_launch_command` actually uses to build the command. CATALOG also
  uniquely backs the **Launcher screen** (screen 6) and the **command palette**
  `run <tool>` rows with a human `description`, and provides `is_streamable()` /
  `refreshes_after()`.

The hazard: if Phase D only adds a `LaunchSpec` to the registry, a card flips to
`launchable = true`, but pressing its key **no-ops** — because
`resolve_launch_command` looks the tool up in `CATALOG::by_id`, where it is
absent, so it returns `None` (no args ⇒ but more importantly the screen/palette
never list it and `is_streamable` can't see it). The two sources must be unified.

`ToolEntry` and `LaunchSpec` already carry the **same** launch facts
(`run_mode`/`args`/`refresh_after`); the only thing `ToolEntry` adds is a
`description` string for the Launcher screen + palette. So the duplication is real
and removable.

## 3. Approach (A — registry is the single source of truth)

**The `COMPONENTS` registry becomes the one source for launch data; `CATALOG`
is retired in favour of a registry-derived view.**

Concretely:

1. **`Component` gains a `blurb: &'static str`** — the one fact `ToolEntry` had
   that the registry lacked: a short human description for the Launcher screen and
   palette rows (e.g. ScriptVault → `"Script library + runner"`). A dedicated
   field (not overloading the terse `role`, which is a noun like `"scripts"`).
2. **A registry "launchable view"** — a helper in `rexops-core` (e.g.
   `launchable_components() -> impl Iterator<Item = &'static Component>`) yielding
   the components whose `launch.is_some()`, in registry order. This is the list the
   Launcher screen and palette iterate.
3. **`resolve_launch_command` reads the registry** — it builds the command from
   `component_by_id(id).launch` (program from `which`/config as today; **args from
   `LaunchSpec.args`**) instead of `catalog::by_id(id).launch_args`.
4. **`is_streamable` / `refreshes_after` read the registry** — `RunMode` and
   `refresh_after` come from `component_by_id(id).launch` instead of `ToolEntry`.
5. **`CATALOG` / `ToolEntry` are removed** from `tools/catalog.rs`; the module
   becomes a thin set of registry-backed helpers (`launchable_components`,
   `by_id`-equivalent over the registry, `is_streamable`, `refreshes_after`). The
   Launcher screen (screen 6), the palette, the launch-availability cache, and the
   `selected_tool` index now operate over the registry-derived launchable list.

> Why not the smaller "add to both tables" (twins) option: it leaves the two-source
> problem in place — the next tool still needs two edits and they can still drift.
> Approach A is what makes "adding a tool is a one-row change" (success criterion
> #4) and "status/adapters/components can never disagree" (#6) true at the root.

> Why not the larger "delete CATALOG and rewrite everything bespoke": Approach A
> already retires `ToolEntry`/`CATALOG`; the difference is we keep a thin
> `tools::` helper layer (a registry *view*) so the Launcher screen / palette
> change minimally (they iterate a slice of `&'static Component` instead of
> `&[ToolEntry]`), rather than each re-deriving registry queries inline.

## 4. The two components (registry rows after Phase D)

```rust
Component {
    id: "scriptvault",
    name: "ScriptVault",
    role: "scripts",
    blurb: "Script library + runner",            // NEW
    group: ComponentGroup::FieldTool,
    health: HealthSource::Feed { contract: "scriptvault" },
    launch: Some(LaunchSpec {                     // was None
        run_mode: RunMode::Foreground,            // opens its own interactive UI
        args: &[],
        refresh_after: false,
    }),
    feed: Some(FeedSpec { contract: "scriptvault" }),
    maturity: Maturity::Live,                     // was FeedReady
},
Component {
    id: "toolfoundry",
    name: "ToolFoundry",
    role: "tool lifecycle",
    blurb: "Tool build/lifecycle manager",        // NEW
    group: ComponentGroup::FieldTool,
    health: HealthSource::Feed { contract: "toolfoundry" },
    launch: Some(LaunchSpec {                      // was None
        run_mode: RunMode::Foreground,
        args: &[],
        refresh_after: false,
    }),
    feed: Some(FeedSpec { contract: "toolfoundry" }),
    maturity: Maturity::Live,                      // was FeedReady
},
```

- **`run_mode: Foreground`** — both are interactive tools that own the terminal
  (like Bulwark/Proto), so they hand over the TTY rather than streaming into the
  Jobs screen. `args: &[]` launches the tool bare (its own picker/UI); no
  RexOps-chosen subcommand. `refresh_after: false` (Foreground tools decide
  refresh via the launcher return path, as the existing `refresh_after` doc notes).
- **`Maturity::Live`** — a tool with working feed health + freshness + a launch is
  fully wired, which is what `Live` means. They now count in the banner's
  `N/M live` rollup (3/11 → 5/11) and read as full instruments. The existing core
  invariant test `live_components_have_a_non_planned_health_source` already passes
  for them (their health source is `Feed`, not `Planned`).

> **Semantic change — what `live` means.** Phase A established an invariant test
> (`status_adapters_and_components_never_disagree_on_the_live_roster` in
> `rexops-app/src/snapshot.rs`) asserting that the `live`-maturity component cards
> are *exactly* the **adapter roster** (`bulwark`/`system`/`workstate`). Phase D
> deliberately widens `live`: a feed-backed tool with a launch is `Live` even
> though it is not adapter-*probed*. So after Phase D, `live` cards = the adapter
> roster **plus** ScriptVault + ToolFoundry. The two cross-source rosters
> (`status`'s `adapter_health` and `adapters`' registry) are **unchanged** — feeds
> are not adapters — so those two still agree exactly with the adapter roster; only
> the "live cards == adapter roster" leg changes to "live cards == adapter roster +
> feed-backed launchables". The plan updates (and renames) that test accordingly.
> This does not weaken success-criterion #6 ("status/adapters/components can never
> disagree about the *roster*"): the registry is still one source; "live" is just a
> richer maturity than "is an adapter".

## 5. Launch mechanism (per the user's "type one word, however is cleanest")

No new resolution logic. `resolve_launch_command` keeps its existing two-step
program resolution — **`which <id>` first, then the adapter's configured `binary`
path** — and now takes its **args from the registry `LaunchSpec`**. So:

- If `scriptvault` / `toolfoundry` resolve as a bare command on `PATH` (one word),
  the card reads `interactive` and launches. ← the user's desired end state.
- If not yet on `PATH`, the card reads `disabled` (same honest 3-state tag as any
  unresolved tool today), and pointing the adapter's `binary` at a path makes it
  launch — **exactly the Bulwark/Proto precedent**, no special-casing.

Phase D does **not** install binaries or add wrappers (respects the
no-wrappers/bare-binary preference): it makes the cockpit *able* to launch these
two by one word the moment that word resolves, and never invites a launch it
can't fulfil (the health+resolve gate in `arm_tool` already covers this).

## 6. What changes (file map)

- `rexops-core/src/component.rs` — `Component` gains `pub blurb: &'static str`;
  add `launchable_components()` (or equivalent registry view). Update the
  `Component` doc.
- `rexops-core/src/component_table.rs` — the 11 rows gain `blurb`; ScriptVault +
  ToolFoundry gain `LaunchSpec` and flip to `Maturity::Live`. (Every other row
  gets a `blurb` too — a one-line description each.)
- `rexops-tui/src/tools/catalog.rs` — remove `CATALOG` / `ToolEntry`; replace with
  registry-backed `launchable_components()` view + `by_id`/`is_streamable`/
  `refreshes_after` reading the registry `LaunchSpec`.
- `rexops-tui/src/tools/launcher.rs` — `resolve_launch_command` reads args from the
  registry `LaunchSpec`.
- `rexops-tui/src/screens/launchpad.rs` — the Launcher screen iterates the
  registry launchable view (`&'static Component`) instead of `&[ToolEntry]`;
  `description` → `blurb`.
- `rexops-tui/src/commands/palette.rs` — `run <tool>` rows iterate the registry
  view; `description` → `blurb`.
- `rexops-tui/src/app/state.rs` — the launch-availability cache iterates the
  registry view instead of `CATALOG`.
- `rexops-tui/src/app/update.rs` — `selected_tool` bounds/Enter use the registry
  view length / lookup.
- Tests: core registry tests (blurb present; scriptvault/toolfoundry launchable +
  Live), the launcher/palette tests (now over the registry view), and an invariant
  test that the Launcher list == registry `launch.is_some()` set (the unification
  guard).

## 7. Behaviour parity & risk

- **Bulwark + Proto keep launching identically** — they move from `CATALOG` rows
  to registry rows that already exist (`bulwark`, `proto` are in `COMPONENTS` with
  `LaunchSpec`s today), so the Launcher screen still lists exactly the same tools
  in the same order, with the same args and run modes. A guard test asserts the
  launchable set is unchanged for them.
- **The cockpit cards** for ScriptVault/ToolFoundry gain a marker that now *arms*
  (was a read-only card). No other card changes.
- **Risk** is the blast radius of retiring `CATALOG`: ~6 call sites + ~8 launcher
  tests. Mitigated by (a) keeping a thin `tools::` view layer so callers change
  shape minimally, and (b) the unification guard test. The four cargo gates stay
  green at every task.

## 8. Non-goals (YAGNI)

- No binary installation, no wrappers, no aliases added (the user's bare-binary
  rule). Resolution stays `which`-then-config-binary.
- No change to feed health/freshness/vital (already working).
- No new screens; the existing Launcher screen + cockpit cards suffice.
- No change to Bulwark/Proto behaviour beyond their launch data now living in the
  registry instead of `CATALOG`.
- Pulse/Tripwire/Rewind/rex-check/rex-forge stay `Planned` (that's Phase E).

## 9. Success criteria

1. ScriptVault + ToolFoundry render as `Live`, launchable cockpit cards; their
   letter/`Enter` arms them through the existing confirm gate.
2. The banner rollup reads `5/11 live`.
3. There is **one** launch source: the `COMPONENTS` registry. `resolve_launch_command`,
   the Launcher screen, the palette, `is_streamable`, and `refreshes_after` all
   read it; `CATALOG`/`ToolEntry` are gone. A guard test locks the Launcher list to
   the registry `launch.is_some()` set.
4. Bulwark + Proto launch exactly as before (same list, args, run modes).
5. All four cargo gates (build/test/clippy/fmt) green at every task; the registry
   view + resolution are unit-tested off-screen.
