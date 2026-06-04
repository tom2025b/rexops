# Phase 6 — Workstate snapshot consumer (design)

**Date:** 2026-06-04
**Status:** Approved, ready to implement
**Repo:** rexops only (read-only)

## Goal

Add a sixth read-only feed consumer for **Workstate** snapshot data, structurally
identical to the Phase 3–5 consumers (ToolFoundry, Bulwark-feed, ScriptVault).
Target contract: `../linux-ops-suite/contracts/workstate.snapshot.schema.json`.

That contract is explicitly **PROVISIONAL** — Workstate emits nothing yet. It
fixes only the universal envelope (`schema_version`, `source_tool: const
"workstate"`, `generated_at`) plus a free-form `projects[]` array of objects with
`additionalProperties: true`. So we type the envelope and keep each project item
**loose**, exactly as the previous consumers handle their provisional shapes.

## Non-goals

- **No invented risk.** The contract has no severity/risk fields. Workstate
  behaves like ScriptVault/ToolFoundry: it populates a structured field + notes,
  and does **not** call `merge_risk`. Only Bulwark feeds the risk pane.
- **No TUI screen/launcher wiring.** The cockpit surfaces Workstate through
  `snap.notes` (auto-rendered as TUI banners) and the CLI status block. No
  dedicated screen, action, or launcher entry.
- **No writes, no spawning a binary.** Read-only is absolute.

## Shape

```rust
pub struct Project {
    pub name: Option<String>,        // opportunistic
    pub path: Option<String>,        // opportunistic
    #[serde(flatten)]
    pub rest: BTreeMap<String, Value>, // everything else, verbatim
}

pub struct WorkstateInfo {
    pub schema_version: i64,
    pub source_tool: String,         // lenient: should be "workstate", not rejected
    pub generated_at: String,
    pub projects: Vec<Project>,
}
```

Helpers: `WorkstateInfo::project_count()`, `Project::label()` (path → name →
`"<unknown>"`).

## Behavior (the established pattern)

- **Acquisition precedence:** `with_text` (routed stdin) → `with_path` →
  `standard_path` = `$XDG_DATA_HOME/rexops/feeds/workstate.snapshot.json`
  (falls back to `~/.local/share/...` when XDG unset). Never reads stdin itself.
- **Single `read() -> (AdapterHealth, Option<AdapterOutput<WorkstateInfo>>)`** in
  one acquisition. `(Healthy, Some)` / `(Degraded, None)` feed-present-bad-version
  / `(Unavailable, None)` no feed.
- **Version gate, not JSON-Schema validation:** a `VersionProbe` reads
  `schema_version` first; `== 1` → full parse; missing/other → `Ok(None)`
  graceful skip. Malformed JSON stays a hard `AdapterError::JsonParse`.
- **Adapter key:** plain `"workstate"` (no live counterpart, so no `-feed` suffix).
- `Adapter` trait impl: `check_available` / `version` (`schema_version=N`) /
  `health`.

## Routing

`classify_feed` in `rexops-app/src/snapshot.rs` gets a new **positive** match:

```rust
Some("workstate") => return FeedKind::Workstate,
```

placed alongside the bulwark/scriptvault `source_tool` arms, short-circuiting
before the ToolFoundry required-field check (no P4-style stdin collision). An
unrecognized blob stays `Unknown` → note + path fallback, never silently
misrouted. `FeedKind` gains a `Workstate` variant.

`populate_workstate(snap, routed_stdin)` mirrors `populate_scriptvault`: records
adapter health, on a v1 feed populates `snap.workstate` and adds a count note +
the first couple of project labels; degraded → "unknown/missing schema version —
skipped" note; missing → silence.

## Cockpit surface

- **CLI `rexops status`:** a `Workstate (snapshot as of <date>):` block printing
  `projects: N` and the first few project labels. Added after the Bulwark block.
- **TUI:** notes only (auto-rendered from `snap.add_note`).
- **`--json`:** free via the new `OpsSnapshot.workstate` field.

## Edit surface (8 touch points)

1. `rexops-adapters/src/workstate.rs` — new consumer + tests.
2. `rexops-adapters/fixtures/workstate/snapshot_v1.json` — fixture
   (`source_tool: "workstate"`, 3 projects).
3. `rexops-adapters/src/lib.rs` — `mod workstate;`, `pub use`, doc line.
4. `rexops-core/src/lib.rs` — re-export `WorkstateInfo` + doc line.
5. `rexops-core/src/models.rs` — add `pub workstate: Option<crate::WorkstateInfo>`
   to `OpsSnapshot` and init in `new()` (only constructor — no struct literals
   elsewhere).
6. `rexops-app/src/snapshot.rs` — import, `FeedKind::Workstate`, classify arm,
   `populate_workstate`, enabled-block in `build_snapshot_with_piped`,
   enabled-block in `build_adapter_registry`, routing/cross-leak test.
7. `rexops-cli/src/main.rs` — `if let Some(ws) = &snap.workstate` render block.
8. (TUI untouched beyond notes.)

## Tests

- Adapter unit tests: parse v1 fixture w/ count; unknown-major skip; missing
  version skip; lenient source_tool; malformed JSON is a parse error; unknown
  project fields preserved in `rest`; serde roundtrip; `with_text` reads from
  memory; missing path → Unavailable.
- `snapshot.rs`: extend the cross-leak routing test to 4-way — a workstate feed
  populates `snap.workstate` and leaks into none of bulwark/toolfoundry/
  scriptvault; `classify_feed(WORKSTATE_FEED) == FeedKind::Workstate`.

## Verification

`cargo test` across the workspace + `cargo build` (proves no TUI exhaustive match
broke from the additive `OpsSnapshot` field). `cargo clippy` clean (crate denies
unwrap/expect outside tests).
