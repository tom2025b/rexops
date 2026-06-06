//! snapshot.rs — Shared builders for OpsSnapshot and AdapterRegistry.
//!
//! This module contains the *only* place that knows how to turn a loaded
//! AppConfig into a live OpsSnapshot by probing the enabled adapters.
//!
//! Previously this logic (and the nearly-identical registry builder) was
//! copy-pasted between CLI and TUI. That was temporary scaffolding per the
//! architecture plan. Now it lives in rexops-app so:
//! - There is one source of truth for "what does a snapshot contain?"
//! - The Workstate v3 snapshot is the single source of truth for scripts/tools/findings.
//! - CLI and TUI stay thin (they just call these fns and render).
//!
//! The functions still live in the "app" crate (not core) because they perform
//! side-effecting work (executing adapter probes). Core stays pure data.

use rexops_adapters::{Adapter, BulwarkAdapter, SystemAdapter, WorkstateAdapter};
use rexops_core::{AdapterEntry, AdapterId, AdapterRegistry, AppConfig, OpsSnapshot, RiskSummary};

/// Build a live OpsSnapshot by probing adapters that are enabled in config.
///
/// Respects the per-adapter `enabled` flag (default true when key absent).
/// Always adds a final "config loaded" note.
/// Populates first-class structured fields from system probes and Workstate,
/// plus notes for the dashboard/logs. Workstate is the only snapshot input for
/// scripts/tools/findings.
///
/// This is the single implementation used by both `rexops status` and the TUI
/// refresh thread.
pub fn build_snapshot(config: &AppConfig) -> OpsSnapshot {
    // Thin wrapper: read the single piped stdin (if any), then delegate. The
    // delegate is stdin-free so it can be unit-tested by passing the bytes in.
    build_snapshot_with_piped(config, read_piped_stdin().as_deref())
}

/// Core of `build_snapshot`, with the piped-stdin bytes (if any) passed in. Kept
/// separate so the snapshot-routing glue is testable without touching real stdin or
/// the filesystem.
fn build_snapshot_with_piped(config: &AppConfig, piped: Option<&str>) -> OpsSnapshot {
    let mut snap = OpsSnapshot::new();

    // Bulwark: only probe if enabled in config (defaults to true if absent).
    let bul_enabled = config.adapters.get("bulwark").map_or(true, |c| c.enabled);
    if bul_enabled {
        let bul = BulwarkAdapter::new();
        let health = bul.health();
        if let Ok(id) = AdapterId::new(bul.binary()) {
            snap.set_adapter_health(&id, health);

            if health.is_available() {
                if let Ok(Some(ver)) = bul.version() {
                    snap.add_note(format!("bulwark version: {ver}"));
                }
            } else {
                snap.add_note(
                    "bulwark adapter unavailable (binary not found or --help probe failed)"
                        .to_owned(),
                );
            }
        }
    }

    // System: respect enabled (default true). Lightweight, always works.
    let sys_enabled = config.adapters.get("system").map_or(true, |c| c.enabled);
    if sys_enabled {
        let sys = SystemAdapter::new();
        let sys_health = sys.health();
        if let Ok(id) = AdapterId::new("system") {
            snap.set_adapter_health(&id, sys_health);
        }
        if let Ok(out) = sys.info() {
            let i = &out.data;
            snap.system = Some(i.clone());
            if let Some(h) = &i.hostname {
                snap.add_note(format!("system hostname: {h}"));
            }
            if let Some(k) = &i.kernel {
                snap.add_note(format!("system kernel: {k}"));
            }
            if let Some(u) = &i.uptime {
                snap.add_note(format!("system uptime: {u}"));
            }
            for d in i.disk.iter().take(2) {
                snap.add_note(format!("system disk: {d}"));
            }
        }
    }

    // Workstate v3 is the source of truth for scripts/tools/findings. Piped
    // input is accepted only when it is a recognized Workstate snapshot; any
    // other piped blob is ignored rather than falling back to another path.
    let route = piped.map(classify_snapshot);
    let ws_enabled = config.adapters.get("workstate").map_or(true, |c| c.enabled);
    if ws_enabled {
        match (piped, route) {
            (Some(text), Some(SnapshotKind::Workstate)) => {
                populate_workstate(&mut snap, Some(text.to_owned()));
            }
            (Some(_), Some(SnapshotKind::Unknown)) => {
                snap.add_note("stdin: not a Workstate v3 snapshot — ignored".to_owned());
            }
            (None, _) => populate_workstate(&mut snap, None),
            (Some(_), None) => unreachable!("route is always present when piped is Some"),
        }
    }

    // Config note (now loaded). Neutral message that makes sense for both CLI and TUI.
    snap.add_note("config: loaded (respects 'enabled' per adapter)".to_owned());

    snap
}

