//! Parse a tool's `status` contract — one JSON line `{healthy, detail,
//! latency_ms}` — into health. Pure over `(stdout, exit_ok)` so every outcome
//! (good, unhealthy, garbled, empty) is unit-tested without spawning anything.
//! The spawn + timeout live in `snapshot.rs`; this is only the parse + mapping.

use rexops_core::AdapterHealth;
use serde::Deserialize;

/// The parsed outcome of a `status` probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusProbe {
    pub health: AdapterHealth,
    pub detail: String,
    pub latency_ms: Option<u64>,
}

/// The wire shape emitted by `pulse status` (and any future StatusCommand tool).
#[derive(Debug, Deserialize)]
struct Wire {
    healthy: bool,
    #[serde(default)]
    detail: String,
    #[serde(default)]
    latency_ms: u64,
}

/// Map a tool's status output to health. The first stdout line must be the JSON
/// contract; anything else (empty, non-JSON) is `Unavailable`. `exit_ok` is
/// advisory — the JSON `healthy` field is authoritative when present.
pub fn parse_status(stdout: &str, _exit_ok: bool) -> StatusProbe {
    let first = stdout.lines().next().unwrap_or("").trim();
    match serde_json::from_str::<Wire>(first) {
        Ok(w) => StatusProbe {
            health: if w.healthy {
                AdapterHealth::Healthy
            } else {
                AdapterHealth::Degraded
            },
            detail: w.detail,
            latency_ms: Some(w.latency_ms),
        },
        Err(_) => StatusProbe {
            health: AdapterHealth::Unavailable,
            detail: "bad status output".to_owned(),
            latency_ms: None,
        },
    }
}

// Learning Notes
// - `exit_ok` is kept in the signature (snapshot.rs passes it) but the JSON
//   `healthy` flag wins — a tool that prints a valid line is trusted over its
//   exit code, and a tool that prints garbage is Unavailable regardless of code.
// - serde_json is already a workspace dep; no new crate.

#[cfg(test)]
mod tests {
    use super::*;
    use rexops_core::AdapterHealth;

    #[test]
    fn healthy_line_parses_to_healthy_with_latency() {
        let p = parse_status(
            r#"{"healthy":true,"detail":"all clear","latency_ms":7}"#,
            true,
        );
        assert_eq!(p.health, AdapterHealth::Healthy);
        assert_eq!(p.detail, "all clear");
        assert_eq!(p.latency_ms, Some(7));
    }

    #[test]
    fn unhealthy_line_parses_to_degraded_keeping_detail() {
        let p = parse_status(
            r#"{"healthy":false,"detail":"1 crit","latency_ms":9}"#,
            false,
        );
        assert_eq!(p.health, AdapterHealth::Degraded);
        assert_eq!(p.detail, "1 crit");
        assert_eq!(p.latency_ms, Some(9));
    }

    #[test]
    fn garbage_stdout_is_unavailable() {
        let p = parse_status("not json", false);
        assert_eq!(p.health, AdapterHealth::Unavailable);
        assert_eq!(p.detail, "bad status output");
        assert_eq!(p.latency_ms, None);
    }

    #[test]
    fn empty_stdout_is_unavailable() {
        let p = parse_status("", false);
        assert_eq!(p.health, AdapterHealth::Unavailable);
        assert_eq!(p.detail, "bad status output");
    }

    #[test]
    fn extra_lines_use_the_first_json_line() {
        let p = parse_status(
            "{\"healthy\":true,\"detail\":\"ok\",\"latency_ms\":1}\nnoise\n",
            true,
        );
        assert_eq!(p.health, AdapterHealth::Healthy);
        assert_eq!(p.detail, "ok");
    }
}
