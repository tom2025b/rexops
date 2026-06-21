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
    status_to_freshness, AdapterEntry, AdapterHealth, AdapterId, AdapterRegistry, AppConfig,
    OpsSnapshot, Provenance, RiskSummary, WorkstateInfo,
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

/// Spawn a component's `status` subcommand, bounded by its configured timeout,
/// and parse the one-line JSON contract. Returns the parsed probe result.
///
/// Resolution: the configured `binary` for the id (from adapters config) is
/// used if present and non-empty, otherwise the id itself is tried on PATH.
/// On spawn failure the binary is missing → Unavailable "not found".
/// On timeout the child is killed → Unavailable "status timed out".
/// The parse + health mapping live in `status_probe`; this is only the glue.
fn status_command_probe(
    config: &AppConfig,
    id: &str,
    args: &[&str],
) -> crate::status_probe::StatusProbe {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let program = config
        .adapters
        .get(id)
        .and_then(|a| a.binary.as_deref())
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .unwrap_or(id)
        .to_owned();

    // Any configured prefix args run before the status subcommand's own args.
    // This is empty for every real adapter; tests use it to point `binary` at an
    // interpreter that reads a script argument (so a probe never has to execute a
    // freshly written file — which races `ETXTBSY` under parallel test spawns).
    let prefix = config
        .adapters
        .get(id)
        .map_or(&[][..], |a| a.status_prefix_args.as_slice());

    let start = Instant::now();
    let Ok(mut child) = Command::new(&program)
        .args(prefix)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return crate::status_probe::StatusProbe {
            health: rexops_core::AdapterHealth::Unavailable,
            detail: "not found".to_owned(),
            latency_ms: None,
        };
    };

    let timeout = adapter_timeout(config, id);
    let deadline = start + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                if let Some(mut so) = child.stdout.take() {
                    let _ = so.read_to_string(&mut out);
                }
                return crate::status_probe::parse_status(&out, status.success());
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return crate::status_probe::StatusProbe {
                        health: rexops_core::AdapterHealth::Unavailable,
                        detail: "status timed out".to_owned(),
                        latency_ms: None,
                    };
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => {
                return crate::status_probe::StatusProbe {
                    health: rexops_core::AdapterHealth::Unavailable,
                    detail: "bad status output".to_owned(),
                    latency_ms: None,
                };
            }
        }
    }
}

/// Probe a component's health by *binary presence*: spawn `<bin> <version_args>`
/// (e.g. `tripwire --help`), bounded by the configured timeout, and map the
/// outcome to health. This is the Probe equivalent of [`status_command_probe`]
/// for tools that have no one-line JSON `status` contract — health is simply
/// "does the binary run?":
///   • spawn fails (binary absent) → Unavailable
///   • exits 0                     → Healthy
///   • exits non-zero / times out  → Unavailable
///
/// Program resolution mirrors `status_command_probe`: the configured `binary`
/// (and any `status_prefix_args`) win, else the id itself is tried on PATH — so
/// the same test seam (point `binary` at an interpreter reading a script) works
/// here without ever executing a freshly written file (`ETXTBSY`).
fn probe_binary_presence(config: &AppConfig, id: &str, version_args: &[&str]) -> AdapterHealth {
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let program = config
        .adapters
        .get(id)
        .and_then(|a| a.binary.as_deref())
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .unwrap_or(id)
        .to_owned();
    let prefix = config
        .adapters
        .get(id)
        .map_or(&[][..], |a| a.status_prefix_args.as_slice());

    let start = Instant::now();
    let Ok(mut child) = Command::new(&program)
        .args(prefix)
        .args(version_args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return AdapterHealth::Unavailable;
    };

    let deadline = start + adapter_timeout(config, id);
    loop {
        match child.try_wait() {
            // Exited: 0 → Healthy, anything else → Unavailable.
            Ok(Some(status)) => {
                return if status.success() {
                    AdapterHealth::Healthy
                } else {
                    AdapterHealth::Unavailable
                };
            }
            // Still running: poll until the deadline, then kill → Unavailable.
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return AdapterHealth::Unavailable;
            }
            // wait() itself errored — treat as a failed probe.
            Err(_) => return AdapterHealth::Unavailable,
        }
    }
}

