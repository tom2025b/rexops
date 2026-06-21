//! snapshot.rs — Shared builders for OpsSnapshot and AdapterRegistry.
//!
//! This module contains the *only* place that knows how to turn a loaded
//! AppConfig into a live OpsSnapshot by probing the enabled adapters.
//!
//! Snapshot and registry construction lives here so:
//! - There is one source of truth for "what does a snapshot contain?"
//! - The Workstate v3 snapshot is the single source of truth for scripts/tools/findings.
//! - CLI and TUI stay thin (they just call these fns and render).
//!
//! The functions still live in the "app" crate (not core) because they perform
//! side-effecting work (executing adapter probes). Core stays pure data.

use std::time::Duration;

use rexops_adapters::{Adapter, BulwarkAdapter, SystemAdapter, WorkstateAdapter};
use rexops_core::{
    status_to_freshness, AdapterEntry, AdapterId, AdapterRegistry, AppConfig, OpsSnapshot,
    Provenance, RiskSummary, WorkstateInfo,
};

/// The ids the app currently resolves to live health. Derived from the core
/// registry (every id must be a real, non-`Planned` component) intersected with
/// the set this crate actually probes today. The intersection is what preserves
/// behavior parity while the table already lists not-yet-wired feeds
/// (ScriptVault/ToolFoundry) that Phase D will light up.
fn real_adapter_ids() -> Vec<&'static str> {
    // The sources the app resolves in `build_snapshot_with_piped` today.
    const RESOLVED_TODAY: &[&str] = &["bulwark", "system", "workstate"];
    rexops_core::COMPONENTS
        .iter()
        .filter(|c| !matches!(c.health, rexops_core::HealthSource::Planned))
        .map(|c| c.id)
        .filter(|id| RESOLVED_TODAY.contains(id))
        .collect()
}

/// Whether a real adapter should be probed: it must be one of the ids returned
/// by [`real_adapter_ids`] AND enabled in config. Routing every probe site (in
/// both the snapshot and the registry builder) through this one gate is what
/// makes the registry-derived roster the single authoritative source — an id
/// that isn't in it can't be probed or land in `adapter_health`, so `status`
/// and `adapters` cannot drift apart again.
fn real_adapter_enabled(config: &AppConfig, id: &str) -> bool {
    let roster = real_adapter_ids();
    debug_assert!(
        roster.contains(&id),
        "{id} is not a real adapter; only {roster:?} may be probed"
    );
    roster.contains(&id) && config.adapter_enabled(id)
}

/// The configured probe timeout for an adapter, as a `Duration`. Resolves the
/// per-adapter `timeout_secs` override (else the global default) via core, so the
/// configured value is finally honoured — it used to be parsed and ignored.
fn adapter_timeout(config: &AppConfig, id: &str) -> Duration {
    Duration::from_secs(config.adapter_timeout_secs(id))
}

/// Construct a Bulwark adapter wired to its configured binary (if any) and
/// timeout. Shared by the snapshot and registry builders so both probe the
/// SAME adapter the same way.
fn bulwark_adapter(config: &AppConfig) -> BulwarkAdapter {
    let timeout = adapter_timeout(config, "bulwark");
    match config
        .adapters
        .get("bulwark")
        .and_then(|a| a.binary.as_deref())
        .map(str::trim)
        .filter(|b| !b.is_empty())
    {
        Some(binary) => BulwarkAdapter::with_binary(binary),
        None => BulwarkAdapter::new(),
    }
    .with_timeout(timeout)
}

/// Construct a System adapter wired to its configured timeout.
fn system_adapter(config: &AppConfig) -> SystemAdapter {
    SystemAdapter::new().with_timeout(adapter_timeout(config, "system"))
}

