# ScriptVault export consumer — design (Phase 5)

Date: 2026-06-04
Status: approved

## Goal

Make RexOps aware of ScriptVault's script inventory, favorites, and recents by
reading an exported, versioned ScriptVault JSON, using the contract at
`../linux-ops-suite/contracts/scriptvault.export.schema.json` as the target
shape. The cockpit shows total scripts, favorites count, recents count, and
basic status. RexOps never writes back to ScriptVault.

## Key decision: reshape the existing stub into a real consumer

`crates/rexops-adapters/src/scriptvault.rs` is currently a demo stub with
hardcoded sample data — exactly the situation ToolFoundry was in before Phase 3.
We reshape `ScriptVaultInfo` / `Script` to the contract envelope and replace the
demo path with the feed-consumer pattern (same as ToolFoundry + Bulwark). We
**keep the type names** to avoid re-export ripple through `adapters/lib.rs`,
`core/lib.rs`, and `OpsSnapshot.scriptvault`.

## Types — favorites/recents as arrays, scripts loose

The contract is PROVISIONAL: it fixes the envelope (`schema_version`,
`source_tool: "scriptvault"`, `generated_at`) plus `scripts[]` (free-form
objects), `favorites[]` (string ids), `recents[]` (string ids). It warns "do not
treat the item shapes as final."

The current stub models `favorite`/`recent` as per-script booleans. The contract
models them as **separate id arrays**. We follow the contract:

```
ScriptVaultInfo {
    schema_version: i64,
    source_tool: String,       // lenient #[serde(default)]
    generated_at: String,      // #[serde(default)]
    scripts: Vec<Script>,      // #[serde(default)] — loose, like Bulwark's ScanItem
    favorites: Vec<String>,    // #[serde(default)] — favorite script ids
    recents: Vec<String>,      // #[serde(default)] — recently launched ids
}

Script {
    id: Option<String>,        // opportunistic
    name: Option<String>,      // opportunistic display fallback
    description: Option<String>,
    #[serde(flatten)] rest: Map<String, Value>,
}
```

Why arrays and not the boolean join: the summary needs only **counts**
(`scripts.len()`, `favorites.len()`, `recents.len()`). Matching each favorite id
back to a script would be a fragile join over a provisional, free-form item whose
id field is not fixed — and it buys nothing the requirement asks for. No `Tally`
struct (three `.len()` calls are not logic worth abstracting).

## Acquisition + version gate + routing — same as Phases 3/4

- Acquisition precedence: in-memory text (`with_text`) → explicit path
  (`with_path`) → standard path `$XDG_DATA_HOME/rexops/feeds/scriptvault.export.json`
  (fallback `~/.local/share/...`), per INTEGRATION_MAP.md. The adapter never reads
  stdin directly.
- `read() -> (AdapterHealth, Option<…>)` single acquisition.
- Version gate: `schema_version == 1` → parse → Healthy; missing/unknown →
  `Ok(None)` graceful skip → Degraded + note; no feed → Unavailable.
- `source_tool` lenient (note, don't crash on mismatch).
- The snapshot layer's single piped-stdin read + `classify_feed` router gains a
  third positive match: `source_tool == "scriptvault"`.

## Cockpit summary

- `snap.scriptvault: Option<ScriptVaultInfo>` (existing field, reshaped contents).
- CLI `status`: "N scripts, F favorites, R recents (as of DATE)" + a few script
  labels. **recents count is new** (the old model didn't carry it).
- TUI Scripts screen: minimal update to the new shape. Keep favorite stars only
  as an opportunistic membership check (star if the script's id/name is in
  `favorites[]`) — never a correctness dependency. No new screen.
- The dashboard already surfaces scriptvault via notes.

## Wiring (touch points)

1. `adapters/scriptvault.rs` — reshape types; feed-consumer pattern; drop the
   `exec`/`run_optional` demo version code.
2. `app/snapshot.rs`:
   - `classify_feed` — add the scriptvault arm.
   - Convert the scriptvault block to the routed `read()` pattern + wire into the
     router dispatch (the `route == Some(FeedKind::ScriptVault)` branch).
   - `populate_scriptvault(&mut snap, routed_stdin)` helper.
   - **`feeds_only_config()` test helper currently DISABLES scriptvault — remove
     it from the disable list** so routing tests fire it.
   - Add a scriptvault cross-leak routing test (guard must cover all three feeds).
3. `cli/main.rs` — scriptvault section to new shape (+ recents).
4. `tui/screens/scripts.rs` — compile-fixing update to new shape.
5. Registry label update for scriptvault.

## Fixture & tests

- NEW `fixtures/scriptvault/export_v1.json` — envelope-shaped: a few scripts,
  some favorite ids, some recent ids.

Tests in `scriptvault.rs`:
1. v1 fixture parses; scripts/favorites/recents counts correct.
2. Unknown major version → `Ok(None)` graceful skip.
3. Missing `schema_version` → `Ok(None)` graceful.
4. Lenient `source_tool` mismatch parses (no crash).
5. Unknown script fields preserved in `rest`.
6. `read()` returns health + info from one acquisition (via `with_text`).
7. Missing path → Unavailable, not error.
8. Serde roundtrip.

Snapshot-layer tests (in `snapshot.rs`):
9. `classify_feed` routes a scriptvault blob to `FeedKind::ScriptVault`.
10. Cross-leak: routing a scriptvault feed populates only `snap.scriptvault`.

## Behavior change to call out

ScriptVault stops showing demo data by default. With no feed present it reports
`Unavailable` and the Scripts screen is empty — the same transition ToolFoundry
made in Phase 3. An empty Scripts screen is expected, not a regression.

## Non-goals

- No writes to ScriptVault, no binary spawn (in-memory text / file only).
- No rigid script item schema — provisional; refine when ScriptVault ships real JSON.
- No JSON-Schema validator dependency (serde + version gate).
- No new TUI screen.
- The `~/bin/r-<toolname>` wrapper convention does not apply — extends the
  existing `rexops` binary.
