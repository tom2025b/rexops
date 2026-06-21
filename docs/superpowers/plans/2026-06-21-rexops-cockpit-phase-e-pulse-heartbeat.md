# RexOps Cockpit Phase E — Pulse / Heartbeat Monitor — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Light up Pulse as a `Live`, launchable cockpit card whose vital is a heartbeat sparkline, by adding a `pulse status` JSON contract, a `StatusCommand` health handler in RexOps, and a shared suite-ui `Heartbeat` widget.

**Architecture:** Three repos, three PRs, in order. (1) **linux-ops-suite**: Pulse gains a read-only `pulse status` subcommand emitting one JSON line `{healthy, detail, latency_ms}`, reusing its existing snapshot→`Verdict` pipeline. (2) **umbrella → suite-ui**: a pure-render `Heartbeat` widget. (3) **rexops**: a `StatusCommand` probe block + parse in `snapshot.rs`, a transient latency ring buffer in the TUI app, the Pulse card rendering the heartbeat, the Phase C drill-down showing history, and the Pulse registry row flipped to `Live`.

**Tech Stack:** Rust (stdlib + serde + serde_json already in the workspaces), ratatui (via suite-ui), the existing rexops `AppConfig`/adapter probe machinery.

## Global Constraints

- **Four cargo gates green at EVERY commit:** `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace -- -D warnings` (exit 0), `cargo fmt --all --check`.
- **No binary installation, no wrappers, no aliases.** The Pulse card launches the bare `pulse` binary; binary resolution stays `which <id>` → configured binary. (Honour the bare-binary rule; do NOT create `~/bin/r-pulse` or aliases.)
- **No persistence.** The heartbeat ring buffer is in-memory and transient.
- **No new screens.** The Phase C `CockpitDetail` screen is reused for Pulse's history.
- **No auto-poll.** Heartbeat fills on manual `r` + on-launch refresh only.
- **suite-ui CI ordering rule:** suite-ui changes land via a PR to umbrella `main` FIRST; THEN bump the rexops pin (`crates/rexops-tui/Cargo.toml`, currently `rev = "2f5fa8231ea85f0827d11aef1cb13d3ddb347249"`) in a separate rexops PR.
- **Approval gate:** do NOT push or open any PR without explicit per-action approval.
- Each `.rs` stays focused (<~300 LOC) with a `// Learning Notes` footer matching the surrounding files.

---

## PR 1 — `pulse status` contract  (repo: linux-ops-suite)

Branch: `pulse-status-contract` off umbrella `main`.

### Task 1: `StatusReport` type + serialization

**Files:**
- Create: `crates/pulse/src/status.rs`
- Modify: `crates/pulse/src/main.rs` (add `mod status;` near the other `mod` lines)
- Test: inline `#[cfg(test)]` in `crates/pulse/src/status.rs`

**Interfaces:**
- Consumes: `crate::verdict::{Verdict, State}` (existing: `Verdict { state: State, causes: Vec<Cause>, .. }`, `State::{Healthy, NeedsAttention, Incomplete}`; `Cause { what, why, source }`).
- Produces: `pub struct StatusReport { pub healthy: bool, pub detail: String, pub latency_ms: u64 }`; `pub fn StatusReport::from_verdict(v: &Verdict, latency_ms: u64) -> StatusReport`; `pub fn StatusReport::to_json_line(&self) -> String`; `pub fn StatusReport::exit_code(&self) -> std::process::ExitCode`.

- [ ] **Step 1: Write the failing test**

```rust
// crates/pulse/src/status.rs  (bottom)
#[cfg(test)]
mod tests {
    use super::*;
    use crate::verdict::{Cause, State, Verdict};

    fn verdict(state: State, causes: Vec<Cause>) -> Verdict {
        Verdict {
            state,
            age: "1m ago".to_owned(),
            critical: 0,
            high: 0,
            confidence_reduced: false,
            unavailable: 0,
            stale: 0,
            causes,
            sources: Vec::new(),
        }
    }

    #[test]
    fn healthy_verdict_serializes_to_one_json_line_and_exits_zero() {
        let r = StatusReport::from_verdict(&verdict(State::Healthy, Vec::new()), 7);
        assert_eq!(
            r.to_json_line(),
            r#"{"healthy":true,"detail":"all clear","latency_ms":7}"#
        );
        assert!(!r.to_json_line().contains('\n'));
        assert_eq!(r.exit_code(), std::process::ExitCode::SUCCESS);
    }

    #[test]
    fn attention_verdict_uses_top_cause_as_detail_and_exits_one() {
        let causes = vec![Cause {
            what: "bulwark".to_owned(),
            why: "1 critical finding".to_owned(),
            source: "bulwark".to_owned(),
        }];
        let r = StatusReport::from_verdict(&verdict(State::NeedsAttention, causes), 12);
        assert_eq!(r.healthy, false);
        assert_eq!(r.detail, "bulwark: 1 critical finding");
        assert_eq!(r.exit_code(), std::process::ExitCode::from(1));
    }

    #[test]
    fn incomplete_with_no_causes_is_not_healthy_and_has_a_detail() {
        let r = StatusReport::from_verdict(&verdict(State::Incomplete, Vec::new()), 3);
        assert_eq!(r.healthy, false);
        assert_eq!(r.detail, "snapshot incomplete");
        assert_eq!(r.exit_code(), std::process::ExitCode::from(1));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p pulse status::tests`
