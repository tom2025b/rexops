//! snapshot.rs — Shared builders for OpsSnapshot and AdapterRegistry.
//!
//! This module contains the *only* place that knows how to turn a loaded
//! AppConfig into a live OpsSnapshot by probing the enabled adapters.
//!
//! Previously this logic (and the nearly-identical registry builder) was
//! copy-pasted between CLI and TUI. That was temporary scaffolding per the
//! architecture plan. Now it lives in rexops-app so:
//! - There is one source of truth for "what does a snapshot contain?"
//! - Adding a fifth adapter only requires editing this file.
//! - CLI and TUI stay thin (they just call these fns and render).
//!
//! The functions still live in the "app" crate (not core) because they perform
//! side-effecting work (executing adapter probes). Core stays pure data.

use rexops_adapters::{
    Adapter, BulwarkAdapter, BulwarkFeedAdapter, ScriptVaultAdapter, SystemAdapter,
    ToolFoundryAdapter, WorkstateAdapter,
};
use rexops_core::{AdapterEntry, AdapterId, AdapterRegistry, AppConfig, OpsSnapshot, RiskSummary};

/// Build a live OpsSnapshot by probing adapters that are enabled in config.
///
/// Respects the per-adapter `enabled` flag (default true when key absent).
/// Always adds a final "config loaded" note.
/// Populates first-class structured fields (system, scriptvault, toolfoundry,
/// bulwark) when the corresponding adapter succeeds, plus notes for the
/// dashboard/logs. Feed consumers (toolfoundry/bulwark/scriptvault) share the
/// single piped stdin, read once here and routed by content.
///
/// This is the single implementation used by both `rexops status` and the TUI
/// refresh thread.
pub fn build_snapshot(config: &AppConfig) -> OpsSnapshot {
    // Thin wrapper: read the single piped stdin (if any), then delegate. The
    // delegate is stdin-free so it can be unit-tested by passing the bytes in.
    build_snapshot_with_piped(config, read_piped_stdin().as_deref())
}

/// Core of `build_snapshot`, with the piped-stdin bytes (if any) passed in. Kept
/// separate so the feed-routing glue is testable without touching real stdin or
/// the standard feed paths.
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

    // Feed consumers (ToolFoundry, Bulwark scan, ScriptVault) share a single piped
    // stdin. stdin is a process-wide singleton — readable once, owned by exactly
    // one consumer — so the caller read it ONCE and we route those bytes to
    // whichever feed identifies itself. Everything else falls back to standard paths.
    let route = piped.map(classify_feed);
    if route == Some(FeedKind::Unknown) {
        snap.add_note(
            "stdin: piped feed not recognized — falling back to standard paths".to_owned(),
        );
    }

    // ToolFoundry: read-only consumer of the `rexops-feed` contract.
    let tf_enabled = config
        .adapters
        .get("toolfoundry")
        .map_or(true, |c| c.enabled);
    if tf_enabled {
        let routed = if route == Some(FeedKind::ToolFoundry) {
            piped.map(str::to_owned)
        } else {
            None
        };
        populate_toolfoundry(&mut snap, routed);
    }

    // Bulwark scan feed: read-only consumer of the exported scan JSON (separate
    // from the live `bulwark inspect` adapter above). Keyed "bulwark-feed".
    let bwf_enabled = config
        .adapters
        .get("bulwark-feed")
        .map_or(true, |c| c.enabled);
    if bwf_enabled {
        let routed = if route == Some(FeedKind::Bulwark) {
            piped.map(str::to_owned)
        } else {
            None
        };
        populate_bulwark_feed(&mut snap, routed);
    }

    // ScriptVault export feed: read-only consumer of the script inventory JSON.
    let sv_enabled = config
        .adapters
        .get("scriptvault")
        .map_or(true, |c| c.enabled);
    if sv_enabled {
        let routed = if route == Some(FeedKind::ScriptVault) {
            piped.map(str::to_owned)
        } else {
            None
        };
        populate_scriptvault(&mut snap, routed);
    }

    // Workstate snapshot feed: read-only consumer of per-project repo health.
    let ws_enabled = config.adapters.get("workstate").map_or(true, |c| c.enabled);
    if ws_enabled {
        let routed = if route == Some(FeedKind::Workstate) {
            piped.map(str::to_owned)
        } else {
            None
        };
        populate_workstate(&mut snap, routed);
    }

    // Config note (now loaded). Neutral message that makes sense for both CLI and TUI.
    snap.add_note("config: loaded (respects 'enabled' per adapter)".to_owned());

    snap
}

