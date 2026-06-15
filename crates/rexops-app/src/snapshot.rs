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

/// The three real adapters RexOps probes — each a distinct data SOURCE with its
/// own backing (a binary, the local host, or a compiled snapshot) and therefore
/// its own [`AdapterHealth`](rexops_core::AdapterHealth).
///
/// This is the single source of truth for "what adapters exist," shared by the
/// snapshot builder and the registry builder so the `status` and `adapters`
/// views can never disagree about the roster again (they used to: `status`
/// listed six because it folded Workstate's scripts/tools/findings *sections*
/// into `adapter_health`, while `adapters` listed only these three).
///
/// scripts/tools/findings are deliberately ABSENT here: they are not adapters,
/// they are sections of the one Workstate snapshot. They carry *freshness*, not
/// health, and are surfaced under Workstate — never as adapters.
const REAL_ADAPTERS: &[&str] = &["bulwark", "system", "workstate"];

/// Whether a real adapter should be probed: it must be one of [`REAL_ADAPTERS`]
/// AND enabled in config. Routing every probe site (in both the snapshot and the
/// registry builder) through this one gate is what makes `REAL_ADAPTERS` the
/// single authoritative roster — an id that isn't in it can't be probed or land
/// in `adapter_health`, so `status` and `adapters` cannot drift apart again.
fn real_adapter_enabled(config: &AppConfig, id: &str) -> bool {
    debug_assert!(
        REAL_ADAPTERS.contains(&id),
        "{id} is not a real adapter; only {REAL_ADAPTERS:?} may be probed"
    );
    REAL_ADAPTERS.contains(&id) && config.adapter_enabled(id)
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
                for d in i.disk.iter().take(2) {
                    snap.add_note(format!("system disk: {d}"));
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
                label: Some("Bulwark content inspection (via inspect scan)".to_owned()),
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
                REAL_ADAPTERS.contains(&id.as_str()),
                "adapter_health contains '{}', which is not a real adapter ({:?})",
                id.as_str(),
                REAL_ADAPTERS
            );
        }
    }

    #[test]
    fn status_and_adapters_views_agree_on_the_roster() {
        // The exact bug from the audit: `status` (adapter_health) and `adapters`
        // (the registry) must list the SAME adapters. With everything enabled,
        // both must equal REAL_ADAPTERS — no more "6 vs 3" disagreement.
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
        let mut expected: Vec<String> = REAL_ADAPTERS.iter().map(|s| (*s).to_owned()).collect();
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
}
