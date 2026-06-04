# Bulwark scan feed consumer — design (Phase 4)

Date: 2026-06-04
Status: approved

## Goal

Make RexOps aware of Bulwark inventory + risk data by reading an exported,
versioned Bulwark scan JSON, using the contract at
`../linux-ops-suite/contracts/bulwark.scan.schema.json` as the target envelope.
The cockpit shows total items scanned, a risk breakdown if available, and any
high-risk items. RexOps never writes back to Bulwark.

## Key decision: a new, separate module

A real Bulwark adapter already exists (`crates/rexops-adapters/src/bulwark.rs`)
that spawns `bulwark inspect scan --format json` and parses live findings. That
is a *different data source* (live inspection of text) from this Phase 4 work
(consuming an exported, versioned scan feed from file/stdin).

We add a **new module** `crates/rexops-adapters/src/bulwark_feed.rs` and leave
`bulwark.rs` untouched. This keeps the two concerns cleanly separated and mirrors
the Phase 3 ToolFoundry feed consumer pattern exactly.

## Types — loose, because the contract is provisional

The contract is explicitly PROVISIONAL: it fixes only the envelope
(`schema_version`, `source_tool: "bulwark"`, `generated_at`, `items[]`) and says
"do not treat the item shape below as final." So we do NOT rigidly type items.

```
BulwarkScanInfo {
    schema_version: i64,
    source_tool: String,       // lenient: #[serde(default)], note-don't-crash on mismatch
    generated_at: String,      // #[serde(default)]
    items: Vec<ScanItem>,      // #[serde(default)]
}

ScanItem {
    severity: Option<String>,  // opportunistic; read as string, never an enum
    id: Option<String>,        // opportunistic id-like label for high-risk display
    name: Option<String>,      // opportunistic fallback label
    #[serde(flatten)] rest: Map<String, Value>,  // keep everything else
}
```

Severity is read as a **string**, lowercased, and bucketed with an "unknown"
fallback. We deliberately do NOT reuse the existing `BulwarkSeverity` enum from
`bulwark.rs` — it has no catch-all variant and would hard-fail on an unexpected
or missing value, which is the opposite of "graceful / provisional."

## Acquisition + version gate — same as ToolFoundry feed

- **stdin** when piped, else the documented standard path
  `$XDG_DATA_HOME/rexops/feeds/bulwark.scan.json` (fallback `~/.local/share/...`),
  per linux-ops-suite `docs/INTEGRATION_MAP.md`.

  IMPLEMENTATION REFINEMENT (discovered during build): stdin is a process-wide
  singleton — readable once, and the bytes belong to exactly one consumer. With
  two feed consumers (ToolFoundry + Bulwark) each reading stdin, the first drains
  the pipe and starves the second (and could even misparse the wrong feed). So
  stdin reading is HOISTED OUT of the adapters into the snapshot layer:
  - Adapters never touch `std::io::stdin()`. They gain a `with_text(String)`
    constructor; precedence is text → explicit path → standard path.
  - `build_snapshot` reads stdin ONCE (`read_piped_stdin`), classifies the blob
    by content (`classify_feed`), and routes it to the matching consumer via
    `with_text`. Bulwark is matched by `source_tool == "bulwark"`; ToolFoundry by
    its required fields (`tool_count`+`attention_count`+`tools`). Both are
    POSITIVE matches — an unrecognized blob is reported, never silently misrouted.
  - This refactor also fixed the same latent stdin collision in `toolfoundry.rs`
    (Phase 3) and removed it from the registry builder.
- **Single read:** `read() -> Result<(AdapterHealth, Option<AdapterOutput<BulwarkScanInfo>>)>`
  from one acquisition. Carries forward the Phase 3 stdin consume-once fix — never
  call a draining `health()` then `info()` on a piped feed.
- **Version gate:** parse `schema_version` first. Known (== supported) → full
  parse → Healthy. Missing/unknown major → `Ok(None)` graceful skip → Degraded +
  note. No feed found → Unavailable. Malformed JSON → JsonParse error, noted (not
  a crash). `source_tool` mismatch is lenient: a note, never an error.

Supported `schema_version`: 1 (the provisional envelope uses integer majors; we
accept 1 and skip anything else gracefully).

## Cockpit summary

This is the behavior-changing part and was explicitly approved.

The dashboard already renders a **Risk Summary pane** from `snap.risk`, but
nothing currently populates it (it is always zeros today). The feed consumer is
the natural thing to light it up:

- Derive a `RiskSummary` (critical/high/medium/low/info + total_findings +
  should_block) from item severities and set `snap.risk` via `merge_risk`.
  `should_block` = any item is critical or high.
- Also store structured `snap.bulwark: Option<BulwarkScanInfo>` (new field on
  `OpsSnapshot`, parallel to `toolfoundry`) for the items list.
- "Risk breakdown if available": when NO item carries a severity-like field, add
  a note "risk breakdown unavailable" and leave counts at zero rather than
  zero-filling something misleading.
- CLI `status`: a Bulwark section — total items, risk line (or "breakdown
  unavailable"), and up to ~5 high-risk (critical/high) item labels.
- **No new TUI screen** — the existing risk pane + notes cover the summary.

## Wiring (touch points)

1. NEW `adapters/src/bulwark_feed.rs` — types + read/parse/version-gate (the work).
2. `adapters/src/lib.rs` — `mod bulwark_feed;` + re-export `BulwarkFeedAdapter`,
   `BulwarkScanInfo`, `ScanItem`.
3. `core/src/models.rs` — add `OpsSnapshot.bulwark: Option<crate::BulwarkScanInfo>`
   (`#[serde(default, skip_serializing_if = "Option::is_none")]`); update `new()`.
4. `core/src/lib.rs` — re-export the new type if core needs it for the field
   (mirror how `ToolFoundryInfo` is referenced).
5. `app/src/snapshot.rs` — `populate_bulwark_feed(&mut snap)` helper using one
   `read()`; sets adapter health, populates `snap.bulwark`, merges `snap.risk`,
   adds notes. Add a registry entry/label too.
6. `cli/src/main.rs` — Bulwark section in `print_status_human`.

## Fixture & tests

- NEW `fixtures/bulwark/scan_feed_v1.json` — envelope-shaped (distinct name so it
  does not shadow the inspect adapter's findings-shaped `scan_sample.json`). A few
  items with mixed severities incl. at least one `critical`/`high`.

Tests in `bulwark_feed.rs`:
1. v1 fixture parses; item count correct; risk counts derived correctly;
   `should_block` true when a critical/high item is present.
2. Unknown major version (e.g. 99) → `Ok(None)` graceful skip (no error).
3. Missing `schema_version` → `Ok(None)` graceful.
4. Items with no severity field → risk counts zero + "unavailable" signalled
   (e.g. a helper returns None/empty), not a misleading zero-fill claim.
5. `read()` returns health + info together from one acquisition (explicit path).
6. Missing path → Unavailable, not error.
7. Serde roundtrip of `BulwarkScanInfo`.

## Non-goals

- No writes to Bulwark, no spawning any binary (file/stdin only).
- No rigid item schema — provisional by design; refine when Bulwark ships real JSON.
- No JSON-Schema validator dependency (serde + explicit version gate, as Phase 3).
- No new TUI screen.
- The `~/bin/r-<toolname>` wrapper convention does not apply — we extend the
  existing `rexops` binary.