/// Whether a blob of piped JSON is a Workstate v3 snapshot or something else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotKind {
    Workstate,
    Unknown,
}

/// Read piped stdin once. Returns Some(text) only when stdin is NOT a terminal
/// (i.e. content was piped in) and is non-empty. Errors and empty pipes → None.
fn read_piped_stdin() -> Option<String> {
    use std::io::{IsTerminal, Read};
    if std::io::stdin().is_terminal() {
        return None;
    }
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_ok() && !buf.trim().is_empty() {
        Some(buf)
    } else {
        None
    }
}

/// Classify piped JSON: Workstate v3 snapshot (schema_version==3 plus the three
/// Section keys) or Unknown. A positive match only — an unrecognized blob is
/// Unknown, never silently misrouted.
fn classify_snapshot(text: &str) -> SnapshotKind {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return SnapshotKind::Unknown;
    };
    let ws = v.get("schema_version").and_then(serde_json::Value::as_i64) == Some(3)
        && v.get("scripts").is_some()
        && v.get("tools").is_some()
        && v.get("findings").is_some();
    if ws {
        SnapshotKind::Workstate
    } else {
        SnapshotKind::Unknown
    }
}

/// Set a section's adapter health from its Workstate `status` and add a
/// freshness/provenance note like `"tools: Stale (source observed 2026-06-02)"`.
/// Shared by all three Workstate sections so the mapping stays consistent.
fn note_section_freshness(
    snap: &mut OpsSnapshot,
    label: &str,
    adapter_id: &str,
    status: &str,
    provenance: &rexops_adapters::Provenance,
) {
    if let Ok(id) = AdapterId::new(adapter_id) {
        snap.set_adapter_health(&id, rexops_adapters::status_to_health(status));
    }
    match provenance.source_observed_at.as_deref() {
        Some(src) => snap.add_note(format!("{label}: {status} (source observed {src})")),
        None => snap.add_note(format!("{label}: {status}")),
    }
}

/// Read the Workstate v3 snapshot and fold it into the OpsSnapshot.
///
/// The snapshot's three sections route into the structured fields RexOps renders:
///   tools.data    -> snap.tools
///   scripts.data  -> snap.scripts
///   findings.data -> snap.findings (+ merged risk)
/// Each section's freshness `status` is mapped to AdapterHealth and a provenance
/// note is added. Unknown/missing versions and a missing snapshot degrade
/// gracefully.
fn populate_workstate(snap: &mut OpsSnapshot, routed_stdin: Option<String>) {
    let ws = match routed_stdin {
        Some(text) => WorkstateAdapter::with_text(text),
        None => WorkstateAdapter::new(),
    };
    let (ws_health, snapshot) = match ws.read() {
        Ok(pair) => pair,
        Err(e) => {
            snap.add_note(format!("workstate: snapshot unreadable ({e})"));
            (rexops_core::AdapterHealth::Unknown, None)
        }
    };
    if let Ok(id) = AdapterId::new("workstate") {
        snap.set_adapter_health(&id, ws_health);
    }

    let Some(out) = snapshot else {
        if ws_health == rexops_core::AdapterHealth::Degraded {
            snap.add_note(
                "workstate: snapshot present but unknown/missing schema version — skipped"
                    .to_owned(),
            );
        }
        return;
    };

    let info = out.data;
    snap.add_note(format!(
        "workstate: v3 snapshot, {}/3 sections populated (built {}) — source of truth",
        info.populated_section_count(),
        info.built_at
    ));

    fold_ws_tools(snap, &info);
    fold_ws_scripts(snap, &info);
    fold_ws_findings(snap, &info);

    snap.workstate = Some(info);
}