Expected: FAIL — `cannot find ... StatusReport` / module `status` not declared.

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/pulse/src/status.rs  (top)
//! `pulse status` — a machine-readable liveness line for parent processes
//! (RexOps' StatusCommand health source). Read-only, non-interactive: it reuses
//! the same snapshot→Verdict pipeline the screens use and serializes the result
//! instead of rendering it. One JSON line, then exit 0 (healthy) / 1 (not).

use std::process::ExitCode;

use serde::Serialize;

use crate::verdict::{State, Verdict};

/// The tiny contract RexOps parses. Stable: add fields only additively.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusReport {
    /// True only when the verdict is fully healthy.
    pub healthy: bool,
    /// A short human reason (the top cause, or a state summary).
    pub detail: String,
    /// Wall-time Pulse spent reading its snapshot + computing the verdict.
    pub latency_ms: u64,
}

impl StatusReport {
    /// Derive the report from an already-computed verdict. No new health logic:
    /// `healthy` mirrors `State::Healthy`; `detail` is the top cause if any,
    /// else a one-line summary of the state.
    pub fn from_verdict(v: &Verdict, latency_ms: u64) -> Self {
        let healthy = matches!(v.state, State::Healthy);
        let detail = match v.causes.first() {
            Some(c) => format!("{}: {}", c.what, c.why),
            None => match v.state {
                State::Healthy => "all clear".to_owned(),
                State::NeedsAttention => "needs attention".to_owned(),
                State::Incomplete => "snapshot incomplete".to_owned(),
            },
        };
        StatusReport {
            healthy,
            detail,
            latency_ms,
        }
    }

    /// The single JSON line printed on stdout (no trailing newline added here;
    /// the caller uses `println!`).
    pub fn to_json_line(&self) -> String {
        // serde_json never fails for this plain struct.
        serde_json::to_string(self).expect("StatusReport is always serializable")
    }

    /// Process exit code: success when healthy, `1` otherwise.
    pub fn exit_code(&self) -> ExitCode {
        if self.healthy {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        }
    }
}

// Learning Notes
// - `detail` reuses the verdict's existing `causes`/`state`; Pulse gains no new
//   health model — the contract is a *view* of the verdict, like the screens.
// - serde_json is already a workspace dependency (used by the contract readers);
//   no new crate is introduced.
```

Then declare the module in `main.rs` (alphabetical with the other `mod` lines, e.g. after `mod sources;`):

```rust
mod status;
```

Add the deps to `crates/pulse/Cargo.toml` ONLY if missing (check first with `grep -E 'serde|serde_json' crates/pulse/Cargo.toml`):

```toml
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p pulse status::tests`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/pulse/src/status.rs crates/pulse/src/main.rs crates/pulse/Cargo.toml
git commit -m "feat(pulse): StatusReport — verdict→JSON contract for parent processes

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: wire `pulse status` into the arg matcher (timed, headless)

**Files:**
- Modify: `crates/pulse/src/main.rs` (the `while i < args.len()` match in `fn main`, around the `"--dump-view"` arm; and the `HELP` text)
- Test: `crates/pulse/tests/status_cli.rs` (new integration test running the built binary)

**Interfaces:**
- Consumes: `crate::status::StatusReport`; existing `verdict::Readings::load(&sources::DataDir::resolve())`; existing `app::App::new(readings, theme)` and `app.verdict()` accessor (`pub(crate) fn verdict(&self) -> &Verdict`).
- Produces: CLI behaviour — `pulse status` prints one JSON line and exits 0/1.

- [ ] **Step 1: Write the failing test**

```rust
// crates/pulse/tests/status_cli.rs
//! `pulse status` end-to-end: a real subprocess, an isolated empty data dir, so
//! the contract (one JSON line + exit code) is verified exactly as RexOps will
//! invoke it.

use std::process::Command;

fn run_status(data_dir: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_pulse"))
        .arg("status")
        .env("PULSE_DATA_DIR", data_dir)
        .env("NO_COLOR", "1")
        .output()
        .expect("run pulse status")
}

