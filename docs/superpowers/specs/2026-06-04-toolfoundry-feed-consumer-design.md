# ToolFoundry rexops-feed consumer — design

Date: 2026-06-04
Status: approved

## Goal

Implement a read-only consumer for the ToolFoundry `rexops-feed`, using the
contract at `../linux-ops-suite/contracts/toolfoundry.rexops-feed.schema.json`.
The cockpit (CLI `status`, TUI dashboard notes) shows total tools, tools needing
attention, and basic status. RexOps never writes back to ToolFoundry.

## Key decision: make the existing stub real in place

`crates/rexops-adapters/src/toolfoundry.rs` is currently a mock stub whose
`Tool` / `ToolFoundryInfo` shapes do not match the contract. Its Learning Notes
already anticipate this: *"If made real later, replace mock vec with actual
command execution + parse."*

We reshape `Tool` / `ToolFoundryInfo` to the contract and replace the mock data
path with a real reader. **We keep the type names** so we avoid rename ripples
through `adapters/lib.rs`, `core/lib.rs`, and `OpsSnapshot.toolfoundry`.

## Contract types

`ToolFoundryInfo`:
- `schema_version: i64`
- `as_of: String`
- `tool_count: usize`
- `attention_count: usize`
- `tools: Vec<Tool>` (`#[serde(default)]`)

`Tool`:
- `id: String`, `display_name: String` (required by schema)
- `owner`, `project`, `lifecycle_state`, `status`, `manifest_path`: `String`
  with `#[serde(default)]`
- `review_due: bool`, `drifted: bool`: `#[serde(default)]`
- `health_passed: u32`, `health_total: u32`: `#[serde(default)]`

`additionalProperties: true` in the schema → serde ignores unknown keys by
default, which is the desired forward-compatible behavior.

## Feed acquisition — path or stdin

No JSON-Schema validator crate. The whole codebase parses with serde; we follow
that and gate the version explicitly.

- **stdin** when input is piped (`std::io::IsTerminal` → stdin is not a tty).
  Lets `toolfoundry rexops-feed --json | rexops status` work with no new
  subcommand.
- **standard path** otherwise: `$XDG_DATA_HOME/rexops/feeds/toolfoundry.rexops-feed.json`,
  fallback `~/.local/share/rexops/feeds/toolfoundry.rexops-feed.json` (documented
  in linux-ops-suite `docs/INTEGRATION_MAP.md`).
- If neither yields content → adapter is `Unavailable` (normal for an optional
  tool; never an error).

### Version gate (graceful), not schema rejection

The schema pins `schema_version` to `const: 1`, but the requirement is to treat
missing/unknown major versions *gracefully*. A real `const:1` validator would
hard-reject version 2 — the opposite of graceful. So:

1. Parse just `{ schema_version: Option<i64> }` from the raw JSON first.
2. `Some(1)` → full typed parse into `ToolFoundryInfo` → `Ok(Some(info))`.
3. `Some(other)` or `None` → `Ok(None)` (graceful skip). The cockpit adds a note
   explaining the feed was skipped due to an unknown/missing version.

The ToolFoundry CLI **exits non-zero when attention is required** (a behavioral
part of the contract). The consumer must not treat valid JSON on stdin as a
failure regardless of upstream exit code — we read the bytes, not the exit code.
We never spawn the binary ourselves, so this only matters for the piped case,
where stdin already carries the bytes.

## Adapter health semantics

For a file/stdin consumer (no binary probe):
- feed acquired + `schema_version == 1` → `Healthy`
- feed acquired + unknown/missing version → `Degraded` (+ note)
- no feed found → `Unavailable`

## Cockpit wiring

- `app/snapshot.rs` `build_snapshot` toolfoundry block: read the feed, set health
  accordingly, populate `snap.toolfoundry` on success, and add a note:
  `"toolfoundry: N tools, M need attention (as of <date>)"`. On graceful skip,
  add a note explaining the skip. Update the registry block's label to reflect
  the real feed consumer.
- `cli/main.rs` `print_status_human`: the ToolFoundry section shows
  `tool_count`, `attention_count`, and a few per-tool `display_name (status,
  lifecycle_state)` lines. (Old `name/owner/health/symlink` fields are gone.)
- TUI dashboard already renders snapshot notes — **no new screen**.

## Fixture & tests

Copy the example fixture into
`crates/rexops-adapters/fixtures/toolfoundry/rexops_feed_v1.json` (hermetic:
`include_str!` across to the sibling repo would break a standalone clone).

Tests in `toolfoundry.rs`:
1. v1 fixture parses; `tool_count == 1`, `attention_count == 1`, tool `status`
   is `attention`.
2. `schema_version: 2` JSON → consumer returns `Ok(None)` (graceful, no error).
3. JSON with no `schema_version` → `Ok(None)` (graceful).
4. `ToolFoundryInfo` serde roundtrip.

## Non-goals

- No writes to ToolFoundry, no spawning the binary.
- No JSON-Schema validator dependency.
- No new TUI screen.
- The `~/bin/r-<toolname>` wrapper-script convention does not apply — we extend
  the existing `rexops` binary.