/// Build a live OpsSnapshot by probing adapters that are enabled in config,
/// reading the piped stdin (if any) inline.
///
/// This is the entry point for **one-shot** callers like `rexops status`: stdin
/// is a process-lifetime resource that can only be consumed once, so reading it
/// here is correct for a command that builds exactly one snapshot and exits.
///
/// Long-lived callers that refresh repeatedly (the TUI) must NOT use this — a
/// second call would find stdin already drained (silent data-source flip) or, on
/// a pipe that never closes, block forever on the read and never deliver a
/// snapshot. They read stdin once at startup with [`read_piped_stdin`] and pass
/// the captured bytes to [`build_snapshot_with_piped`] on every refresh instead.
///
/// Respects the per-adapter `enabled` flag (default true when key absent).
/// Always adds a final "config loaded" note. Populates first-class structured
/// fields from system probes and Workstate, plus notes for the dashboard/logs.
/// Workstate is the only snapshot input for scripts/tools/findings.
pub fn build_snapshot(config: &AppConfig) -> OpsSnapshot {
    // Thin wrapper: read the single piped stdin (if any), then delegate. The
    // delegate is stdin-free so it can be unit-tested by passing the bytes in.
    build_snapshot_with_piped(config, read_piped_stdin().as_deref())
}

/// Build a snapshot from an explicitly supplied piped-stdin blob (or `None`).
///
/// This is the **repeatable** builder: it touches neither stdin nor process
/// global state, so a caller can invoke it as many times as it likes with the
/// same captured bytes and get identical routing every time. The TUI uses it on
/// every refresh, passing the stdin it captured once at startup — which is what
/// keeps every refresh seeing the same data source and never blocking on a
/// re-read. Also the unit-test seam for the snapshot-routing glue.
pub fn build_snapshot_with_piped(config: &AppConfig, piped: Option<&str>) -> OpsSnapshot {
    let mut snap = OpsSnapshot::new();

    // Bulwark: only probe if enabled in config (defaults to true if absent).
    // ONE probe gives both health and version — no more spawning the binary
    // three times (check_available + version + version) for a single refresh.
    if real_adapter_enabled(config, "bulwark") {
        let bul = bulwark_adapter(config);
        let (health, version) = bul.probe();
        if let Ok(id) = AdapterId::new("bulwark") {
            snap.set_adapter_health(&id, health);

            if health.is_available() {
                if let Some(ver) = version {
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

    // System: respect enabled (default true). Lightweight, always works. A single
    // `info()` call yields health, version, AND the data — reuse all three rather
    // than calling health()/version() again on the side.
    if real_adapter_enabled(config, "system") {
        let sys = system_adapter(config);
        let id = AdapterId::new("system").ok();
        match sys.info() {
            Ok(out) => {
                if let Some(id) = &id {
                    snap.set_adapter_health(id, out.health);
                }
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
                let disk_shown: usize = 2;
                for d in i.disk.iter().take(disk_shown) {
                    snap.add_note(format!("system disk: {d}"));
                }
                if let Some(extra) = i.disk.len().checked_sub(disk_shown).filter(|n| *n > 0) {
                    snap.add_note(format!("system disk: … (+{extra} more)"));
                }
            }
            // info() is effectively infallible, but if it ever errors we still
            // record the adapter (Unavailable) so `system` never silently drops
            // out of the roster.
            Err(_) => {
                if let Some(id) = &id {
                    snap.set_adapter_health(id, rexops_core::AdapterHealth::Unavailable);
                }
            }
        }
    }

    // Workstate v3 is the source of truth for scripts/tools/findings. Piped
    // input is accepted only when it is a recognized Workstate snapshot; any
    // other piped blob is ignored rather than falling back to another path.
    //
    // Match on `piped` alone and classify inside the Some arm, so the route is
    // only ever computed where it exists — there is no "(Some, None)" state to
    // explain away (it was previously an `unreachable!`).
    if real_adapter_enabled(config, "workstate") {
        match piped {
            Some(text) => match classify_snapshot(text) {
                SnapshotKind::Workstate => populate_workstate(&mut snap, Some(text.to_owned())),
                SnapshotKind::Unknown => {
                    snap.add_note("stdin: not a Workstate v3 snapshot — ignored".to_owned());
                }
            },
            None => populate_workstate(&mut snap, None),
        }
    }

    // Config note (now loaded). Neutral message that makes sense for both CLI and TUI.
    snap.add_note("config: loaded (respects 'enabled' per adapter)".to_owned());

    // Project the resolved state into per-component statuses (must be last: it
    // reads adapter_health + the folded fields the blocks above populated).
    registry_walk(&mut snap, config);

    snap
}

/// Project the already-resolved snapshot state into one `ComponentStatus` per
/// registry row. Runs LAST in the build, after the probe blocks have populated
/// `adapter_health` and the structured fields — it re-probes nothing, it only
/// reads what is already there. This is what makes the cockpit, `status`, and
/// `components` all read the same single resolution.
fn registry_walk(snap: &mut OpsSnapshot, config: &AppConfig) {
    use rexops_core::{AdapterHealth, ComponentStatus, HealthSource};

    for comp in rexops_core::COMPONENTS {
        // Health: a Planned source never touches I/O and reads Unknown; every
        // other source's health was already resolved into adapter_health by the
        // probe blocks (or stays Unknown if that source isn't wired this phase).
        let health = match comp.health {
            HealthSource::Planned => AdapterHealth::Unknown,
            _ => snap
                .adapter_health
                .get(comp.id)
                .copied()
                .unwrap_or(AdapterHealth::Unknown),
        };

        let launchable = comp.launch.is_some()
            && config.adapter_enabled(comp.id)
            && health != AdapterHealth::Unavailable;

        snap.push_component(ComponentStatus {
            id: comp.id.to_owned(),
            name: comp.name.to_owned(),
            group: comp.group.label().to_owned(),
            maturity: comp.maturity.label().to_owned(),
            health,
            freshness: component_freshness(snap, comp.id),
            vital: component_vital(snap, comp.id),
            launchable,
        });
    }
}

/// Freshness for a feed-backed component, read from the matching Workstate
/// section's status the fold already produced. `None` for non-feed sources.
///
/// Only the components whose data IS a Workstate section map here:
/// `scriptvault` → the scripts section, `toolfoundry` → the tools section. The
/// `workstate` component itself is the whole-snapshot brain, not any single
/// section — borrowing one section's freshness for it would be incoherent, so it
/// returns `None` and conveys its currency through its vital ("N/3 fresh")
/// instead.
fn component_freshness(snap: &OpsSnapshot, id: &str) -> Option<rexops_core::Freshness> {
    use rexops_core::status_to_freshness;
    let ws = snap.workstate.as_ref()?;
    let status = match id {
        "scriptvault" => ws.scripts.status.as_str(),
        "toolfoundry" => ws.tools.status.as_str(),
        _ => return None,
    };
    Some(status_to_freshness(status))
}

/// The one headline number per component, derived from already-folded data.
/// `None` when there is nothing meaningful to show (e.g. a Planned component).
fn component_vital(snap: &OpsSnapshot, id: &str) -> Option<String> {
    match id {
        "workstate" => snap
            .workstate
            .as_ref()
            .map(|ws| format!("{}/3 fresh", ws.populated_section_count())),
        "bulwark" => snap.findings.as_ref().map(|f| {
            let t = f.risk_tally();
            format!("{} crit {} high", t.critical, t.high)
        }),
        "scriptvault" => snap
            .scripts
            .as_ref()
            .map(|s| format!("{} scripts", s.total())),
        "toolfoundry" => snap
            .tools
            .as_ref()
            .map(|t| format!("{} need review", t.attention_count)),
        "system" => snap.system.as_ref().and_then(|s| s.hostname.clone()),
        _ => None,
    }
}

/// Whether a blob of piped JSON is a Workstate v3 snapshot or something else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotKind {
    Workstate,
    Unknown,
}

/// Read piped stdin once. Returns Some(text) only when stdin is NOT a terminal
/// (i.e. content was piped in) and is non-empty. Errors and empty pipes → None.
///
/// Public so long-lived front-ends can capture the pipe a single time at
/// startup and then feed the captured bytes to [`build_snapshot_with_piped`] on
/// every refresh — stdin is consume-once, so reading it per refresh is a bug
/// (see the `build_snapshot` docs).
pub fn read_piped_stdin() -> Option<String> {
    use std::io::{IsTerminal, Read};
    // Cap the read so a huge or endless pipe can't drive an unbounded allocation
    // (and can't block the TUI's one-time startup read forever). A Workstate
    // snapshot is kilobytes; 16 MiB is orders of magnitude of headroom. `take`
    // truncates at the cap rather than erroring — a snapshot near the cap is
    // unheard of, and a truncated giant blob simply fails the v3 classify and is
    // ignored, which is the right graceful outcome.
    const MAX_PIPED_BYTES: u64 = 16 * 1024 * 1024;

    if std::io::stdin().is_terminal() {
        return None;
    }
    let mut buf = String::new();
    if std::io::stdin()
        .take(MAX_PIPED_BYTES)
        .read_to_string(&mut buf)
        .is_ok()
        && !buf.trim().is_empty()
    {
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

/// Add a section freshness/provenance note like
/// `"section scripts: stale (source observed 2026-06-02)"`.
///
/// A Workstate section's `status` describes how CURRENT its data is, not adapter
/// health — so this records *freshness* and deliberately does NOT write into
/// `adapter_health`. Only the three real adapters (`bulwark`/`system`/
/// `workstate`) ever appear in `adapter_health`; the sections live under the
/// Workstate adapter and surface their currency through these notes (and the
/// typed `Section.status` the screens read).
fn note_section_freshness(
    snap: &mut OpsSnapshot,
    label: &str,
    status: &str,
    provenance: &Provenance,
) {
    let freshness = status_to_freshness(status).label();
    match provenance.source_observed_at.as_deref() {
        Some(src) => snap.add_note(format!(
            "section {label}: {freshness} (source observed {src})"
        )),
        None => snap.add_note(format!("section {label}: {freshness}")),
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
fn fold_ws_tools(snap: &mut OpsSnapshot, info: &WorkstateInfo) {
    let Some(tools) = &info.tools.data else {
        return;
    };
    note_section_freshness(snap, "tools", &info.tools.status, &info.tools.provenance);
    snap.add_note(format!(
        "tools: {} total, {} need attention (as of {})",
        tools.tool_count, tools.attention_count, tools.as_of
    ));
    let attention_shown: usize = 3;
    let attention_total = tools.tools.iter().filter(|t| t.needs_attention()).count();
    for t in tools
        .tools
        .iter()
        .filter(|t| t.needs_attention())
        .take(attention_shown)
    {
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
    if let Some(extra) = attention_total
        .checked_sub(attention_shown)
        .filter(|n| *n > 0)
    {
        snap.add_note(format!("  attention: … (+{extra} more)"));
    }
    snap.tools = Some(tools.clone());
}

/// Fold the snapshot's `scripts` section into `snap.scripts` (+ freshness/notes).
fn fold_ws_scripts(snap: &mut OpsSnapshot, info: &WorkstateInfo) {
    let Some(scripts) = &info.scripts.data else {
        return;
    };
    note_section_freshness(
        snap,
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
fn fold_ws_findings(snap: &mut OpsSnapshot, info: &WorkstateInfo) {
    let Some(findings) = &info.findings.data else {
        return;
    };
    note_section_freshness(
        snap,
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
        let high_risk_shown: usize = 5;
        let high_risk_total = findings.high_risk_items().count();
        for item in findings.high_risk_items().take(high_risk_shown) {
            let sev = item.severity.as_deref().unwrap_or("?");
            snap.add_note(format!("  high-risk: {} ({})", item.label(), sev));
        }
        if let Some(extra) = high_risk_total
            .checked_sub(high_risk_shown)
            .filter(|n| *n > 0)
        {
            snap.add_note(format!("  high-risk: … (+{extra} more)"));
        }
    } else {
        snap.add_note(format!(
            "findings: {} scanned — risk breakdown unavailable",
            findings.items.len()
        ));
    }
    snap.findings = Some(findings.clone());
}

/// Build an AdapterRegistry from live probes.
/// Only includes adapters enabled in config.
///
/// This is intentionally separate from build_snapshot because the `rexops adapters`
/// subcommand only cares about the registry view (health + label), not the full
/// risk/notes/structured data.
pub fn build_adapter_registry(config: &AppConfig) -> AdapterRegistry {
    let mut reg = AdapterRegistry::new();

    if real_adapter_enabled(config, "bulwark") {
        // One probe; reuse the same config-wired adapter the snapshot builder uses.
        let (health, _version) = bulwark_adapter(config).probe();
        if let Ok(id) = AdapterId::new("bulwark") {
            reg.insert(AdapterEntry {
                id,
                health,
                label: Some("Bulwark content inspection (presence/version probe)".to_owned()),
            });
        }
    }

    if real_adapter_enabled(config, "system") {
        let (sys_health, _version) = system_adapter(config).probe();
        if let Ok(id) = AdapterId::new("system") {
            reg.insert(AdapterEntry {
                id,
                health: sys_health,
                label: Some("Lightweight system info (hostname, kernel, uptime, disk)".to_owned()),
            });
        }
    }

    if real_adapter_enabled(config, "workstate") {
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
    fn bulwark_probe_uses_the_configured_binary() {
        // The config `binary` for bulwark must drive the ADAPTER probe (it used to
        // be ignored — the builder always probed plain "bulwark"). Point it at a
        // binary that definitely does not exist → Unavailable; point it at `echo`
        // (always present) → available. Proves config → adapter wiring.
        let mut cfg = AppConfig::default();
        // System + workstate off so we isolate bulwark.
        for name in ["system", "workstate"] {
            cfg.adapters.insert(
                name.to_owned(),
                rexops_core::AdapterConfig {
                    enabled: false,
                    ..Default::default()
                },
            );
        }
        cfg.adapters.insert(
            "bulwark".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("rexops-no-such-bulwark-xyz987".to_owned()),
                timeout_secs: None,
            },
        );
        let snap = build_snapshot_with_piped(&cfg, None);
        let bul = AdapterId::new("bulwark").unwrap();
        assert_eq!(
            snap.adapter_health_of(&bul),
            Some(rexops_core::AdapterHealth::Unavailable),
            "a configured-but-missing bulwark binary must probe Unavailable"
        );

        cfg.adapters.get_mut("bulwark").unwrap().binary = Some("echo".to_owned());
        let snap = build_snapshot_with_piped(&cfg, None);
        assert!(
            snap.adapter_health_of(&bul)
                .is_some_and(|h| h.is_available()),
            "a configured bulwark binary that exists (echo) must probe available"
        );
    }

    #[test]
    fn configured_timeout_bounds_a_hanging_adapter_binary() {
        // THE TIMEOUT-WIRING PROOF: a tiny `timeout_secs` must actually cap the
        // probe. Point bulwark at a script whose `--version` hangs far longer than
        // the configured timeout; the build must return promptly (well under the
        // hang) and report the adapter not-healthy — proving the configured value
        // is threaded into the spawn (it used to be ignored; this would block for
        // the full hang under the old hardcoded 30s default).
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        use std::time::Instant;

        let dir = std::env::temp_dir();
        let path = dir.join(format!("rexops-app-hang-{}", std::process::id()));
        {
            let mut f = std::fs::File::create(&path).unwrap();
            // Ignores all args (including --version) and sleeps well past the
            // timeout. `exec` replaces the shell with `sleep` so there is no
            // grandchild holding the stdout pipe open past the kill — this
            // exercises the timeout-kill path directly (a plain `sleep 30` would
            // leave a grandchild and is a separate exec concern).
            writeln!(f, "#!/bin/sh\nexec sleep 30").unwrap();
            let mut perms = f.metadata().unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }

        let mut cfg = AppConfig::default();
        for name in ["system", "workstate"] {
            cfg.adapters.insert(
                name.to_owned(),
                rexops_core::AdapterConfig {
                    enabled: false,
                    ..Default::default()
                },
            );
        }
        cfg.adapters.insert(
            "bulwark".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some(path.to_string_lossy().into_owned()),
                timeout_secs: Some(1), // 1s cap vs a 30s hang
            },
        );

        let begin = Instant::now();
        let snap = build_snapshot_with_piped(&cfg, None);
        let elapsed = begin.elapsed();
        let _ = std::fs::remove_file(&path);

        // Generous ceiling (probe may spawn twice on the absent-vs-present path,
        // each capped at ~1s) but FAR below the 30s hang the old code would wait.
        assert!(
            elapsed < Duration::from_secs(10),
            "configured 1s timeout must bound the probe; took {elapsed:?}"
        );
        let bul = AdapterId::new("bulwark").unwrap();
        assert_ne!(
            snap.adapter_health_of(&bul),
            Some(rexops_core::AdapterHealth::Healthy),
            "a hanging (timed-out) probe must not report Healthy"
        );
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
    fn sections_are_not_adapters_and_carry_freshness_not_health() {
        // The model fix (UX-1/CR-1): scripts/tools/findings are Workstate
        // SECTIONS, not adapters. They must NOT appear in adapter_health (which
        // is reserved for the real probed sources), and their Stale status must
        // surface as a neutral *freshness* note — never as a health fault.
        let snap = build_via_pipe(WORKSTATE_FEED);
        for id in ["tools", "scripts", "findings"] {
            assert!(
                !snap.adapter_health.contains_key(id),
                "{id} is a section, not an adapter — it must be absent from adapter_health"
            );
        }
        // Freshness is reported as a neutral note, not a Degraded health entry.
        assert!(
            snap.notes
                .iter()
                .any(|n| n == "section tools: stale (source observed 2026-06-02T00:00:00Z)"),
            "a section's staleness must surface as a neutral freshness note, notes were: {:?}",
            snap.notes
        );
    }

    #[test]
    fn adapter_health_roster_only_ever_holds_real_adapters() {
        // The roster guarantee behind UX-1: every key in adapter_health must be
        // one of the three REAL adapters. If a future change re-introduces a
        // synthetic adapter (e.g. folds a section back in), this fails.
        let snap = build_snapshot_with_piped(&AppConfig::default(), Some(WORKSTATE_FEED));
        for id in snap.adapter_health.keys() {
            assert!(
                real_adapter_ids().contains(&id.as_str()),
                "adapter_health contains '{}', which is not a real adapter ({:?})",
                id.as_str(),
                real_adapter_ids()
            );
        }
    }

    #[test]
    fn status_and_adapters_views_agree_on_the_roster() {
        // The exact bug from the audit: `status` (adapter_health) and `adapters`
        // (the registry) must list the SAME adapters. With everything enabled,
        // both must equal the three real adapters — no more "6 vs 3" disagreement.
        let cfg = AppConfig::default();
        let snap = build_snapshot_with_piped(&cfg, Some(WORKSTATE_FEED));
        let reg = build_adapter_registry(&cfg);

        let mut from_status: Vec<String> = snap
            .adapter_health
            .keys()
            .map(|id| id.as_str().to_owned())
            .collect();
        from_status.sort();
        let mut from_registry: Vec<String> = reg
            .list()
            .iter()
            .map(|e| e.id.as_str().to_owned())
            .collect();
        from_registry.sort();
        let mut expected: Vec<String> =
            real_adapter_ids().iter().map(|s| (*s).to_owned()).collect();
        expected.sort();

        assert_eq!(
            from_status, expected,
            "status roster must be exactly the real adapters"
        );
        assert_eq!(
            from_registry, expected,
            "adapters roster must be exactly the real adapters"
        );
    }

    #[test]
    fn real_adapter_roster_is_derived_from_the_registry() {
        // The roster the app probes must be exactly today's three, and every one of
        // them must be a real (non-Planned) component in the core registry — proving
        // the roster is registry-derived, not a hand-maintained duplicate that can
        // drift.
        let mut roster = real_adapter_ids();
        roster.sort_unstable();
        assert_eq!(roster, vec!["bulwark", "system", "workstate"]);

        for id in &roster {
            let c = rexops_core::component_by_id(id)
                .unwrap_or_else(|| panic!("roster id '{id}' missing from COMPONENTS"));
            assert!(
                !matches!(c.health, rexops_core::HealthSource::Planned),
                "roster id '{id}' must have a real health source, not Planned"
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

    #[test]
    fn registry_walk_projects_one_status_per_component() {
        // The walk must emit exactly one ComponentStatus per registry row, in table
        // order, projecting the already-resolved health — never re-probing.
        let snap = build_snapshot_with_piped(&workstate_only_config(), Some(WORKSTATE_FEED));
        assert_eq!(
            snap.components.len(),
            rexops_core::COMPONENTS.len(),
            "one status per registry component"
        );
        // Order matches the table.
        for (status, comp) in snap.components.iter().zip(rexops_core::COMPONENTS) {
            assert_eq!(status.id, comp.id, "component statuses follow table order");
        }
    }

    #[test]
    fn planned_components_are_neutral_not_faulty() {
        // A Planned component (e.g. pulse) must surface as Unknown health and a
        // "planned" maturity — never Healthy (fake green) and never Unavailable
        // (a fault). It is honest, dim, and does no I/O.
        let snap = build_snapshot_with_piped(&workstate_only_config(), Some(WORKSTATE_FEED));
        let pulse = snap
            .components
            .iter()
            .find(|c| c.id == "pulse")
            .expect("pulse is a registry row");
        assert_eq!(pulse.maturity, "planned");
        assert_eq!(pulse.health, rexops_core::AdapterHealth::Unknown);
        assert!(!pulse.launchable, "a planned component is not launchable");
    }

    #[test]
    fn live_workstate_component_reflects_resolved_health() {
        // The workstate component's projected health must equal what the probe block
        // already wrote into adapter_health — proving projection, not re-probe.
        let snap = build_snapshot_with_piped(&workstate_only_config(), Some(WORKSTATE_FEED));
        let ws_health = snap
            .adapter_health
            .get("workstate")
            .copied()
            .expect("workstate probed");
        let ws_component = snap
            .components
            .iter()
            .find(|c| c.id == "workstate")
            .expect("workstate is a registry row");
        assert_eq!(ws_component.health, ws_health);
    }

    #[test]
    fn repeated_calls_with_the_same_piped_bytes_route_identically() {
        // The TUI captures stdin once and feeds the SAME bytes to every refresh.
        // build_snapshot_with_piped must therefore be a pure function of its
        // (config, piped) inputs — calling it twice with the same Workstate blob
        // must populate the structured fields BOTH times. The regression this
        // guards is the old `build_snapshot` reading stdin inline: a second call
        // found the pipe drained and silently fell back to the no-stdin path, so
        // refresh #2 lost the data source that refresh #1 had.
        let cfg = workstate_only_config();
        let first = build_snapshot_with_piped(&cfg, Some(WORKSTATE_FEED));
        let second = build_snapshot_with_piped(&cfg, Some(WORKSTATE_FEED));

        assert!(first.workstate.is_some(), "first call routes the snapshot");
        assert!(
            second.workstate.is_some(),
            "second call with the same bytes must route identically — not fall back to empty"
        );
        assert_eq!(
            first.tools.is_some(),
            second.tools.is_some(),
            "tools routing must be stable across repeated calls"
        );
        assert_eq!(
            first.risk.critical, second.risk.critical,
            "merged risk must be identical across repeated calls"
        );
    }

    #[test]
    fn status_and_adapters_agree_on_the_roster_and_live_is_that_roster_plus_feed_tools() {
        // THE INVARIANT (refined in Phase D): `status`'s adapter_health and
        // `adapters`' registry still agree EXACTLY with the adapter roster
        // (bulwark/system/workstate) — feeds are not adapters. But Phase D widened
        // what "live" means: a feed-backed tool with a launch (ScriptVault,
        // ToolFoundry) is `Live` too, even though it is not adapter-*probed*. So
        // the cockpit's "live" cards are the adapter roster PLUS those feed-backed
        // launchables — a superset, not an equal set. The registry is still one
        // source; "live" is just a richer maturity than "is an adapter".
        let cfg = AppConfig::default();
        let snap = build_snapshot_with_piped(&cfg, Some(WORKSTATE_FEED));
        let reg = build_adapter_registry(&cfg);

        let mut from_adapter_health: Vec<String> = snap
            .adapter_health
            .keys()
            .map(|id| id.as_str().to_owned())
            .collect();
        from_adapter_health.sort();

        let mut from_registry: Vec<String> = reg
            .list()
            .iter()
            .map(|e| e.id.as_str().to_owned())
            .collect();
        from_registry.sort();

        let mut live_components: Vec<String> = snap
            .components
            .iter()
            .filter(|c| c.maturity == "live")
            .map(|c| c.id.clone())
            .collect();
        live_components.sort();

        let mut expected = real_adapter_ids()
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<Vec<_>>();
        expected.sort();

        // The two cross-source rosters still agree exactly with the adapter
        // roster — feeds are not adapters, so adding feed-backed Live tools does
        // not change adapter_health or the registry adapter list.
        assert_eq!(from_adapter_health, expected, "status roster");
        assert_eq!(from_registry, expected, "adapters roster");

        // Phase D: `live` now means "fully wired" — the adapter roster PLUS the
        // feed-backed launchable tools (ScriptVault + ToolFoundry). So the live
        // cards are a SUPERSET of the adapter roster. Assert the exact new set.
        let mut expected_live = expected.clone();
        expected_live.push("scriptvault".to_owned());
        expected_live.push("toolfoundry".to_owned());
        expected_live.sort();
        assert_eq!(live_components, expected_live, "live component cards");
    }
}