/// Fold the snapshot's `tools` section into `snap.tools` (+ freshness/notes).
fn fold_ws_tools(snap: &mut OpsSnapshot, info: &rexops_adapters::WorkstateInfo) {
    let Some(tools) = &info.tools.data else {
        return;
    };
    note_section_freshness(
        snap,
        "tools",
        "tools",
        &info.tools.status,
        &info.tools.provenance,
    );
    snap.add_note(format!(
        "tools: {} total, {} need attention (as of {})",
        tools.tool_count, tools.attention_count, tools.as_of
    ));
    for t in tools.tools.iter().filter(|t| t.needs_attention()).take(3) {
        let review_note = if t.review_due_flag {
            match t.review_after.as_deref() {
                Some(date) => format!(", review due since {date}"),
                None => ", review due".to_string(),
            }
        } else {
            String::new()
        };
        snap.add_note(format!(
            "  attention: {} ({}, {}{})",
            t.display_name, t.status, t.lifecycle_state, review_note
        ));
    }
    snap.tools = Some(tools.clone());
}

/// Fold the snapshot's `scripts` section into `snap.scripts` (+ freshness/notes).
fn fold_ws_scripts(snap: &mut OpsSnapshot, info: &rexops_adapters::WorkstateInfo) {
    let Some(scripts) = &info.scripts.data else {
        return;
    };
    note_section_freshness(
        snap,
        "scripts",
        "scripts",
        &info.scripts.status,
        &info.scripts.provenance,
    );
    snap.add_note(format!(
        "scripts: {} total, {} favorites, {} recents (as of {})",
        scripts.total(),
        scripts.favorites_count(),
        scripts.recents_count(),
        scripts.generated_at
    ));
    snap.scripts = Some(scripts.clone());
}

/// Fold the snapshot's `findings` section into `snap.findings` and merge its risk.
fn fold_ws_findings(snap: &mut OpsSnapshot, info: &rexops_adapters::WorkstateInfo) {
    let Some(findings) = &info.findings.data else {
        return;
    };
    note_section_freshness(
        snap,
        "findings",
        "findings",
        &info.findings.status,
        &info.findings.provenance,
    );
    let t = findings.risk_tally();
    if t.has_risk_data() {
        snap.merge_risk(&RiskSummary {
            critical: t.critical,
            high: t.high,
            medium: t.medium,
            low: t.low,
            info: t.info,
            total_findings: t.rated_total() + t.unknown,
            should_block: t.should_block(),
            max_severity: None,
        });
        snap.add_note(format!(
            "findings: {} scanned — critical={} high={} medium={} low={} info={}",
            findings.items.len(),
            t.critical,
            t.high,
            t.medium,
            t.low,
            t.info
        ));
        for item in findings.high_risk_items().take(5) {
            let sev = item.severity.as_deref().unwrap_or("?");
            snap.add_note(format!("  high-risk: {} ({})", item.label(), sev));
        }
    } else {
        snap.add_note(format!(
            "findings: {} scanned — risk breakdown unavailable",
            findings.items.len()
        ));
    }
    snap.findings = Some(findings.clone());
}

/// Build a simple AdapterRegistry from live probes (demo of registry usage).
/// Only includes adapters enabled in config.
///
/// This is intentionally separate from build_snapshot because the `rexops adapters`
/// subcommand only cares about the registry view (health + label), not the full
/// risk/notes/structured data.
pub fn build_adapter_registry(config: &AppConfig) -> AdapterRegistry {
    let mut reg = AdapterRegistry::new();

    let bul_enabled = config.adapters.get("bulwark").map_or(true, |c| c.enabled);
    if bul_enabled {
        let bul = BulwarkAdapter::new();
        let health = bul.health();
        if let Ok(id) = AdapterId::new("bulwark") {
            reg.insert(AdapterEntry {
                id,
                health,
                label: Some("Bulwark content inspection (via inspect scan)".to_owned()),
            });
        }
    }

    let sys_enabled = config.adapters.get("system").map_or(true, |c| c.enabled);
    if sys_enabled {
        let sys = SystemAdapter::new();
        let sys_health = sys.health();
        if let Ok(id) = AdapterId::new("system") {
            reg.insert(AdapterEntry {
                id,
                health: sys_health,
                label: Some("Lightweight system info (hostname, kernel, uptime, disk)".to_owned()),
            });
        }
    }

    let ws_enabled = config.adapters.get("workstate").map_or(true, |c| c.enabled);
    if ws_enabled {
        let ws = WorkstateAdapter::new();
        let ws_health = ws.health();
        if let Ok(id) = AdapterId::new("workstate") {
            reg.insert(AdapterEntry {
                id,
                health: ws_health,
                label: Some("Workstate snapshot consumer (read-only)".to_owned()),
            });
        }
    }

    reg
}