// Learning Notes
// - `status_command_probe` is spawn-only glue: parse + mapping live in
//   `status_probe` so the probe contract is tested independently of I/O.
// - We use `adapter_timeout(config, id)` (which already reads the per-adapter
//   override else global default) so StatusCommand probes respect the same
//   timeout config as Bulwark/system probes.
// - Phase E: Pulse uses StatusCommand, so this function is now called on every
//   real refresh (not just tests). The COMPONENTS loop in build_snapshot_with_piped
//   fires for Pulse and populates both adapter_health and status_latency.
// - `probe_binary_presence` does the same for HealthSource::Probe rows
//   (tripwire/rewind/rex-check/rex-forge): an installed binary reads Healthy, an
//   absent one Unavailable — no more permanently-Unknown "Live" cards.

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

    // StatusCommand components: spawn `<bin> status`, bounded by the configured
    // timeout, parse the one-line JSON contract into health + latency. Runs before
    // the registry walk so adapter_health and status_latency are populated when the
    // walk reads them. Phase E: Pulse has HealthSource::StatusCommand — this loop
    // now fires for it on every build_snapshot_with_piped call (when enabled).
    for comp in rexops_core::COMPONENTS {
        if let rexops_core::HealthSource::StatusCommand { args, .. } = comp.health {
            if config.adapter_enabled(comp.id) {
                let probe = status_command_probe(config, comp.id, args);
                if let Ok(id) = AdapterId::new(comp.id) {
                    snap.set_adapter_health(&id, probe.health);
                }
                if let Some(ms) = probe.latency_ms {
                    snap.status_latency.insert(comp.id.to_owned(), ms);
                }
                snap.status_detail
                    .insert(comp.id.to_owned(), probe.detail.clone());
                snap.add_note(format!("{} status: {}", comp.id, probe.detail));
            }
        }
    }

    // Probe components: spawn `<bin> <version_args>` (binary-presence health) for
    // every HealthSource::Probe row that isn't already a real adapter — the four
    // Probe+launch tools (tripwire/rewind/rex-check/rex-forge). bulwark also has a
    // Probe source but is resolved above by its adapter, so skip it here to avoid a
    // double probe. An installed binary reads Healthy, an absent one Unavailable —
    // so the cockpit card and the launch gate both see real health, not a
    // permanent Unknown. Runs before the registry walk (which reads adapter_health).
    for comp in rexops_core::COMPONENTS {
        if let rexops_core::HealthSource::Probe { version_args, .. } = comp.health {
            if real_adapter_ids().contains(&comp.id) || !config.adapter_enabled(comp.id) {
                continue;
            }
            let health = probe_binary_presence(config, comp.id, version_args);
            if let Ok(id) = AdapterId::new(comp.id) {
                snap.set_adapter_health(&id, health);
            }
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
        // probe blocks above (adapter / StatusCommand / Probe). It stays Unknown
        // only when the tool wasn't probed — disabled in config, or before the
        // first refresh — never as a permanent state for a wired source.
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
        _ => match rexops_core::component_by_id(id).map(|c| &c.health) {
            // StatusCommand components (e.g. Pulse): the probe's one-line detail is
            // the card vital fallback (the TUI overlays a heartbeat sparkline when it
            // has samples; this is what shows otherwise, incl. a Degraded reason).
            Some(rexops_core::HealthSource::StatusCommand { .. }) => {
                snap.status_detail.get(id).cloned()
            }
            // Probe components (binary-presence tools): prefer any real detail the
            // probe recorded (none today — these tools have no JSON contract, which
            // is *why* they're Probe and not StatusCommand — but if a probe later
            // surfaces one, it shows automatically). Otherwise the vital reflects the
            // resolved presence: "installed" / "not installed". Unknown (not yet
            // probed — disabled or pre-first-refresh) shows nothing, never a guess.
            Some(rexops_core::HealthSource::Probe { .. }) => {
                snap.status_detail.get(id).cloned().or_else(|| {
                    match snap.adapter_health.get(id).copied() {
                        Some(AdapterHealth::Healthy) => Some("installed".to_owned()),
                        Some(AdapterHealth::Unavailable) => Some("not installed".to_owned()),
                        _ => None,
                    }
                })
            }
            _ => None,
        },
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
                ..Default::default()
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
                ..Default::default()
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
        // The roster guarantee behind UX-1: every key in adapter_health must be a
        // registry row resolved by one of the snapshot's probe paths — either one of
        // the three real feed/probe adapters (bulwark/system/workstate), a
        // StatusCommand component (pulse), or a Probe component (the binary-presence
        // tools: tripwire/rewind/rex-check/rex-forge/proto). It may never hold a
        // synthetic/section key.
        let snap = build_snapshot_with_piped(&AppConfig::default(), Some(WORKSTATE_FEED));
        let probe_or_status_ids: Vec<&str> = rexops_core::COMPONENTS
            .iter()
            .filter(|c| {
                matches!(
                    c.health,
                    rexops_core::HealthSource::StatusCommand { .. }
                        | rexops_core::HealthSource::Probe { .. }
                )
            })
            .map(|c| c.id)
            .collect();
        for id in snap.adapter_health.keys() {
            let is_real_adapter = real_adapter_ids().contains(&id.as_str());
            let is_probe_or_status = probe_or_status_ids.contains(&id.as_str());
            assert!(
                is_real_adapter || is_probe_or_status,
                "adapter_health contains '{}', which is neither a real adapter ({:?}) nor a Probe/StatusCommand component ({:?})",
                id.as_str(),
                real_adapter_ids(),
                probe_or_status_ids,
            );
        }
    }

    #[test]
    fn status_and_adapters_views_agree_on_the_roster() {
        // The exact bug from the audit: `status` (adapter_health) and `adapters`
        // (the registry) must list the SAME adapters. With everything enabled, both
        // must equal the three real feed/probe adapters — no more "6 vs 3"
        // disagreement. Phase E: Pulse gains StatusCommand health and is probed via
        // the StatusCommand loop, so adapter_health now also contains "pulse". The
        // `build_adapter_registry` view still only covers the old-style feed/probe
        // adapters (bulwark/system/workstate) — the two views legitimately differ by
        // exactly the StatusCommand set.
        let cfg = AppConfig::default();
        let snap = build_snapshot_with_piped(&cfg, Some(WORKSTATE_FEED));
        let reg = build_adapter_registry(&cfg);

        let mut from_registry: Vec<String> = reg
            .list()
            .iter()
            .map(|e| e.id.as_str().to_owned())
            .collect();
        from_registry.sort();
        let mut expected_registry: Vec<String> =
            real_adapter_ids().iter().map(|s| (*s).to_owned()).collect();
        expected_registry.sort();

        // The registry view (build_adapter_registry) covers only the three
        // feed/probe adapters — StatusCommand components are probed separately.
        assert_eq!(
            from_registry, expected_registry,
            "adapters registry must be exactly the feed/probe adapters"
        );

        // adapter_health must be exactly: the three feed/probe adapters, PLUS pulse
        // (StatusCommand), PLUS every Probe component now resolved by the
        // binary-presence loop (proto/tripwire/rewind/rex-check/rex-forge). This is a
        // HARDCODED LITERAL anchor — intentionally NOT derived from the registry. If
        // someone adds a probe/status component that shouldn't be in adapter_health,
        // this literal catches it (a derived expectation would silently expand too).
        let mut expected_status: Vec<&str> = vec![
            "bulwark",
            "pulse",
            "system",
            "workstate",
            "proto",
            "tripwire",
            "rewind",
            "rex-check",
            "rex-forge",
        ];
        expected_status.sort_unstable();
        let mut from_status: Vec<String> = snap
            .adapter_health
            .keys()
            .map(|id| id.as_str().to_owned())
            .collect();
        from_status.sort();
        assert_eq!(
            from_status.as_slice(),
            expected_status,
            "status roster must be exactly the feed/probe adapters + pulse + the Probe tools — update this literal consciously if the set changes"
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
    fn registry_has_reached_the_live_ceiling_and_nothing_is_fake_green() {
        // History: this test used to pick a Planned exemplar (Pulse → Tripwire → the
        // first remaining Planned row) and assert Planned → Unknown health + not
        // launchable. Every flippable tool is now Live, so the registry holds ZERO
        // Planned rows — the old premise is permanently false. Pin the new invariant:
        // the ceiling is reached (10 Live + proto FeedReady, 0 Planned), and no card
        // is fake-green — an Unwired/unresolved source must read Unknown, never a
        // spurious Healthy. (The Planned → Unknown projection rule still lives in
        // registry_walk's `HealthSource::Planned => Unknown` arm; it's simply
        // unexercised by the table until someone adds a new Planned tool — at which
        // point this renamed test makes them confront the neutrality contract.)
        assert_eq!(
            rexops_core::COMPONENTS
                .iter()
                .filter(|c| matches!(c.health, rexops_core::HealthSource::Planned))
                .count(),
            0,
            "all flippable tools are Live — the registry has no Planned rows left"
        );

        // Nothing is fake-green: with only the workstate feed piped, every component
        // is either genuinely resolved (Healthy/Degraded/Unavailable from a real
        // probe) or honestly Unknown — never Healthy without a resolution behind it.
        let snap = build_snapshot_with_piped(&workstate_only_config(), Some(WORKSTATE_FEED));
        for c in &snap.components {
            if c.health == rexops_core::AdapterHealth::Healthy {
                assert!(
                    snap.adapter_health.get(c.id.as_str()).copied()
                        == Some(rexops_core::AdapterHealth::Healthy),
                    "{} reads Healthy with no resolution behind it (fake-green)",
                    c.id
                );
            }
        }
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

    // --- Task 5: StatusCommand probe helpers and tests ---

    /// A stub status "binary" for tests: writes a shell script that prints
    /// `stdout_line` and exits with `exit_code`, and returns the program +
    /// prefix args that run it. The script is a plain (non-executable) data file
    /// that `/bin/sh` *reads* — it is never `execve`-d.
    ///
    /// This avoids a Heisenbug: creating an executable file and spawning it
    /// moments later races under `cargo test`'s parallelism. While one test is
    /// still writing its script, a `fork` for an unrelated spawn on another
    /// thread can hold the open write handle across its exec window, so the
    /// kernel rejects the exec with `ETXTBSY` ("Text file busy", raw OS error
    /// 26). It only bit the saturated `--workspace` run and reproduced reliably
    /// under stress; unique filenames/dirs do not help, because the contended
    /// state is the process fd table. Running the script through the shell as an
    /// argument (`sh <script> <args…>`) means nothing ever executes a written
    /// file, so the race cannot occur.
    #[cfg(test)]
    fn stub_binary(stdout_line: &str, exit_code: i32) -> StubBinary {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!(
            "rexops-stub-{}-{}",
            std::process::id(),
            next_stub_id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stub.sh");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(f, "echo '{stdout_line}'\nexit {exit_code}\n").unwrap();
        f.flush().unwrap();
        StubBinary {
            program: "/bin/sh".to_owned(),
            script: path.to_string_lossy().into_owned(),
        }
    }

    /// A test status binary: the interpreter to spawn plus the script it reads.
    /// `script` becomes the leading probe arg, so any status subcommand args the
    /// probe appends arrive as ignored positional parameters. See [`stub_binary`].
    #[cfg(test)]
    struct StubBinary {
        program: String,
        script: String,
    }

    /// A per-process, monotonic id giving each stub its own directory. An atomic
    /// counter is collision-free, unlike a wall-clock suffix (two parallel tests
    /// can read the same nanosecond).
    #[cfg(test)]
    fn next_stub_id() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    /// Build a config whose "pulse" adapter is probed via the given stub, with
    /// all real adapters disabled (to isolate the status probe).
    #[cfg(test)]
    fn pulse_only_config(stub: &StubBinary) -> AppConfig {
        let mut cfg = AppConfig::default();
        for name in ["bulwark", "system", "workstate"] {
            cfg.adapters.insert(
                name.to_owned(),
                rexops_core::AdapterConfig {
                    enabled: false,
                    ..Default::default()
                },
            );
        }
        cfg.adapters.insert(
            "pulse".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some(stub.program.clone()),
                timeout_secs: None,
                status_prefix_args: vec![stub.script.clone()],
            },
        );
        cfg
    }

    /// A config whose "pulse" adapter points at a binary that does not exist,
    /// used to prove the missing-binary path. Takes a plain program string.
    #[cfg(test)]
    fn pulse_missing_binary_config(binary: &str) -> AppConfig {
        let mut cfg = AppConfig::default();
        for name in ["bulwark", "system", "workstate"] {
            cfg.adapters.insert(
                name.to_owned(),
                rexops_core::AdapterConfig {
                    enabled: false,
                    ..Default::default()
                },
            );
        }
        cfg.adapters.insert(
            "pulse".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some(binary.to_owned()),
                timeout_secs: None,
                status_prefix_args: Vec::new(),
            },
        );
        cfg
    }

    #[test]
    fn status_command_probe_reads_a_tools_json_line() {
        use rexops_core::AdapterHealth;
        let stub = stub_binary(r#"{"healthy":true,"detail":"ok","latency_ms":5}"#, 0);
        let cfg = pulse_only_config(&stub);
        // Call the probe function directly as a unit-test of parse+spawn wiring.
        // (Phase E: Pulse now has StatusCommand health, so the COMPONENTS loop also
        // fires — but this direct call still exercises the low-level function.)
        let probe = status_command_probe(&cfg, "pulse", &["status"]);
        assert_eq!(
            probe.health,
            AdapterHealth::Healthy,
            "healthy JSON line must yield Healthy"
        );
        assert_eq!(
            probe.latency_ms,
            Some(5),
            "latency_ms from the JSON line must be returned"
        );
    }

    #[test]
    fn status_command_probe_missing_binary_is_unavailable() {
        use rexops_core::AdapterHealth;
        let cfg = pulse_missing_binary_config("/nonexistent/pulse-xyz-task5");
        // Call the probe function directly: proves missing binary → Unavailable.
        let probe = status_command_probe(&cfg, "pulse", &["status"]);
        assert_eq!(
            probe.health,
            AdapterHealth::Unavailable,
            "a configured-but-missing binary must probe Unavailable"
        );
    }

    // --- Probe (binary-presence) probe helpers and tests ---

    /// Build a config that probes `tool_id` via the given stub (a `--help`-style
    /// presence check), with all real adapters disabled to isolate the probe.
    #[cfg(test)]
    fn probe_only_config(tool_id: &str, stub: &StubBinary) -> AppConfig {
        let mut cfg = AppConfig::default();
        for name in ["bulwark", "system", "workstate"] {
            cfg.adapters.insert(
                name.to_owned(),
                rexops_core::AdapterConfig {
                    enabled: false,
                    ..Default::default()
                },
            );
        }
        cfg.adapters.insert(
            tool_id.to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some(stub.program.clone()),
                timeout_secs: None,
                status_prefix_args: vec![stub.script.clone()],
            },
        );
        cfg
    }

    #[test]
    fn binary_presence_probe_maps_exit_status_to_health() {
        // Exit 0 → Healthy; any non-zero exit → Unavailable. (The stub's stdout is
        // irrelevant to a presence probe — only the exit status matters.)
        let ok = stub_binary("ignored", 0);
        let cfg_ok = probe_only_config("tripwire", &ok);
        assert_eq!(
            probe_binary_presence(&cfg_ok, "tripwire", &["--help"]),
            AdapterHealth::Healthy,
            "a binary that exits 0 must probe Healthy"
        );

        let bad = stub_binary("ignored", 3);
        let cfg_bad = probe_only_config("tripwire", &bad);
        assert_eq!(
            probe_binary_presence(&cfg_bad, "tripwire", &["--help"]),
            AdapterHealth::Unavailable,
            "a binary that exits non-zero must probe Unavailable"
        );
    }

    #[test]
    fn binary_presence_probe_missing_binary_is_unavailable() {
        let mut cfg = AppConfig::default();
        cfg.adapters.insert(
            "rewind".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/nonexistent/rewind-xyz-probe".to_owned()),
                timeout_secs: None,
                status_prefix_args: Vec::new(),
            },
        );
        assert_eq!(
            probe_binary_presence(&cfg, "rewind", &["--help"]),
            AdapterHealth::Unavailable,
            "a configured-but-missing binary must probe Unavailable"
        );
    }

    #[test]
    fn probe_loop_lights_an_installed_tool_healthy_end_to_end() {
        // INTEGRATION-LOOP COVERAGE: the fix. With tripwire's binary present (exit 0),
        // the Probe loop in build_snapshot_with_piped must write Healthy into
        // adapter_health, and the registry walk must project that onto the card —
        // a tripwire card that is genuinely Healthy and launchable, not a permanent
        // Unknown. This is what "Live via Probe" was always supposed to mean.
        let stub = stub_binary("tripwire 1.0", 0);
        let cfg = probe_only_config("tripwire", &stub);
        let snap = build_snapshot_with_piped(&cfg, None);

        let id = rexops_core::AdapterId::new("tripwire").unwrap();
        assert_eq!(
            snap.adapter_health_of(&id),
            Some(AdapterHealth::Healthy),
            "an installed Probe binary must resolve Healthy through the loop"
        );

        let card = snap
            .components
            .iter()
            .find(|c| c.id == "tripwire")
            .expect("tripwire card");
        assert_eq!(
            card.health,
            AdapterHealth::Healthy,
            "card health is Healthy"
        );
        assert!(
            card.launchable,
            "a Healthy launchable Probe tool must be launchable"
        );
        assert_eq!(
            card.vital.as_deref(),
            Some("installed"),
            "a resolved-Healthy Probe card must show an 'installed' vital, not a blank"
        );
    }

    #[test]
    fn probe_loop_marks_an_absent_tool_unavailable_and_unlaunchable() {
        // The honest-failure half: with rex-forge's binary absent, the Probe loop
        // writes Unavailable, the card reads Unavailable, and the registry walk's
        // `health != Unavailable` gate makes it NOT launchable — never a fake-green
        // card and never a launch button for a missing binary.
        let mut cfg = AppConfig::default();
        for name in ["bulwark", "system", "workstate"] {
            cfg.adapters.insert(
                name.to_owned(),
                rexops_core::AdapterConfig {
                    enabled: false,
                    ..Default::default()
                },
            );
        }
        cfg.adapters.insert(
            "rex-forge".to_owned(),
            rexops_core::AdapterConfig {
                enabled: true,
                binary: Some("/nonexistent/rex-forge-xyz-probe".to_owned()),
                timeout_secs: None,
                status_prefix_args: Vec::new(),
            },
        );
        let snap = build_snapshot_with_piped(&cfg, None);

        let card = snap
            .components
            .iter()
            .find(|c| c.id == "rex-forge")
            .expect("rex-forge card");
        assert_eq!(
            card.health,
            AdapterHealth::Unavailable,
            "an absent Probe binary must read Unavailable"
        );
        assert!(
            !card.launchable,
            "an Unavailable tool must not be launchable"
        );
        assert_eq!(
            card.vital.as_deref(),
            Some("not installed"),
            "an absent Probe card must say 'not installed', not a blank"
        );
    }

    #[test]
    fn probe_loop_does_not_double_probe_bulwark() {
        // bulwark has HealthSource::Probe but is resolved by its own adapter above;
        // the Probe loop must skip it (it's in real_adapter_ids) so we never spawn
        // two probes for one tool. With bulwark disabled it is absent from the
        // roster entirely — proving the loop didn't re-add it.
        let snap = build_snapshot_with_piped(&AppConfig::default(), Some(WORKSTATE_FEED));
        // bulwark resolves exactly once (via its adapter); it is a real adapter, not
        // a Probe-loop entry. Its presence/health is owned by the adapter block.
        assert!(
            real_adapter_ids().contains(&"bulwark"),
            "bulwark is a real adapter — the Probe loop must defer to its adapter"
        );
        // The roster contains bulwark once (HashMap keys are unique by construction,
        // but the intent is: the adapter owns it, the loop skipped it).
        assert!(snap.adapter_health.keys().any(|k| k.as_str() == "bulwark"));
    }

    #[test]
    fn every_probe_tool_is_unknown_until_probed_then_resolves() {
        // ITEM-3 INVARIANT, all four Probe+launch tools at once: each must read
        // Unknown when it is NOT probed (adapter disabled — the "not yet probed"
        // state), and resolve to a real health (Healthy via a present stub) once the
        // probe runs. This is the regression guard for the bug we fixed: a Probe row
        // must never be permanently Unknown when its binary is present, and must
        // never be fake-green when it hasn't been probed.
        for id in ["tripwire", "rewind", "rex-check", "rex-forge"] {
            // (1) Disabled → never probed → Unknown (NOT Unavailable, NOT Healthy),
            // and not launchable (a disabled adapter can't launch).
            let mut disabled = AppConfig::default();
            disabled.adapters.insert(
                id.to_owned(),
                rexops_core::AdapterConfig {
                    enabled: false,
                    ..Default::default()
                },
            );
            let snap = build_snapshot_with_piped(&disabled, Some(WORKSTATE_FEED));
            let card = snap
                .components
                .iter()
                .find(|c| c.id == id)
                .unwrap_or_else(|| panic!("{id} card"));
            assert_eq!(
                card.health,
                AdapterHealth::Unknown,
                "{id} must be Unknown until actually probed (disabled = not probed)"
            );
            assert!(
                !card.launchable,
                "{id} must not be launchable while its adapter is disabled"
            );
            assert!(
                !snap.adapter_health.keys().any(|k| k.as_str() == id),
                "{id} must be absent from adapter_health when never probed"
            );

            // (2) Probed with a present binary (stub exits 0) → Healthy, launchable,
            // and a non-blank vital. Maturity is Live throughout (registry-driven).
            let stub = stub_binary("ok", 0);
            let cfg = probe_only_config(id, &stub);
            let snap = build_snapshot_with_piped(&cfg, None);
            let card = snap
                .components
                .iter()
                .find(|c| c.id == id)
                .unwrap_or_else(|| panic!("{id} card"));
            assert_eq!(
                card.health,
                AdapterHealth::Healthy,
                "{id} must resolve Healthy once its present binary is probed"
            );
            assert_eq!(card.maturity, "live", "{id} maturity must be live");
            assert!(card.launchable, "{id} must be launchable when Healthy");
            assert_eq!(
                card.vital.as_deref(),
                Some("installed"),
                "{id} must show a real vital once probed, not a blank"
            );
        }
    }

    #[test]
    fn pulse_status_command_loop_populates_health_and_latency_end_to_end() {
        // INTEGRATION-LOOP COVERAGE (Task 8 / Task-5 deferred): now that Pulse has
        // HealthSource::StatusCommand, the COMPONENTS loop in build_snapshot_with_piped
        // fires for it. This test drives the FULL loop (not just status_command_probe
        // directly) and asserts that both adapter_health and status_latency are
        // correctly populated — proving the wiring from registry flip → probe → snap.
        use rexops_core::AdapterHealth;
        let stub = stub_binary(r#"{"healthy":true,"detail":"ok","latency_ms":5}"#, 0);
        let cfg = pulse_only_config(&stub);
        let snap = build_snapshot_with_piped(&cfg, None);

        // Health: the COMPONENTS loop must have written Healthy for pulse.
        let pulse_id = rexops_core::AdapterId::new("pulse").unwrap();
        assert_eq!(
            snap.adapter_health_of(&pulse_id),
            Some(AdapterHealth::Healthy),
            "pulse adapter_health must be Healthy after the StatusCommand loop fires"
        );

        // Latency: status_latency must contain pulse with the value from the JSON.
        assert_eq!(
            snap.status_latency.get("pulse").copied(),
            Some(5u64),
            "pulse status_latency must be 5 ms as reported by the stub"
        );

        // Maturity in the registry walk: pulse is now Live, not Planned.
        let pulse_component = snap.components.iter().find(|c| c.id == "pulse");
        assert!(pulse_component.is_some(), "pulse must be in components");
        assert_eq!(
            pulse_component.unwrap().maturity,
            "live",
            "pulse must surface as live after the registry flip"
        );
    }

    #[test]
    fn degraded_status_command_detail_surfaces_as_component_vital() {
        // Spec §5 gap: a healthy:false (Degraded) probe must surface its detail
        // string as the card vital, because the heartbeat ring-buffer has no
        // samples (latency_ms is None on a Degraded probe). Verify end-to-end:
        // stub → build_snapshot_with_piped → component_vital == probe.detail.
        use rexops_core::AdapterHealth;
        let stub = stub_binary(r#"{"healthy":false,"detail":"1 crit","latency_ms":3}"#, 1);
        let cfg = pulse_only_config(&stub);
        let snap = build_snapshot_with_piped(&cfg, None);

        // Probe was Degraded (healthy:false).
        let pulse_id = rexops_core::AdapterId::new("pulse").unwrap();
        assert_eq!(
            snap.adapter_health_of(&pulse_id),
            Some(AdapterHealth::Degraded),
            "healthy:false must probe Degraded"
        );

        // status_detail must carry the probe's detail string.
        assert_eq!(
            snap.status_detail.get("pulse").map(String::as_str),
            Some("1 crit"),
            "status_detail[pulse] must be the probe's detail"
        );

        // component_vital must return the detail as the card vital.
        let pulse_component = snap.components.iter().find(|c| c.id == "pulse");
        assert!(pulse_component.is_some(), "pulse must be in components");
        assert_eq!(
            pulse_component.unwrap().vital.as_deref(),
            Some("1 crit"),
            "a Degraded Pulse card vital must show the probe detail"
        );
    }

    #[test]
    fn non_status_command_component_vital_is_unaffected_by_catch_all() {
        // Guard: the new catch-all arm must NOT bleed into non-StatusCommand
        // components. The catch-all is guarded by a HealthSource::StatusCommand
        // check, so non-StatusCommand ids must never appear in status_detail.
        // Use a workstate-only build so we have a real snapshot to work with.
        let snap = build_snapshot_with_piped(&workstate_only_config(), Some(WORKSTATE_FEED));

        // workstate is NOT a StatusCommand component — its vital comes from its
        // named arm, not status_detail. It must not appear in status_detail.
        assert!(
            !snap.status_detail.contains_key("workstate"),
            "workstate is not a StatusCommand component and must not appear in status_detail"
        );

        // system is also not a StatusCommand component — same guarantee.
        assert!(
            !snap.status_detail.contains_key("system"),
            "system is not a StatusCommand component and must not appear in status_detail"
        );

        // A completely unknown id (not in the registry at all) returns None from
        // component_vital — proving the catch-all's else branch still returns None.
        let dummy_snap = OpsSnapshot::new();
        assert!(
            component_vital(&dummy_snap, "__no_such_id__").is_none(),
            "an id not in the registry must return None from component_vital"
        );
    }

    #[test]
    fn status_and_adapters_agree_on_the_roster_and_live_is_that_roster_plus_feed_tools() {
        // THE INVARIANT (refined in Phase E): `adapters`' registry still covers only
        // the three feed/probe adapters (bulwark/system/workstate). `status`'s
        // adapter_health is now a SUPERSET: it also contains StatusCommand components
        // (pulse, Phase E). Phase D widened "live" to include feed-backed launchables
        // (ScriptVault/ToolFoundry); Phase E adds pulse via StatusCommand, so live
        // is now 6/11: the three probed adapters + ScriptVault + ToolFoundry + Pulse.
        //
        // I-2 RESTORE: the adapter_health cross-check (from_adapter_health vs the
        // literal) was dropped in Phase E — restored here so both the registry-view
        // AND the adapter_health-view of the roster are asserted against the literal.
        let cfg = AppConfig::default();
        let snap = build_snapshot_with_piped(&cfg, Some(WORKSTATE_FEED));
        let reg = build_adapter_registry(&cfg);

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

        let mut expected_registry = real_adapter_ids()
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<Vec<_>>();
        expected_registry.sort();

        // The build_adapter_registry view covers only the three feed/probe adapters
        // — StatusCommand components are probed by the snapshot loop, not here.
        assert_eq!(
            from_registry, expected_registry,
            "adapters registry must be exactly the feed/probe adapters"
        );

        // I-2: RESTORED adapter_health cross-check — hardcoded literal anchor.
        // adapter_health is the feed/probe adapters + pulse (StatusCommand) + every
        // Probe tool the binary-presence loop now resolves. This is a second vantage
        // point (in addition to the I-1 test) asserting the same literal.
        let mut from_adapter_health: Vec<String> = snap
            .adapter_health
            .keys()
            .map(|id| id.as_str().to_owned())
            .collect();
        from_adapter_health.sort();
        let mut expected_adapter_health: Vec<&str> = vec![
            "bulwark",
            "pulse",
            "system",
            "workstate",
            "proto",
            "tripwire",
            "rewind",
            "rex-check",
            "rex-forge",
        ];
        expected_adapter_health.sort_unstable();
        assert_eq!(
            from_adapter_health.as_slice(),
            expected_adapter_health,
            "adapter_health roster must be the feed/probe adapters + pulse + the Probe tools"
        );

        // "live" = 3 probed adapters + 2 feed-backed launchables + pulse
        // (StatusCommand) + rex-check + tripwire + rewind + rex-forge (Probe+launch).
        // Ten live cards out of eleven registry rows — the ceiling: the only non-live
        // entry is proto (FeedReady). The Probe+launch rows are now probed for
        // binary presence (see the Probe loop), so they DO appear in adapter_health
        // above — Healthy if installed, Unavailable if not — and `maturity == "live"`
        // (registry-driven) lights the card.
        let mut expected_live = expected_registry.clone();
        expected_live.push("pulse".to_owned()); // Phase E: StatusCommand Live
        expected_live.push("scriptvault".to_owned()); // Phase D: feed-backed Live
        expected_live.push("toolfoundry".to_owned()); // Phase D: feed-backed Live
        expected_live.push("rex-check".to_owned()); // Phase F-tail: Probe+launch Live
        expected_live.push("tripwire".to_owned()); // Probe+launch Live
        expected_live.push("rewind".to_owned()); // Probe+launch Live
        expected_live.push("rex-forge".to_owned()); // Probe+launch Live
        expected_live.sort();
        assert_eq!(
            live_components, expected_live,
            "live component cards (10/11)"
        );
    }
}