/// Which feed a blob of piped JSON belongs to. We match each feed *positively*
/// (never a silent "else → toolfoundry"), so a future feed lacking a tag is
/// reported Unknown rather than silently misrouted into the wrong consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeedKind {
    ToolFoundry,
    Bulwark,
    ScriptVault,
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

/// Classify piped JSON by content. Bulwark and ScriptVault carry a `source_tool`
/// tag; ToolFoundry has no self-tag, so we positively match its required fields.
/// Every arm is a POSITIVE match — an unrecognized blob is Unknown, never
/// silently misrouted into the wrong consumer.
fn classify_feed(text: &str) -> FeedKind {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return FeedKind::Unknown;
    };
    match v.get("source_tool").and_then(|s| s.as_str()) {
        Some("bulwark") => return FeedKind::Bulwark,
        Some("scriptvault") => return FeedKind::ScriptVault,
        Some("workstate") => return FeedKind::Workstate,
        _ => {}
    }
    // ToolFoundry feed: no source_tool, but tool_count + attention_count + tools.
    let tf = v.get("tool_count").is_some()
        && v.get("attention_count").is_some()
        && v.get("tools").is_some();
    if tf {
        FeedKind::ToolFoundry
    } else {
        FeedKind::Unknown
    }
}

/// Read the Bulwark scan feed and fold it into the snapshot.
///
/// On a supported-version feed this populates `snap.bulwark`, merges a derived
/// risk breakdown into `snap.risk` (the dashboard risk pane), and notes the
/// high-risk items. Missing feeds / unknown versions degrade gracefully.
fn populate_bulwark_feed(snap: &mut OpsSnapshot, routed_stdin: Option<String>) {
    // Use routed stdin text when the piped feed was identified as ours, else the
    // standard path. Either way it's a single acquisition (no stdin re-drain).
    let bwf = match routed_stdin {
        Some(text) => BulwarkFeedAdapter::with_text(text),
        None => BulwarkFeedAdapter::new(),
    };
    let (health, feed) = match bwf.read() {
        Ok(pair) => pair,
        Err(e) => {
            snap.add_note(format!("bulwark-feed: scan unreadable ({e})"));
            (rexops_core::AdapterHealth::Unknown, None)
        }
    };
    if let Ok(id) = AdapterId::new("bulwark-feed") {
        snap.set_adapter_health(&id, health);
    }
    match feed {
        Some(out) => {
            let info = out.data.clone();
            let t = info.risk_tally();
            if t.has_risk_data() {
                // Translate the adapter-local tally into core's RiskSummary and
                // merge it so the existing dashboard risk pane lights up.
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
                    "bulwark: {} items scanned — critical={} high={} medium={} low={} info={}",
                    info.items.len(),
                    t.critical,
                    t.high,
                    t.medium,
                    t.low,
                    t.info
                ));
                for item in info.high_risk_items().take(5) {
                    let sev = item.severity.as_deref().unwrap_or("?");
                    snap.add_note(format!("  high-risk: {} ({})", item.label(), sev));
                }
            } else {
                snap.add_note(format!(
                    "bulwark: {} items scanned — risk breakdown unavailable",
                    info.items.len()
                ));
            }
            snap.bulwark = Some(info);
        }
        None if health == rexops_core::AdapterHealth::Degraded => {
            snap.add_note(
                "bulwark-feed: scan present but unknown/missing schema version — skipped"
                    .to_owned(),
            );
        }
        None => {}
    }
}