#[test]
fn status_prints_one_json_line_with_the_contract_fields() {
    let tmp = std::env::temp_dir().join(format!("pulse-status-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();

    let out = run_status(&tmp);
    let stdout = String::from_utf8_lossy(&out.stdout);

    // exactly one line
    assert_eq!(stdout.lines().count(), 1, "stdout was:\n{stdout}");
    // valid JSON carrying the three contract fields
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON line");
    assert!(v.get("healthy").and_then(|x| x.as_bool()).is_some());
    assert!(v.get("detail").and_then(|x| x.as_str()).is_some());
    assert!(v.get("latency_ms").and_then(|x| x.as_u64()).is_some());

    // an empty data dir → not healthy → exit 1
    assert_eq!(out.status.code(), Some(1), "stdout:\n{stdout}");

    let _ = std::fs::remove_dir_all(&tmp);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p pulse --test status_cli`
Expected: FAIL — `pulse status` currently hits the `other =>` arm and exits 2 with "unexpected argument 'status'".

- [ ] **Step 3: Write minimal implementation**

Add a `status` arm to the match in `fn main` (place it just before the `other =>` arm). It must run BEFORE the interactive/dump branches and return directly:

```rust
            "status" => {
                // Machine-readable liveness for parent processes (RexOps). Reuse
                // the same readings→verdict the screens use; time only that work.
                use std::time::Instant;
                let start = Instant::now();
                let readings = verdict::Readings::load(&sources::DataDir::resolve());
                let theme = Theme::resolve(color_choice, theme_choice);
                let app = app::App::new(readings, theme);
                let latency_ms = start.elapsed().as_millis() as u64;
                let report = status::StatusReport::from_verdict(app.verdict(), latency_ms);
                println!("{}", report.to_json_line());
                return report.exit_code();
            }
```

Then add a line to the `HELP` constant's command list (next to `--dump-view`):

```
//!   pulse status          one JSON liveness line, then exit (for RexOps)
```
(Update both the `//!` doc header list and the runtime `HELP` string if they are separate — grep `HELP` to confirm; keep them in sync.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p pulse --test status_cli`
Expected: PASS.

- [ ] **Step 5: Full gates + commit**

Run: `cargo build --workspace && cargo test -p pulse && cargo clippy -p pulse -- -D warnings && cargo fmt --all --check`
Expected: all green.

```bash
git add crates/pulse/src/main.rs crates/pulse/tests/status_cli.rs
git commit -m "feat(pulse): add 'pulse status' — one JSON liveness line for RexOps

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

> **PR 1 boundary:** run the four workspace gates, then STOP for approval to push + open the linux-ops-suite PR. Do not proceed to PR 2 until PR 1 is merged.

---

## PR 2 — `Heartbeat` suite-ui widget  (repo: linux-ops-suite umbrella → suite-ui)

Branch: `suite-ui-heartbeat` off umbrella `main`.

### Task 3: `Heartbeat` widget (pure render)

**Files:**
- Create: `crates/suite-ui/src/heartbeat.rs`
- Modify: `crates/suite-ui/src/lib.rs` (add `mod heartbeat;` and `pub use heartbeat::Heartbeat;`)
- Test: inline `#[cfg(test)]` in `heartbeat.rs` + one case in `crates/suite-ui/tests/snapshots.rs`

**Interfaces:**
- Consumes: `crate::theme::{Theme, Health}` (existing); ratatui `text::{Line, Span}`.
- Produces: `pub struct Heartbeat<'a> { pub samples: &'a [u64], pub latest_ms: Option<u64> }`; `pub fn Heartbeat::sparkline(samples: &[u64]) -> String`; `pub fn Heartbeat::text(&self) -> String`; `pub fn Heartbeat::line(&self, theme: Theme) -> Line<'static>`.

- [ ] **Step 1: Write the failing test**

```rust
// crates/suite-ui/src/heartbeat.rs  (bottom)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparkline_maps_samples_across_eight_levels() {
        // min→lowest block, max→highest; flat input → all the same mid-ish block.
        assert_eq!(Heartbeat::sparkline(&[0, 7, 15]), "▁▄█");
        assert_eq!(Heartbeat::sparkline(&[]), "");
        assert_eq!(Heartbeat::sparkline(&[5, 5, 5]), "▁▁▁");
    }

    #[test]
    fn text_pairs_heart_sparkline_and_latest_latency() {
        let hb = Heartbeat {
            samples: &[1, 4, 9],
            latest_ms: Some(9),
        };
        assert_eq!(hb.text(), "♥ ▁▄█ 9ms");
    }

    #[test]
    fn text_with_no_samples_shows_only_heart_and_latency() {
        let hb = Heartbeat {
            samples: &[],
            latest_ms: Some(7),
        };
        assert_eq!(hb.text(), "♥ 7ms");
    }

    #[test]
    fn text_with_nothing_is_just_the_heart() {
        let hb = Heartbeat {
            samples: &[],
            latest_ms: None,
        };
        assert_eq!(hb.text(), "♥");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p suite-ui heartbeat::tests`
Expected: FAIL — `Heartbeat` undefined.

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/suite-ui/src/heartbeat.rs  (top)
//! Heartbeat: a liveness sparkline (`♥ ▁▂▅▇▅▂ 12ms`) for monitor-style cards.
//!
//! The one genuinely novel cockpit vital: a heart glyph, a Unicode block
//! sparkline of recent samples, and the latest value. Pure like
//! [`SeverityBadge`](crate::SeverityBadge) — it yields a styled [`Line`], not a
//! region you draw on its own; a card folds it into the cell where its vital
//! goes. Empty input degrades gracefully so a card with no samples yet still
//! reads cleanly.

use ratatui::text::{Line, Span};

use crate::theme::{Health, Theme};

/// The eight block glyphs, low→high.
const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A heartbeat vital over recent latency samples (oldest→newest) plus the
/// latest reading. Cheap to build; borrow the samples.
#[derive(Debug, Clone, Copy)]
pub struct Heartbeat<'a> {
    /// Recent latency samples, oldest first.
    pub samples: &'a [u64],
    /// The most recent latency, shown as `Nms`. `None` hides the number.
    pub latest_ms: Option<u64>,
}

impl<'a> Heartbeat<'a> {
    /// Map samples onto the eight block glyphs by min→max range. Empty → "".
    /// A flat series maps to the lowest block (no variation to show).
    pub fn sparkline(samples: &[u64]) -> String {
        if samples.is_empty() {
            return String::new();
        }
        let min = *samples.iter().min().unwrap();
        let max = *samples.iter().max().unwrap();
        let span = max.saturating_sub(min);
        samples
            .iter()
            .map(|&s| {
                let idx = if span == 0 {
                    0
                } else {
                    // 0..=7 across the range
                    (((s - min) * 7) / span) as usize
                };
                BLOCKS[idx]
            })
            .collect()
    }

    /// The textual vital, pure for tests/reuse: `♥`, then the sparkline (if any),
    /// then the latest latency (if any).
    pub fn text(&self) -> String {
        let mut out = String::from("♥");
        let spark = Self::sparkline(self.samples);
        if !spark.is_empty() {
            out.push(' ');
            out.push_str(&spark);
        }
        if let Some(ms) = self.latest_ms {
            out.push_str(&format!(" {ms}ms"));
        }
        out
    }

    /// The vital as a styled one-span [`Line`], painted in the healthy accent
    /// (gated by `NO_COLOR` via `Theme`). The glyphs carry the meaning textually
    /// when colour is off.
    pub fn line(&self, theme: Theme) -> Line<'static> {
        Line::from(Span::styled(self.text(), theme.health(Health::Healthy)))
    }
}

// Learning Notes
// - Pure render: `text`/`sparkline` are testable without a backend; `line` is the
//   only styled surface, matching the badge/strip widgets.
// - Range-normalised sparkline (min→max), so a steady ~7ms heartbeat still shows
//   a visible trace rather than a flat line — except a truly constant series,
//   which honestly reads flat.
```

Confirm `theme.health(Health::Healthy)` exists (grep `fn health` in `crates/suite-ui/src/theme.rs`); if the method/enum names differ, use the exact ones the file defines. Then in `lib.rs`:

```rust
mod heartbeat;
// ... with the other pub use lines:
pub use heartbeat::Heartbeat;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p suite-ui heartbeat::tests`
Expected: PASS (4 tests).

- [ ] **Step 5: Add a snapshot test + commit**

Add to `crates/suite-ui/tests/snapshots.rs` (match the file's existing harness — most cases assert a rendered/asserted string; mirror the nearest existing widget case):

```rust
#[test]
fn heartbeat_vital_renders_heart_sparkline_and_latency() {
    use suite_ui::Heartbeat;
    let hb = Heartbeat { samples: &[2, 5, 9, 4], latest_ms: Some(4) };
    assert_eq!(hb.text(), "♥ ▁▄█▃ 4ms");
}
```

Run: `cargo build --workspace && cargo test -p suite-ui && cargo clippy -p suite-ui -- -D warnings && cargo fmt --all --check`
Expected: all green.

```bash
git add crates/suite-ui/src/heartbeat.rs crates/suite-ui/src/lib.rs crates/suite-ui/tests/snapshots.rs
git commit -m "feat(suite-ui): Heartbeat widget — liveness sparkline vital

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

> **PR 2 boundary:** run the gates, STOP for approval to push + open the umbrella PR. After it merges, note the merge-commit SHA — PR 3 Task 7 bumps the rexops pin to it.

---

## PR 3 — RexOps Phase E  (repo: rexops)

Branch: `rexops-cockpit-phase-e` (already exists; the spec commit `9d9e8fc` is on it).

### Task 4: parse the `status` contract → health (pure)

**Files:**
- Create: `crates/rexops-app/src/status_probe.rs`
- Modify: `crates/rexops-app/src/lib.rs` (add `mod status_probe;`)
- Test: inline `#[cfg(test)]` in `status_probe.rs`

**Interfaces:**
- Consumes: `rexops_core::AdapterHealth` (`Healthy/Degraded/Unavailable/Unknown`).
- Produces: `pub struct StatusProbe { pub health: AdapterHealth, pub detail: String, pub latency_ms: Option<u64> }`; `pub fn parse_status(stdout: &str, exit_ok: bool) -> StatusProbe`.

- [ ] **Step 1: Write the failing test**

```rust
// crates/rexops-app/src/status_probe.rs  (bottom)
#[cfg(test)]
mod tests {
    use super::*;
    use rexops_core::AdapterHealth;

    #[test]
    fn healthy_line_parses_to_healthy_with_latency() {
        let p = parse_status(r#"{"healthy":true,"detail":"all clear","latency_ms":7}"#, true);
        assert_eq!(p.health, AdapterHealth::Healthy);
        assert_eq!(p.detail, "all clear");
        assert_eq!(p.latency_ms, Some(7));
    }

    #[test]
    fn unhealthy_line_parses_to_degraded_keeping_detail() {
        let p = parse_status(r#"{"healthy":false,"detail":"1 crit","latency_ms":9}"#, false);
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
        let p = parse_status("{\"healthy\":true,\"detail\":\"ok\",\"latency_ms\":1}\nnoise\n", true);
        assert_eq!(p.health, AdapterHealth::Healthy);
        assert_eq!(p.detail, "ok");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-app status_probe::tests`
Expected: FAIL — `parse_status` undefined.

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/rexops-app/src/status_probe.rs  (top)
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rexops-app status_probe::tests`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/rexops-app/src/status_probe.rs crates/rexops-app/src/lib.rs
git commit -m "feat(rexops): parse the status contract into health (pure)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: spawn the `status` probe in the registry build

**Files:**
- Modify: `crates/rexops-app/src/snapshot.rs` (add a probe block in `build_snapshot_with_piped`, mirroring the Bulwark block at lines ~120-140; add a small spawn helper; surface latency)
- Test: `crates/rexops-app/src/snapshot.rs` `#[cfg(test)]` (mirror `bulwark_probe_uses_the_configured_binary` at ~line 598 and `configured_timeout_bounds_a_hanging_adapter_binary` at ~640)

**Interfaces:**
- Consumes: `crate::status_probe::{parse_status, StatusProbe}`; existing `real_adapter_enabled`, `adapter_timeout`, `OpsSnapshot::set_adapter_health`, `AdapterId::new`.
- Produces: after this task, a component whose `health` is `HealthSource::StatusCommand` has its `adapter_health[id]` populated from the probe, and its latency recorded in a new `snap` field `status_latency: HashMap<String, u64>` (added here) read by the vital in Task 6.

- [ ] **Step 1: Write the failing test**

```rust
// in crates/rexops-app/src/snapshot.rs  #[cfg(test)] mod tests
#[test]
fn status_command_probe_reads_a_tools_json_line() {
    use rexops_core::AdapterHealth;
    // A fake "pulse" that prints the contract and exits 0. `printf` is on PATH in
    // CI; if not available, swap for a tiny shell via `sh -c`.
    let mut config = AppConfig::default();
    config.adapters.insert(
        "pulse".to_owned(),
        crate::config::AdapterConfig {
            enabled: Some(true),
            binary: Some(
                // echo the JSON line: `sh -c 'printf ...'` isn't a single binary,
                // so point at a stub script created by the test.
                stub_binary(r#"{"healthy":true,"detail":"ok","latency_ms":5}"#, 0),
            ),
            ..Default::default()
        },
    );
    let snap = build_snapshot_with_piped(&config, None);
    let h = snap.adapter_health.get("pulse").copied();
    assert_eq!(h, Some(AdapterHealth::Healthy), "notes:\n{:#?}", snap.notes);
}

#[test]
fn status_command_probe_missing_binary_is_unavailable() {
    use rexops_core::AdapterHealth;
    let mut config = AppConfig::default();
    config.adapters.insert(
        "pulse".to_owned(),
        crate::config::AdapterConfig {
            enabled: Some(true),
            binary: Some("/nonexistent/pulse-xyz".to_owned()),
            ..Default::default()
        },
    );
    let snap = build_snapshot_with_piped(&config, None);
    assert_eq!(
        snap.adapter_health.get("pulse").copied(),
        Some(AdapterHealth::Unavailable)
    );
}
```

Add a `stub_binary` test helper near the other test helpers (write an executable shell script to a temp path):

```rust
#[cfg(test)]
fn stub_binary(stdout_line: &str, exit_code: i32) -> String {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("rexops-stub-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("stub-{}", fastrand_like()));
    let mut f = std::fs::File::create(&path).unwrap();
    // Ignores all args (incl. `status`), prints the line, exits with the code.
    write!(f, "#!/bin/sh\necho '{stdout_line}'\nexit {exit_code}\n").unwrap();
    let mut perm = f.metadata().unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&path, perm).unwrap();
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
fn fastrand_like() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
}
```

(If `AdapterConfig`'s fields differ from `{enabled, binary, ..Default}`, grep `struct AdapterConfig` in `crates/rexops-app/src/config.rs` and use the exact fields — the existing `bulwark_probe_uses_the_configured_binary` test already constructs one; copy its construction style verbatim.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-app status_command_probe`
Expected: FAIL — no probe block sets `adapter_health["pulse"]`, so it's `None`.

- [ ] **Step 3: Write minimal implementation**

Add a spawn helper near `bulwark_adapter` (after `system_adapter`):

```rust
/// Spawn a component's `status` subcommand, bounded by its configured timeout,
/// and parse the one-line contract. Returns the parsed probe. Pure spawn glue;
/// the parse + mapping live in `status_probe`. Resolution mirrors launch: the
/// configured `binary` for the id, else the id itself on PATH.
fn status_command_probe(
    config: &AppConfig,
    id: &str,
    args: &[&str],
) -> crate::status_probe::StatusProbe {
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

    let start = Instant::now();
    // Bounded spawn: reuse the wait-with-timeout the adapters already use. If the
    // adapters crate exposes a helper, prefer it; otherwise spawn + kill on
    // timeout here. We spawn, then wait up to `adapter_timeout`.
    let mut child = match Command::new(&program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => {
            return crate::status_probe::StatusProbe {
                health: rexops_core::AdapterHealth::Unavailable,
                detail: "not found".to_owned(),
                latency_ms: None,
            }
        }
    };

    let timeout = adapter_timeout(config, id);
    let deadline = start + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                if let Some(mut so) = child.stdout.take() {
                    use std::io::Read;
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
                }
            }
        }
    }
}
```

Add a `status_latency` field to `OpsSnapshot` (find its struct def — likely in `crates/rexops-app/src/` or `rexops-core`; add `pub status_latency: std::collections::HashMap<String, u64>` and default it `HashMap::new()` in `OpsSnapshot::new`).

Add the probe block in `build_snapshot_with_piped`, after the Bulwark/System/Workstate blocks and BEFORE the `registry_walk(&mut snap, config);` call:

```rust
    // StatusCommand components (e.g. Pulse): spawn `<bin> status`, bounded by the
    // configured timeout, parse the one-line contract into health + latency. Runs
    // before the registry walk so adapter_health/status_latency are populated.
    for comp in rexops_core::COMPONENTS {
        if let rexops_core::HealthSource::StatusCommand { args, .. } = comp.health {
            if real_adapter_enabled(config, comp.id) {
                let probe = status_command_probe(config, comp.id, args);
                if let Ok(id) = AdapterId::new(comp.id) {
                    snap.set_adapter_health(&id, probe.health);
                }
                if let Some(ms) = probe.latency_ms {
                    snap.status_latency.insert(comp.id.to_owned(), ms);
                }
                snap.add_note(format!("{} status: {}", comp.id, probe.detail));
            }
        }
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rexops-app status_command_probe`
Expected: PASS (2 tests).

- [ ] **Step 5: Full gates + commit**

Run: `cargo build --workspace && cargo test -p rexops-app && cargo clippy -p rexops-app -- -D warnings && cargo fmt --all --check`
Expected: all green. (No component uses `StatusCommand` yet — Pulse flips in Task 8 — so this block is exercised only by the tests until then; that's intentional, it keeps the wiring landable independently.)

```bash
git add crates/rexops-app/src/snapshot.rs crates/rexops-app/src/lib.rs
git commit -m "feat(rexops): spawn + time the StatusCommand status probe (bounded)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: heartbeat ring buffer in the TUI app

**Files:**
- Create: `crates/rexops-tui/src/app/heartbeat.rs`
- Modify: `crates/rexops-tui/src/app/state.rs` (hold the buffer on `App`; push on snapshot apply) and `crates/rexops-tui/src/app/mod.rs` (add `mod heartbeat;`)
- Test: inline `#[cfg(test)]` in `heartbeat.rs`

**Interfaces:**
- Consumes: the `OpsSnapshot.status_latency` map from Task 5.
- Produces: `pub struct HeartbeatLog { cap: usize, by_id: HashMap<String, VecDeque<u64>> }`; `pub fn HeartbeatLog::with_capacity(cap: usize) -> Self`; `pub fn HeartbeatLog::record(&mut self, id: &str, latency_ms: u64)`; `pub fn HeartbeatLog::samples(&self, id: &str) -> Vec<u64>`; `pub fn HeartbeatLog::latest(&self, id: &str) -> Option<u64>`.

- [ ] **Step 1: Write the failing test**

```rust
// crates/rexops-tui/src/app/heartbeat.rs  (bottom)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_in_order_and_reports_latest() {
        let mut log = HeartbeatLog::with_capacity(16);
        log.record("pulse", 5);
        log.record("pulse", 8);
        assert_eq!(log.samples("pulse"), vec![5, 8]);
        assert_eq!(log.latest("pulse"), Some(8));
    }

    #[test]
    fn caps_at_capacity_dropping_oldest() {
        let mut log = HeartbeatLog::with_capacity(3);
        for s in [1, 2, 3, 4, 5] {
            log.record("pulse", s);
        }
        assert_eq!(log.samples("pulse"), vec![3, 4, 5]);
    }

    #[test]
    fn unknown_id_is_empty() {
        let log = HeartbeatLog::with_capacity(4);
        assert!(log.samples("nope").is_empty());
        assert_eq!(log.latest("nope"), None);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-tui heartbeat::tests`
Expected: FAIL — `HeartbeatLog` undefined.

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/rexops-tui/src/app/heartbeat.rs  (top)
//! A bounded, transient log of per-component liveness samples — the data behind
//! the cockpit's heartbeat sparkline. In-memory only (cleared on exit), keyed by
//! component id so any StatusCommand tool can have a heartbeat, not just Pulse.

use std::collections::{HashMap, VecDeque};

/// Recent latency samples per component id, capped per id.
#[derive(Debug, Default)]
pub struct HeartbeatLog {
    cap: usize,
    by_id: HashMap<String, VecDeque<u64>>,
}

impl HeartbeatLog {
    /// A log holding up to `cap` samples per component.
    pub fn with_capacity(cap: usize) -> Self {
        HeartbeatLog {
            cap: cap.max(1),
            by_id: HashMap::new(),
        }
    }

    /// Append one sample for `id`, dropping the oldest past capacity.
    pub fn record(&mut self, id: &str, latency_ms: u64) {
        let q = self.by_id.entry(id.to_owned()).or_default();
        q.push_back(latency_ms);
        while q.len() > self.cap {
            q.pop_front();
        }
    }

    /// Samples for `id`, oldest→newest (empty if none).
    pub fn samples(&self, id: &str) -> Vec<u64> {
        self.by_id
            .get(id)
            .map(|q| q.iter().copied().collect())
            .unwrap_or_default()
    }

    /// The most recent sample for `id`, if any.
    pub fn latest(&self, id: &str) -> Option<u64> {
        self.by_id.get(id).and_then(|q| q.back().copied())
    }
}

// Learning Notes
// - Per-id `VecDeque` with a hard cap: O(1) push, bounded memory, no persistence.
// - Default capacity (16) is chosen in `App`; this type stays policy-free.
```

Then on `App` (in `state.rs`): add a field `heartbeats: HeartbeatLog`, init it `HeartbeatLog::with_capacity(16)` in the constructor, and where a new snapshot is applied (the `apply_snapshot`/equivalent that sets `self.snapshot`), record each entry:

```rust
        for (id, ms) in &snapshot.status_latency {
            self.heartbeats.record(id, *ms);
        }
```

(Grep `fn apply` / `self.snapshot =` in `state.rs` to find the single apply site — the Phase C notes say applying a snapshot already auto-focuses the first card, so there is one clear seam.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rexops-tui heartbeat::tests`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/rexops-tui/src/app/heartbeat.rs crates/rexops-tui/src/app/state.rs crates/rexops-tui/src/app/mod.rs
git commit -m "feat(rexops): transient per-component heartbeat ring buffer

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: bump the suite-ui pin to the PR-2 merge

**Files:**
- Modify: `crates/rexops-tui/Cargo.toml:31` (the `suite-ui` `rev`)

**Interfaces:** none (dependency bump). After this, `suite_ui::Heartbeat` is importable in rexops.

- [ ] **Step 1: Update the pin**

Replace the rev with PR 2's merge-commit SHA (call it `<SUITE_UI_SHA>`):

```toml
suite-ui = { git = "https://github.com/tom2025b/linux-ops-suite", rev = "<SUITE_UI_SHA>" }
```

- [ ] **Step 2: Update the lockfile + verify the widget resolves**

Run: `cargo update -p suite-ui --precise <SUITE_UI_SHA> && cargo build --workspace`
Expected: builds; `suite_ui::Heartbeat` now available.

- [ ] **Step 3: Smoke-import in a throwaway test, then gates**

Run: `cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --all --check`
Expected: all green.

- [ ] **Step 4: Commit**

```bash
git add crates/rexops-tui/Cargo.toml Cargo.lock
git commit -m "build(rexops): bump suite-ui pin for the Heartbeat widget

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 8: Pulse becomes a Live card + heartbeat vital + drill-down history + guard

**Files:**
- Modify: `crates/rexops-core/src/component_table.rs:120-128` (flip the Pulse row)
- Modify: the cockpit card vital path — `crates/rexops-tui/src/ui/cockpit_widgets/status_card.rs` (use `suite_ui::Heartbeat` for a `StatusCommand` component's vital, fed by `App`'s `HeartbeatLog`; the wiring that passes samples into the card lives where the card grid is built — `crates/rexops-tui/src/screens/cockpit.rs`)
- Modify: `crates/rexops-tui/src/screens/cockpit_detail.rs` (show recent heartbeat history for a `StatusCommand` component)
- Test: `crates/rexops-tui/src/app/tests/cockpit.rs` (guard: Pulse launchable + Live; others Planned) and a card-render test + a back-compat empty-heartbeat test

**Interfaces:**
- Consumes: `suite_ui::Heartbeat`; `App::heartbeats` (`HeartbeatLog`); `rexops_core::launchable_components()`.
- Produces: Pulse in the launchable + `Live` set; banner rollup `6/11`.

- [ ] **Step 1: Write the failing guard test**

```rust
// crates/rexops-tui/src/app/tests/cockpit.rs
#[test]
fn pulse_is_a_live_launchable_component_and_the_other_planned_four_are_not() {
    let launchable: Vec<&str> = rexops_core::launchable_components()
        .iter()
        .map(|c| c.id)
        .collect();
    assert!(launchable.contains(&"pulse"), "pulse must be launchable: {launchable:?}");

    // The remaining Planned tools stay non-launchable.
    for id in ["tripwire", "rewind", "rex-check", "rex-forge"] {
        assert!(!launchable.contains(&id), "{id} must stay Planned/non-launchable");
    }

    // Pulse's health source is StatusCommand and its maturity is Live.
    let pulse = rexops_core::component_by_id("pulse").unwrap();
    assert!(matches!(pulse.health, rexops_core::HealthSource::StatusCommand { .. }));
    assert_eq!(pulse.maturity, rexops_core::Maturity::Live);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-tui pulse_is_a_live_launchable`
Expected: FAIL — Pulse is currently `Planned`, no launch, so it's absent from `launchable_components()`.

- [ ] **Step 3: Flip the Pulse registry row**

In `component_table.rs`, the Pulse entry (currently lines ~120-128):

```rust
    Component {
        id: "pulse",
        name: "Pulse",
        group: ComponentGroup::Monitor, // unchanged — keep the existing group value
        blurb: "Heartbeat / liveness monitor",
        health: HealthSource::StatusCommand {
            binary: "pulse",
            args: &["status"],
        },
        launch: Some(LaunchSpec {
            run_mode: RunMode::Foreground,
            args: &[], // bare `pulse` — launches the interactive TUI
        }),
        feed: None,
        maturity: Maturity::Live,
    },
```

(Match the EXACT field set + group value the other rows use — copy the surrounding row's shape; only `health`, `launch`, `maturity` change. If `LaunchSpec` has more fields, mirror Bulwark's row.)

- [ ] **Step 4: Run the guard test (passes) + the unification guard from Phase D**

Run: `cargo test -p rexops-tui pulse_is_a_live_launchable && cargo test -p rexops-tui launcher_list_is_exactly_the_registry`
Expected: PASS — Pulse joins the launchable set; the Phase D unification guard still holds (Launcher list == registry launch set, now including pulse).

- [ ] **Step 5: Card vital — render the heartbeat (failing test first)**

Add a card-render test mirroring the Phase B/D card tests (grep `status_card` test names for the harness). The card for a `StatusCommand` component with samples shows the heartbeat text; with none, it shows a plain vital:

```rust
// near the other StatusCard render tests
#[test]
fn statuscommand_card_with_samples_shows_heartbeat_vital() {
    use suite_ui::Heartbeat;
    let hb = Heartbeat { samples: &[2, 5, 9], latest_ms: Some(9) };
    // The card vital for Pulse, given samples, is the heartbeat text.
    assert_eq!(hb.text(), "♥ ▁▄█ 9ms");
    // (Render the actual StatusCard with this vital via the existing card test
    //  harness and assert the heartbeat line is present — copy the nearest
    //  existing card render assertion for the exact API.)
}

#[test]
fn statuscommand_card_with_no_samples_shows_plain_live_vital() {
    use suite_ui::Heartbeat;
    let hb = Heartbeat { samples: &[], latest_ms: None };
    assert_eq!(hb.text(), "♥"); // graceful: just the heart, no broken sparkline
}
```

- [ ] **Step 6: Wire the heartbeat into the card vital**

Where the card grid builds each `StatusCard`'s vital (in `screens/cockpit.rs`, building cards from `OpsSnapshot.components` + `App` state), for a component whose registry `health` is `StatusCommand`, set the vital from `Heartbeat { samples: &app.heartbeats.samples(id), latest_ms: app.heartbeats.latest(id) }.text()` instead of the generic `vital` string; fall back to the component's `vital` when the heartbeat text is just `"♥"` (no data yet) — or always show the heart (decide per the card test). Keep all other components' vital path unchanged.

```rust
// sketch — adapt to the actual card-construction call site:
let vital = if matches!(comp_health_source(id), Some(HealthSource::StatusCommand { .. })) {
    let samples = app.heartbeats.samples(id);
    let hb = suite_ui::Heartbeat { samples: &samples, latest_ms: app.heartbeats.latest(id) };
    let t = hb.text();
    if t == "♥" { status.vital.clone() } else { t } // graceful empty fallback
} else {
    status.vital.clone()
};
```

- [ ] **Step 7: Drill-down history**

In `cockpit_detail.rs`, for a `StatusCommand` component, add a "Heartbeat" section listing recent samples (e.g. the sparkline over the full buffer + the last few values). Mirror the existing detail-section rendering; add one render test asserting the section appears for Pulse and is absent for a non-StatusCommand component.

- [ ] **Step 8: Run all gates**

Run: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --all --check`
Expected: all green. Confirm the banner rollup is `6/11` (grep the rollup test, e.g. `live` count assertion — update it from `5/11` to `6/11`; note this as a deliberate, documented change in the commit, like Phase D's rollup bump).

- [ ] **Step 9: Headless smoke**

Run: `cargo run -p rexops-cli -- components` (or pipe the Workstate fixture as the Phase C/D smoke did)
Expected: Pulse appears as `live` with a heartbeat-style vital (or a plain vital + `live` if no live `pulse status` on PATH — both are acceptable; the point is Pulse is no longer `planned`).

- [ ] **Step 10: Commit**

```bash
git add crates/rexops-core/src/component_table.rs crates/rexops-tui/src/ui/cockpit_widgets/status_card.rs crates/rexops-tui/src/screens/cockpit.rs crates/rexops-tui/src/screens/cockpit_detail.rs crates/rexops-tui/src/app/tests/cockpit.rs
git commit -m "feat(rexops): Pulse becomes a Live launchable card with a heartbeat vital (Phase E)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 9: docs + plan/spec cross-links + LAST_WORK

**Files:**
- Modify: `docs/TUI_DESIGN.md` (note Pulse is now Live with a heartbeat vital; the StatusCommand health path)
- Modify: `docs/superpowers/specs/2026-06-20-rexops-cockpit-redesign-design.md` (tick Phase E in the roadmap, §9)
- Modify (separate repo, separate commit): `~/projects/linux-ops-suite/LAST_WORK.md`

- [ ] **Step 1: Update TUI_DESIGN.md + the roadmap tick** (prose; mirror how Phase C/D were recorded).

- [ ] **Step 2: Run docs-affecting gates** (fmt only): `cargo fmt --all --check`. Commit:

```bash
git add docs/TUI_DESIGN.md docs/superpowers/specs/2026-06-20-rexops-cockpit-redesign-design.md
git commit -m "docs(rexops): record Phase E — Pulse Live + StatusCommand health path

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

- [ ] **Step 3: Update LAST_WORK.md** in the linux-ops-suite repo (per the project's LAST_WORK rule — before declaring Phase E complete). Commit it there separately.

> **PR 3 boundary:** run the four workspace gates a final time, then STOP for approval to push + open the rexops Phase E PR (style: match the Phase C/D PR bodies). Do NOT push or open any PR without explicit approval.

---

## Self-Review

**1. Spec coverage:**
- §4 `pulse status` contract → Tasks 1–2. ✓
- §5 StatusCommand adapter (spawn + parse + mapping + timeout, Planned untouched) → Tasks 4–5. ✓
- §6.1 Heartbeat widget → Task 3. ✓  §6.2 ring buffer → Task 6. ✓
- §7 Pulse Live card + launch + drill-down + 6/11 rollup → Task 8. ✓
- §8 file map → covered across Tasks 1–9; suite-ui pin bump → Task 7. ✓
- §9 sequencing (3 PRs in order, gates green) → PR boundaries after Tasks 2, 3, 9. ✓
- §10 non-goals → encoded as Global Constraints (no persistence/wrappers/new screens/auto-poll). ✓
- §11 success criteria → each maps to a test (contract CLI test T2; parse units T4; spawn/missing T5; widget units+snapshot T3; ring-buffer units T6; guard+rollup+render T8; headless smoke T8.9). ✓

**2. Placeholder scan:** No "TBD/TODO/handle edge cases". The few "grep the exact field/harness and copy the nearest existing test" notes are deliberate *adaptation anchors* (the surrounding code is the source of truth for exact signatures), each pointing at a named existing symbol/file — not blanks. Test code is shown in full.

**3. Type consistency:** `StatusReport{healthy,detail,latency_ms}` (T1) is the wire shape parsed by `parse_status`→`StatusProbe{health,detail,latency_ms}` (T4); `OpsSnapshot.status_latency` (T5) feeds `HeartbeatLog.record` (T6) which feeds `suite_ui::Heartbeat{samples,latest_ms}` (T3) in the card (T8). `AdapterHealth` variants (Healthy/Degraded/Unavailable/Unknown) and `HealthSource::StatusCommand{binary,args}` match the verified enums. `launchable_components()`, `component_by_id`, `Maturity::Live` match Phase A–D APIs.