// Learning Notes:
// - Both builders are side-effecting (they call into adapters which may spawn
//   processes or read /proc etc.). That's why they live in rexops-app, not core.
// - The Workstate v3 snapshot is the single source of truth for scripts/tools/findings.
// - stdin is read ONCE (read_piped_stdin) and routed by content (classify_snapshot),
//   because stdin is a process singleton. Only Workstate v3 snapshots are recognized
//   via piped stdin; anything else is Unknown.
// - The Adapter trait is used only for .health(), .version(), .info(), .binary().
//   No other adapter internals leak out of this crate.

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const WORKSTATE_FEED: &str =
        include_str!("../../rexops-adapters/fixtures/workstate/snapshot_v3.json");

    #[test]
    fn classify_recognizes_workstate_v3_snapshot() {
        assert_eq!(classify_snapshot(WORKSTATE_FEED), SnapshotKind::Workstate);
    }

    #[test]
    fn classify_unknown_blob_is_not_silently_misrouted() {
        assert_eq!(
            classify_snapshot(r#"{"schema_version":1,"hello":"world"}"#),
            SnapshotKind::Unknown
        );
        assert_eq!(classify_snapshot("not json"), SnapshotKind::Unknown);
    }

    /// Config with bulwark + system disabled so the snapshot only reflects
    /// snapshot routing (no binary probes in CI).
    fn workstate_only_config() -> AppConfig {
        let mut cfg = AppConfig::default();
        for name in ["bulwark", "system"] {
            cfg.adapters.insert(
                name.to_owned(),
                rexops_core::AdapterConfig {
                    enabled: false,
                    ..Default::default()
                },
            );
        }
        cfg
    }

    /// Build via the REAL routing glue with `piped` passed straight in (no stdin).
    fn build_via_pipe(piped: &str) -> OpsSnapshot {
        build_snapshot_with_piped(&workstate_only_config(), Some(piped))
    }

    #[test]
    fn workstate_snapshot_fans_out_into_all_structured_fields() {
        let snap = build_via_pipe(WORKSTATE_FEED);
        assert!(snap.workstate.is_some(), "v3 snapshot kept in workstate");
        assert!(snap.tools.is_some(), "tools.data must populate tools");
        assert!(snap.scripts.is_some(), "scripts.data must populate scripts");
        assert!(
            snap.findings.is_some(),
            "findings.data must populate findings"
        );
        assert!(
            snap.risk.critical >= 1,
            "findings risk must merge into the risk pane"
        );
        assert!(snap.risk.should_block, "a critical finding forces block");
    }

    #[test]
    fn workstate_section_status_maps_to_adapter_health() {
        let snap = build_via_pipe(WORKSTATE_FEED);
        let degraded = rexops_core::AdapterHealth::Degraded;
        for id in ["tools", "scripts", "findings"] {
            assert_eq!(
                snap.adapter_health.get(id).copied(),
                Some(degraded),
                "{id} health should be Degraded (section was Stale)"
            );
        }
    }

    #[test]
    fn workstate_findings_risk_counts_are_correct() {
        let snap = build_via_pipe(WORKSTATE_FEED);
        assert_eq!(snap.risk.critical, 1, "one critical finding in fixture");
        assert_eq!(snap.risk.high, 1, "one high finding in fixture");
    }

    #[test]
    fn piped_non_snapshot_leaves_workstate_empty() {
        // A blob that is not a v3 snapshot is classified Unknown and not routed to
        // the Workstate adapter — all three structured fields stay None.
        let snap = build_via_pipe(r#"{"schema_version":1,"hello":"world"}"#);
        assert!(
            snap.workstate.is_none(),
            "non-snapshot must not populate workstate"
        );
        assert!(snap.tools.is_none());
        assert!(snap.scripts.is_none());
        assert!(snap.findings.is_none());
    }
}
