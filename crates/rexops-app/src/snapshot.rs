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
    Adapter, BulwarkAdapter, ScriptVaultAdapter, SystemAdapter, ToolFoundryAdapter,
};
use rexops_core::{AdapterEntry, AdapterId, AdapterRegistry, AppConfig, OpsSnapshot};

/// Build a live OpsSnapshot by probing adapters that are enabled in config.
///
/// Respects the per-adapter `enabled` flag (default true when key absent).
/// Always adds a final "config loaded" note.
/// Populates first-class structured fields (system, scriptvault, toolfoundry)
/// when the corresponding adapter succeeds, plus notes for the dashboard/logs.
///
/// This is the single implementation used by both `rexops status` and the TUI
/// refresh thread.
pub fn build_snapshot(config: &AppConfig) -> OpsSnapshot {
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

    // ScriptVault: stub adapter (read-only metadata/favorites/recents).
    // For demo it always provides sample data; respect 'enabled' like the others.
    let sv_enabled = config
        .adapters
        .get("scriptvault")
        .map_or(true, |c| c.enabled);
    if sv_enabled {
        let sv = ScriptVaultAdapter::new();
        let sv_health = sv.health();
        if let Ok(id) = AdapterId::new("scriptvault") {
            snap.set_adapter_health(&id, sv_health);
        }
        if let Ok(out) = sv.info() {
            let i = &out.data;
            snap.scriptvault = Some(i.clone());
            snap.add_note(format!(
                "scriptvault: {} scripts, {} favorites",
                i.total, i.favorites
            ));
            // Surface first couple of script names for the notes pane / TUI.
            for s in i.scripts.iter().take(2) {
                let flag = if s.favorite { " (favorite)" } else { "" };
                snap.add_note(format!("  script: {}{}", s.name, flag));
            }
        }
    }

    // ToolFoundry: read-only consumer of the `rexops-feed` contract.
    // Reads stdin (when piped) or the documented standard path; never writes back.
    let tf_enabled = config
        .adapters
        .get("toolfoundry")
        .map_or(true, |c| c.enabled);
    if tf_enabled {
        populate_toolfoundry(&mut snap);
    }

    // Config note (now loaded). Neutral message that makes sense for both CLI and TUI.
    snap.add_note("config: loaded (respects 'enabled' per adapter)".to_owned());

    snap
}

/// Read the ToolFoundry feed and fold it into the snapshot.
///
/// Records adapter health, and on a version-1 feed populates `snap.toolfoundry`
/// plus a summary note. Unknown/missing versions and missing feeds are handled
/// gracefully (a note or silence) — never an error that breaks the cockpit.
fn populate_toolfoundry(snap: &mut OpsSnapshot) {
    let tf = ToolFoundryAdapter::new();
    // Single acquisition: stdin can only be drained once, so read() returns both
    // health and the parsed feed together. (Calling health() then info() would
    // consume a piped feed twice and lose the data.)
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
                label: Some("Script metadata / favorites / recents (stub)".to_owned()),
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
