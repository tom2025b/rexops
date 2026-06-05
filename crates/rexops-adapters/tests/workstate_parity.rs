//! workstate_parity.rs — Phase 2, Step 5: parity gate.
//!
//! Proves that for the SAME source data, RexOps gets identical structured output
//! whether it reads the three RAW feeds or the ONE Workstate v3 snapshot. This is
//! the gate that authorizes Step 7 (deleting the raw feed adapters): if the two
//! paths produce equal `ToolFoundryInfo` / `ScriptVaultInfo` / `BulwarkScanInfo`
//! (and equal risk), the raw path is provably redundant.
//!
//! Method: take the v3 snapshot's three section `data` payloads as the single
//! source of truth, and reconstruct the equivalent RAW feed for each by wrapping
//! that exact payload in the envelope a raw adapter expects. Parse the payload
//! both ways — via WorkstateAdapter (snapshot path) and via the raw adapter's
//! parse_feed (raw path) — and assert the results are equal. Same bytes in, so
//! any difference can only come from the two code paths, which is what we test.

use rexops_adapters::{
    BulwarkFeedAdapter, ScriptVaultAdapter, ToolFoundryAdapter, WorkstateAdapter,
};
use serde_json::{json, Value};

const SNAPSHOT_V3: &str = include_str!("../fixtures/workstate/snapshot_v3.json");

/// Pull the raw JSON `data` object for one section out of the v3 snapshot.
fn section_data(section: &str) -> Value {
    let snap: Value = serde_json::from_str(SNAPSHOT_V3).expect("fixture is valid JSON");
    snap[section]["data"].clone()
}

// ENVELOPE METADATA, not rendered data: `schema_version` and `source_tool` live on
// the Workstate snapshot ENVELOPE (the snapshot's `data` payloads omit them), but
// the raw feeds carry them inline. RexOps never renders these two from a section
// (verified: no `.schema_version` / `.source_tool` reads in app/cli/tui), so the
// parity check normalizes them on both sides and compares the data that actually
// drives the dashboard. We inject the same canonical values used when building the
// synthetic raw feeds below.
const CANON_SOURCE_TOOL: &str = "parity";
const CANON_SCHEMA_VERSION: i64 = 1;

#[test]
fn tools_parity_raw_vs_snapshot() {
    // Snapshot path: parse the whole snapshot, take tools.data.
    let snap = WorkstateAdapter::parse_feed(SNAPSHOT_V3).unwrap().unwrap();
    let mut from_snapshot = snap.tools.data.expect("snapshot has tools");

    // Raw path: wrap the SAME tools.data in the raw ToolFoundry envelope and parse
    // it through the raw adapter. ToolFoundryInfo has no source_tool field, so
    // schema_version is the only envelope datum the snapshot section omits.
    let mut raw = section_data("tools");
    raw["schema_version"] = json!(CANON_SCHEMA_VERSION);
    let raw_text = serde_json::to_string(&raw).unwrap();
    let from_raw = ToolFoundryAdapter::parse_feed(&raw_text).unwrap().unwrap();

    // The two paths must agree on everything the dashboard renders.
    assert_eq!(from_raw.tool_count, from_snapshot.tool_count);
    assert_eq!(from_raw.attention_count, from_snapshot.attention_count);
    assert_eq!(from_raw.tools, from_snapshot.tools);

    // Normalize the unrendered envelope metadata, then assert FULL equality.
    from_snapshot.schema_version = CANON_SCHEMA_VERSION;
    assert_eq!(
        from_raw, from_snapshot,
        "ToolFoundryInfo must be identical via raw and snapshot paths"
    );
}

#[test]
fn scripts_parity_raw_vs_snapshot() {
    let snap = WorkstateAdapter::parse_feed(SNAPSHOT_V3).unwrap().unwrap();
    let mut from_snapshot = snap.scripts.data.expect("snapshot has scripts");

    let mut raw = section_data("scripts");
    raw["schema_version"] = json!(CANON_SCHEMA_VERSION);
    raw["source_tool"] = json!(CANON_SOURCE_TOOL);
    let raw_text = serde_json::to_string(&raw).unwrap();
    let from_raw = ScriptVaultAdapter::parse_feed(&raw_text).unwrap().unwrap();

    assert_eq!(from_raw.total(), from_snapshot.total());
    assert_eq!(from_raw.favorites, from_snapshot.favorites);
    assert_eq!(from_raw.recents, from_snapshot.recents);

    from_snapshot.schema_version = CANON_SCHEMA_VERSION;
    CANON_SOURCE_TOOL.clone_into(&mut from_snapshot.source_tool);
    assert_eq!(
        from_raw, from_snapshot,
        "ScriptVaultInfo must be identical via raw and snapshot paths"
    );
}

#[test]
fn findings_parity_raw_vs_snapshot() {
    let snap = WorkstateAdapter::parse_feed(SNAPSHOT_V3).unwrap().unwrap();
    let mut from_snapshot = snap.findings.data.expect("snapshot has findings");

    // The raw Bulwark feed uses `items`; the snapshot uses `findings`. The raw
    // adapter accepts both (alias), so we feed the payload as-is plus the envelope.
    let mut raw = section_data("findings");
    raw["schema_version"] = json!(CANON_SCHEMA_VERSION);
    raw["source_tool"] = json!(CANON_SOURCE_TOOL);
    let raw_text = serde_json::to_string(&raw).unwrap();
    let from_raw = BulwarkFeedAdapter::parse_feed(&raw_text).unwrap().unwrap();

    // Item sets must match, and crucially the derived RISK must match — risk is
    // what the dashboard's risk pane and should_block decision depend on.
    assert_eq!(from_raw.items, from_snapshot.items);
    assert_eq!(
        from_raw.risk_tally(),
        from_snapshot.risk_tally(),
        "derived risk must be identical via raw and snapshot paths"
    );

    from_snapshot.schema_version = CANON_SCHEMA_VERSION;
    CANON_SOURCE_TOOL.clone_into(&mut from_snapshot.source_tool);
    assert_eq!(
        from_raw, from_snapshot,
        "BulwarkScanInfo must be identical via raw and snapshot paths"
    );
}

#[test]
fn all_three_sections_present_so_parity_is_meaningful() {
    // Guard against a future fixture that silently drops a section, which would
    // make the parity tests above vacuously pass on a None.
    let snap = WorkstateAdapter::parse_feed(SNAPSHOT_V3).unwrap().unwrap();
    assert!(snap.tools.data.is_some(), "tools section must be present");
    assert!(
        snap.scripts.data.is_some(),
        "scripts section must be present"
    );
    assert!(
        snap.findings.data.is_some(),
        "findings section must be present"
    );
}