/// Read the ToolFoundry feed and fold it into the snapshot.
///
/// Records adapter health, and on a version-1 feed populates `snap.toolfoundry`
/// plus a summary note. Unknown/missing versions and missing feeds are handled
/// gracefully (a note or silence) — never an error that breaks the cockpit.
fn populate_toolfoundry(snap: &mut OpsSnapshot, routed_stdin: Option<String>) {
    // Use routed stdin text when the piped feed was identified as ours, else the
    // standard path. read() returns health + parsed feed from one acquisition.
    let tf = match routed_stdin {
        Some(text) => ToolFoundryAdapter::with_text(text),
        None => ToolFoundryAdapter::new(),
    };
    let (tf_health, feed) = match tf.read() {
        Ok(pair) => pair,
        Err(e) => {
            // Malformed feed or I/O error: note it, do not crash the cockpit.
            snap.add_note(format!("toolfoundry: feed unreadable ({e})"));
            (rexops_core::AdapterHealth::Unknown, None)
        }
    };
    if let Ok(id) = AdapterId::new("toolfoundry") {
        snap.set_adapter_health(&id, tf_health);
    }
    match feed {
        // A version-1 feed was read and parsed: surface the summary.
        Some(out) => {
            let i = &out.data;
            snap.toolfoundry = Some(i.clone());
            snap.add_note(format!(
                "toolfoundry: {} tools, {} need attention (as of {})",
                i.tool_count, i.attention_count, i.as_of
            ));
            for t in i.tools.iter().filter(|t| t.needs_attention()).take(3) {
                snap.add_note(format!(
                    "  attention: {} ({}, {})",
                    t.display_name, t.status, t.lifecycle_state
                ));
            }
        }
        // Feed present but an unknown/missing major version: skip gracefully.
        None if tf_health == rexops_core::AdapterHealth::Degraded => {
            snap.add_note(
                "toolfoundry: feed present but unknown/missing schema version — skipped".to_owned(),
            );
        }
        // No feed found: normal for an optional tool.
        None => {}
    }
}

/// Read the ScriptVault export feed and fold it into the snapshot.
///
/// On a supported-version feed this populates `snap.scriptvault` and notes the
/// script/favorites/recents counts. Unknown/missing versions and missing feeds
/// degrade gracefully.
fn populate_scriptvault(snap: &mut OpsSnapshot, routed_stdin: Option<String>) {
    let sv = match routed_stdin {
        Some(text) => ScriptVaultAdapter::with_text(text),
        None => ScriptVaultAdapter::new(),
    };
    let (sv_health, feed) = match sv.read() {
        Ok(pair) => pair,
        Err(e) => {
            snap.add_note(format!("scriptvault: export unreadable ({e})"));
            (rexops_core::AdapterHealth::Unknown, None)
        }
    };
    if let Ok(id) = AdapterId::new("scriptvault") {
        snap.set_adapter_health(&id, sv_health);
    }
    match feed {
        Some(out) => {
            let i = &out.data;
            snap.add_note(format!(
                "scriptvault: {} scripts, {} favorites, {} recents (as of {})",
                i.total(),
                i.favorites_count(),
                i.recents_count(),
                i.generated_at
            ));
            for s in i.scripts.iter().take(2) {
                let flag = if i.is_favorite(s) { " (favorite)" } else { "" };
                snap.add_note(format!("  script: {}{}", s.label(), flag));
            }
            snap.scriptvault = Some(i.clone());
        }
        None if sv_health == rexops_core::AdapterHealth::Degraded => {
            snap.add_note(
                "scriptvault: export present but unknown/missing schema version — skipped"
                    .to_owned(),
            );
        }
        None => {}
    }
}

/// Read the Workstate snapshot feed and fold it into the snapshot.
///
/// On a supported-version feed this populates `snap.workstate` and notes the
/// project count plus the first couple of project labels. The Workstate contract
/// carries NO risk fields, so this contributes no risk — notes + structured field
/// only. Unknown/missing versions and missing feeds degrade gracefully.
fn populate_workstate(snap: &mut OpsSnapshot, routed_stdin: Option<String>) {
    let ws = match routed_stdin {
        Some(text) => WorkstateAdapter::with_text(text),
        None => WorkstateAdapter::new(),
    };
    let (ws_health, feed) = match ws.read() {
        Ok(pair) => pair,
        Err(e) => {
            snap.add_note(format!("workstate: snapshot unreadable ({e})"));
            (rexops_core::AdapterHealth::Unknown, None)
        }
    };
    if let Ok(id) = AdapterId::new("workstate") {
        snap.set_adapter_health(&id, ws_health);
    }
    match feed {
        Some(out) => {
            let i = &out.data;
            snap.add_note(format!(
                "workstate: {} projects (as of {})",
                i.project_count(),
                i.generated_at
            ));
            for p in i.projects.iter().take(2) {
                snap.add_note(format!("  project: {}", p.label()));
            }
            snap.workstate = Some(i.clone());
        }
        None if ws_health == rexops_core::AdapterHealth::Degraded => {
            snap.add_note(
                "workstate: snapshot present but unknown/missing schema version — skipped"
                    .to_owned(),
            );
        }
        None => {}
    }
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

    let sv_enabled = config
        .adapters
        .get("scriptvault")
        .map_or(true, |c| c.enabled);
    if sv_enabled {
        let sv = ScriptVaultAdapter::new();
        let sv_health = sv.health();
        if let Ok(id) = AdapterId::new("scriptvault") {
            reg.insert(AdapterEntry {
                id,
                health: sv_health,
                label: Some("ScriptVault export consumer (read-only)".to_owned()),
            });
        }
    }

    let tf_enabled = config
        .adapters
        .get("toolfoundry")
        .map_or(true, |c| c.enabled);
    if tf_enabled {
        let tf = ToolFoundryAdapter::new();
        let tf_health = tf.health();
        if let Ok(id) = AdapterId::new("toolfoundry") {
            reg.insert(AdapterEntry {
                id,
                health: tf_health,
                label: Some("ToolFoundry rexops-feed consumer (read-only)".to_owned()),
            });
        }
    }

    let bwf_enabled = config
        .adapters
        .get("bulwark-feed")
        .map_or(true, |c| c.enabled);
    if bwf_enabled {
        let bwf = BulwarkFeedAdapter::new();
        let bwf_health = bwf.health();
        if let Ok(id) = AdapterId::new("bulwark-feed") {
            reg.insert(AdapterEntry {
                id,
                health: bwf_health,
                label: Some("Bulwark scan export consumer (read-only)".to_owned()),
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
// - The enabled checks are duplicated inside the two fns on purpose for now
//   (keeps each fn self-contained and easy to read). If it gets annoying we can
//   extract a tiny helper later without violating "keep it simple".
// - Note texts are now canonical here. TUI used to have a slightly different
//   "TUI using loaded config..." message; we moved that concern into the TUI's
//   own event log after apply_snapshot (see app.rs).
// - Adding a new adapter? One place: add the enabled block in build_snapshot
//   (for data) and in build_adapter_registry (for the `adapters` command).
// - The Adapter trait is used only for .health(), .version(), .info(), .binary().
//   No other adapter internals leak out of this crate.
// - stdin is read ONCE (read_piped_stdin) and routed by content (classify_feed),
//   because stdin is a process singleton. Per-adapter stdin reads would collide:
//   the first consumer would drain the pipe and starve the rest.

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const BULWARK_FEED: &str =
        include_str!("../../rexops-adapters/fixtures/bulwark/scan_feed_v1.json");
    const TOOLFOUNDRY_FEED: &str =
        include_str!("../../rexops-adapters/fixtures/toolfoundry/rexops_feed_v1.json");
    const SCRIPTVAULT_FEED: &str =
        include_str!("../../rexops-adapters/fixtures/scriptvault/export_v1.json");
    const WORKSTATE_FEED: &str =
        include_str!("../../rexops-adapters/fixtures/workstate/snapshot_v1.json");

    #[test]
    fn classify_routes_each_feed_to_its_own_consumer() {
        assert_eq!(classify_feed(BULWARK_FEED), FeedKind::Bulwark);
        assert_eq!(classify_feed(TOOLFOUNDRY_FEED), FeedKind::ToolFoundry);
        assert_eq!(classify_feed(SCRIPTVAULT_FEED), FeedKind::ScriptVault);
        assert_eq!(classify_feed(WORKSTATE_FEED), FeedKind::Workstate);
    }

    #[test]
    fn classify_unknown_blob_is_not_silently_misrouted() {
        // No source_tool and not ToolFoundry-shaped → Unknown, never a default.
        assert_eq!(
            classify_feed(r#"{"schema_version":1,"hello":"world"}"#),
            FeedKind::Unknown
        );
        assert_eq!(classify_feed("not json"), FeedKind::Unknown);
    }

    /// Config that disables the non-feed adapters, leaving the three feed
    /// consumers (toolfoundry, bulwark-feed, scriptvault) enabled so the built
    /// snapshot reflects only feed routing.
    fn feeds_only_config() -> AppConfig {
        let mut cfg = AppConfig::default();
        // Note: scriptvault is now a feed consumer, so it must stay ENABLED here.
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
    /// Points XDG_DATA_HOME at a guaranteed-empty dir so the non-routed consumer's
    /// `populate_*(None)` standard-path read finds nothing — keeps the test
    /// hermetic regardless of what feed files exist on the dev/CI box.
    fn route_via_build(piped: &str) -> OpsSnapshot {
        let empty = std::env::temp_dir().join(format!("rexops-route-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        std::env::set_var("XDG_DATA_HOME", &empty);
        build_snapshot_with_piped(&feeds_only_config(), Some(piped))
    }

    #[test]
    fn build_routes_bulwark_feed_only_to_bulwark() {
        // Exercises the actual routing `if` in build_snapshot_with_piped — the
        // exact site the original stdin collision lived. Swapping the route
        // conditions would flip these assertions and fail the test.
        let snap = route_via_build(BULWARK_FEED);
        assert!(snap.bulwark.is_some(), "bulwark feed must populate bulwark");
        assert!(
            snap.toolfoundry.is_none(),
            "bulwark feed must NOT leak into toolfoundry"
        );
        assert!(
            snap.scriptvault.is_none(),
            "bulwark feed must NOT leak into scriptvault"
        );
        assert!(
            snap.workstate.is_none(),
            "bulwark feed must NOT leak into workstate"
        );
        assert!(snap.risk.critical >= 1, "risk pane should reflect the scan");
    }

    #[test]
    fn build_routes_toolfoundry_feed_only_to_toolfoundry() {
        let snap = route_via_build(TOOLFOUNDRY_FEED);
        assert!(
            snap.toolfoundry.is_some(),
            "toolfoundry feed must populate toolfoundry"
        );
        assert!(
            snap.bulwark.is_none(),
            "toolfoundry feed must NOT leak into bulwark"
        );
        assert!(
            snap.scriptvault.is_none(),
            "toolfoundry feed must NOT leak into scriptvault"
        );
        assert!(
            snap.workstate.is_none(),
            "toolfoundry feed must NOT leak into workstate"
        );
    }

    #[test]
    fn build_routes_scriptvault_feed_only_to_scriptvault() {
        let snap = route_via_build(SCRIPTVAULT_FEED);
        assert!(
            snap.scriptvault.is_some(),
            "scriptvault feed must populate scriptvault"
        );
        assert!(
            snap.toolfoundry.is_none(),
            "scriptvault feed must NOT leak into toolfoundry"
        );
        assert!(
            snap.bulwark.is_none(),
            "scriptvault feed must NOT leak into bulwark"
        );
        assert!(
            snap.workstate.is_none(),
            "scriptvault feed must NOT leak into workstate"
        );
    }

    #[test]
    fn build_routes_workstate_feed_only_to_workstate() {
        let snap = route_via_build(WORKSTATE_FEED);
        assert!(
            snap.workstate.is_some(),
            "workstate feed must populate workstate"
        );
        assert!(
            snap.toolfoundry.is_none(),
            "workstate feed must NOT leak into toolfoundry"
        );
        assert!(
            snap.bulwark.is_none(),
            "workstate feed must NOT leak into bulwark"
        );
        assert!(
            snap.scriptvault.is_none(),
            "workstate feed must NOT leak into scriptvault"
        );
    }
}
