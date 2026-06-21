# RexOps Cockpit — Phase E: Pulse / Heartbeat Monitor — Design

**Status:** Approved (brainstorm 2026-06-21). Next: implementation plan.

## 1. Goal

Light up **Pulse** — the suite's namesake heartbeat/liveness monitor — as the
first of the five remaining `Planned` components. After Phase E, Pulse is a
**`Live`, launchable** cockpit card whose vital is a **heartbeat sparkline**
(`♥ ▁▂▅▇▅▂ 7ms`) built from its live `status` contract, and pressing its card
letter (or `Enter` on the focused card) launches the Pulse TUI through the
**existing** confirm gate — exactly like Bulwark/Proto/ScriptVault/ToolFoundry
today.

This is the master roadmap's **Phase E — Monitors & mechanics**, scoped to its
first slice: add the `StatusCommand` health path and wire Pulse plus the shared
`Heartbeat` widget. The other four `Planned` tools (Tripwire, Rewind, rex-check,
rex-forge) stay `Planned` — each is a later one-row change once it grows a stable
`status` contract.

After Phase E the banner rollup reads **6/11 live**.

## 2. The real problem — RexOps has no live `status` health path yet

The registry models three working health sources today — `Probe` (Bulwark),
`Feed` (Workstate/ScriptVault/ToolFoundry), and `Host` (System). The
`HealthSource::StatusCommand { binary, args }` variant **already exists in the
enum** (it was designed in Phase A for exactly this), but **nothing handles it**:
the `snapshot.rs` registry walk has no arm for it, and **no suite tool emits the
contract it would read**.

Pulse is the right tool to drive this path into existence: it is the suite's
liveness instrument and already computes its own health verdict. But two gaps
block wiring it:

1. **Pulse has no machine-readable health output.** It is an interactive,
   read-only TUI (`pulse`), with a one-shot *render* path (`pulse --dump-view V`)
   but no JSON/contract a parent process can consume. Its `Verdict { state:
   Healthy | NeedsAttention | … }` is computed internally and only ever rendered.
2. **RexOps can't consume a `status` contract** because the `StatusCommand` arm
   is unimplemented.

Phase E closes both — minimally, reusing what each side already has.

## 3. Approach (option A — RexOps accumulates the heartbeat; Pulse stays thin)

Chosen during the brainstorm over the alternative (Pulse owns and emits its own
sample history). The split:

- **Pulse emits one liveness sample per call.** `pulse status` reports current
  liveness + a single `latency_ms` (the wall-time it took Pulse to read its
  snapshot and compute its verdict). Pulse keeps **no history** and gains no new
  state — smallest possible change, and it reuses Pulse's existing
  snapshot→`Verdict` pipeline verbatim.
- **RexOps accumulates the sparkline.** RexOps already refreshes on `r` and on
  launch; each refresh that yields a Pulse `latency_ms` pushes one sample into a
  small **in-memory ring buffer**. The sparkline is the buffer; it fills over
  successive refreshes and is purely transient (no persistence — matching the
  master doc's "only the in-memory heartbeat ring buffer is transient").

This keeps the contract tiny and stable, puts the only new *stateful* piece
(the ring buffer) in the app layer where refresh already lives, and makes the
`Heartbeat` widget a pure function of a sample slice — trivially testable.

## 4. The contract — `pulse status` (repo: linux-ops-suite)

A new **additive, read-only, non-interactive** subcommand, a sibling of the
existing `--dump-view` headless path (so it inherits the TTY-agnostic, run-once
posture). No clap is introduced — it slots into Pulse's existing hand-rolled arg
matcher.

```
$ pulse status
{"healthy":true,"detail":"snapshot fresh; 3/3 sources current","latency_ms":7}
$ echo $?
0
```

**Output:** exactly one line of JSON on stdout, then exit. The schema (a small
`#[derive(Serialize)] StatusReport`):

| field        | type   | meaning                                                        |
|--------------|--------|---------------------------------------------------------------|
| `healthy`    | bool   | `Verdict.state == Healthy` — reuses Pulse's own verdict       |
| `detail`     | string | short human reason — the verdict's top cause / summary line   |
| `latency_ms` | u64    | wall-time Pulse spent reading its snapshot + computing verdict |

**Exit code:** `0` when `healthy` is true, `1` otherwise. Stdout always carries a
valid JSON line even on an unhealthy/incomplete snapshot (the contract never
breaks — a missing data dir yields `{"healthy":false,"detail":"no snapshot…",…}`,
exit 1, not a panic or empty output).

**Implementation:** beside `--dump-view`, reuse the same snapshot read +
`Verdict` computation; wrap it in a `std::time::Instant` for `latency_ms`;
serialize `StatusReport` instead of rendering a view. No new health logic.

**`--json` flag:** `pulse status` is already machine-only, so JSON is the
default and only format; no `--json` flag is added (YAGNI). A future
human-readable `status` line, if ever wanted, is a separate change.

## 5. The adapter — handle `StatusCommand` in RexOps (repo: rexops)

Implement the missing arm in the `snapshot.rs` registry walk (the same walk that
already dispatches `Probe`/`Feed`/`Host`/`Planned`):

```
HealthSource::StatusCommand { binary, args } =>
    spawn `<resolved-binary> <args…>` with the existing adapter timeout,
    capture stdout, parse the one JSON line → ComponentStatus
```

- **Binary resolution is unchanged** — the same `which <id>` → configured-binary
  resolution that launch uses (no new resolution logic, no new config).
- **Timeout** reuses the existing `adapter_timeout` (the same knob `Probe` uses);
  no new config key.
- **Mapping** (into the existing `AdapterHealth`: `Healthy / Degraded /
  Unavailable / Unknown`):

| outcome                                              | health        | vital shown                    |
|-----------------------------------------------------|---------------|--------------------------------|
| valid JSON, `healthy: true`                          | `Healthy`     | heartbeat sparkline + `Nms`    |
| valid JSON, `healthy: false`                         | `Degraded`    | the `detail` string            |
| binary missing / spawn fails                         | `Unavailable` | `"not found"` (short reason)   |
| non-zero exit with no parseable JSON / garbled stdout | `Unavailable` | `"bad status output"`          |
| timeout                                              | `Unavailable` | `"status timed out"`           |

- The `latency_ms` from a successful parse is surfaced so the app layer can push
  it into the ring buffer (§6).
- **`Planned` arm is untouched** — it still resolves with zero I/O to a neutral
  dimmed card. No spawn, no file read for the other four tools.

Parsing is a pure function over `(stdout, exit_status)` and is unit-tested
off-screen: good line, `healthy:false`, non-zero exit, garbage stdout, empty
stdout, and (via an injected fake) timeout + missing-binary.

## 6. The Heartbeat widget + ring buffer

### 6.1 `Heartbeat` widget (repo: umbrella → suite-ui)

A new shared suite-ui widget — a **pure render function** of a recent-sample
slice plus the latest value, producing `♥ ▁▂▅▇▅▂ 7ms`:

- Input: `&[u64]` latency samples (oldest→newest) + the latest `latency_ms`.
- Output: a heart glyph, a Unicode block sparkline of the samples, and the
  latest latency. Empty slice → just `♥ 7ms` (no sparkline); no samples *and* no
  latest → renders nothing heartbeat-specific (caller falls back to a plain
  vital).
- Lives in suite-ui so any tool can show a heartbeat (master spec §5 names this
  a shared widget). Snapshot-tested in suite-ui.

**Topology note (sequencing constraint):** suite-ui changes land via a PR to the
umbrella `main` **first**, then the rexops pin to suite-ui is bumped in a
separate PR — per the suite's established suite-ui CI ordering rule. Phase E's
three-PR sequence (§9) honours this.

### 6.2 Ring buffer (repo: rexops, app layer)

A small transient buffer of recent Pulse latency samples in the TUI `App`:

- Capacity ~**16** samples (enough for a legible sparkline; bounded so it can't
  grow). Keyed by component id, so the design generalises to future
  `StatusCommand` tools without rework.
- Each refresh (manual `r` or on-launch) that yields a Pulse `latency_ms` pushes
  one sample; the oldest is dropped past capacity.
- **No persistence** — cleared on exit, rebuilt across a run. Matches the master
  doc's stateless posture.
- **Empty buffer is graceful:** before any sample exists, the Pulse card shows a
  plain `Live` vital (e.g. its `detail`), not a broken sparkline. First paint is
  always clean.

## 7. Pulse becomes a `Live` card (repo: rexops)

Flip the single Pulse row in `COMPONENTS`:

| field      | before              | after                                             |
|------------|---------------------|---------------------------------------------------|
| `health`   | `Planned`           | `StatusCommand { binary: "pulse", args: ["status"] }` |
| `launch`   | `None`              | `Some(LaunchSpec { run_mode: Foreground, … })` (bare `pulse`) |
| `maturity` | `Planned`           | `Live`                                             |

Effects, all through **existing** machinery:

- The Pulse card renders its **heartbeat vital** (via the §6.1 widget fed by the
  §6.2 buffer); a dimmed `Planned` card becomes a live instrument.
- Its **card letter / `Enter`** arms a launch of the Pulse TUI through the
  *existing* `arm_tool → pending_action → confirm_pending` gate — no new launch
  path (Phase D made the registry the single launch source; Pulse just gains a
  `LaunchSpec` row).
- **Drill-down reuses the Phase C `CockpitDetail` screen**, which additionally
  shows the recent heartbeat history for a `StatusCommand` component. No new
  screen.
- The **banner rollup** goes `5/11 → 6/11 live`.
- The other four `Planned` tools are **unchanged**.

## 8. What changes (file map)

**linux-ops-suite (Pulse) — PR 1:**
- `crates/pulse/src/main.rs` — add the `status` arm to the arg matcher.
- `crates/pulse/src/…` — a small `StatusReport` (serde) + a function reusing the
  existing snapshot→`Verdict` path, timed for `latency_ms`. (Exact module chosen
  to sit beside the existing headless/verdict code.)
- Tests: golden/unit on the JSON contract + exit code (healthy / attention /
  missing-data).

**umbrella → suite-ui — PR 2:**
- suite-ui: `Heartbeat` widget (pure render) + snapshot test.

**rexops (Phase E) — PR 3 (consumes PR 1 & 2):**
- `crates/rexops-adapters/src/…snapshot.rs` (or the registry-walk module) — the
  `StatusCommand` arm + JSON parse → `ComponentStatus`; pure-parse unit tests.
- `crates/rexops-tui/src/app/…` — the heartbeat ring buffer + push-on-refresh.
- `crates/rexops-tui/src/ui/cockpit_widgets/…` — Pulse card uses the suite-ui
  `Heartbeat` widget for its vital; graceful empty-buffer fallback.
- `crates/rexops-tui/src/screens/cockpit_detail.rs` — heartbeat history in the
  Pulse drill-down.
- `crates/rexops-core/src/component_table.rs` — flip the Pulse row.
- `Cargo.toml` — bump the suite-ui pin to the PR-2 revision.
- Tests: registry guard (Pulse in the launchable + `Live` set; the four others
  still `Planned`), back-compat render (a card with an empty heartbeat ==
  a plain `Live` card), Heartbeat-fed card render.

## 9. Behaviour parity, risk & sequencing

- **No existing launch behaviour changes.** Bulwark/Proto/ScriptVault/ToolFoundry
  launch exactly as before; Pulse is purely additive to the launchable set
  (`[bulwark, proto, scriptvault, toolfoundry] → + pulse`). A guard test asserts
  the set.
- **No other card changes.** Only Pulse flips; the four other `Planned` cards
  render identically (zero-I/O `Planned` path untouched).
- **Risk** is concentrated in (a) the new subprocess `status` path (spawn +
  parse), mitigated by treating *every* non-happy outcome as `Unavailable` with a
  short reason (never a panic, never a hang past the timeout) and pure-parse unit
  tests; and (b) the cross-repo suite-ui pin bump, mitigated by following the
  established suite-ui CI ordering.
- **Sequencing — three PRs, in order:**
  1. **linux-ops-suite:** `pulse status` contract → merge first.
  2. **umbrella → suite-ui:** `Heartbeat` widget → merge, then bump the rexops pin.
  3. **rexops:** `StatusCommand` adapter + ring buffer + Pulse `Live` card
     (Phase E) → consumes 1 & 2.
- The four cargo gates (**build / test / clippy `-D warnings` / fmt**) stay green
  at **every** commit of every PR.

## 10. Non-goals (YAGNI)

- **No auto-refresh polling.** Heartbeat fills on manual `r` + on-launch refresh,
  exactly as health is gathered today. Staggered auto-poll remains a future
  enhancement.
- **No persistence.** The ring buffer is in-memory and transient; RexOps stays
  stateless across runs.
- **No new screens.** The Phase C drill-down is reused for Pulse's history.
- **No binary installation, no wrappers, no aliases.** Resolution stays
  `which`-then-configured-binary; the card launches bare `pulse`.
- **Pulse keeps no sample history** (option A) — `pulse status` is a single
  sample per call; RexOps owns the series.
- **The other four `Planned` tools stay `Planned`** — Tripwire/Rewind/rex-check/
  rex-forge are a later slice, each one registry row + a `status` contract when
  ready.
- **No `--json` flag on `pulse status`** — JSON is its only output.

## 11. Success criteria

1. `pulse status` prints one valid JSON line `{healthy, detail, latency_ms}` and
   exits `0`/`1` by health, for healthy, attention, and missing-data snapshots —
   reusing Pulse's existing verdict, with no new health logic and no TTY needed.
2. RexOps handles `HealthSource::StatusCommand`: a successful parse yields a
   `Healthy`/`Degraded` card; every failure mode (missing binary, non-zero,
   garbled, timeout) yields `Unavailable` with a short reason and never panics or
   hangs past the timeout. The `Planned` path is unchanged (zero I/O).
3. A shared suite-ui `Heartbeat` widget renders `♥ <sparkline> <N>ms` from a
   sample slice (graceful when empty), snapshot-tested.
4. RexOps keeps a bounded, transient ring buffer of Pulse latencies, pushed on
   each refresh; the Pulse card shows the heartbeat vital, with a plain `Live`
   vital before any sample exists.
5. Pulse renders as a `Live`, launchable cockpit card; its letter/`Enter` arms a
   Pulse-TUI launch through the existing confirm gate; its drill-down shows recent
   heartbeat history. The banner rollup reads `6/11 live`. A guard test locks
   Pulse into the launchable + `Live` set and keeps the other four `Planned`.
6. All four cargo gates green at every commit across the three PRs; the
   `StatusCommand` parse + the registry view are unit-tested off-screen; a
   headless smoke (`rexops components` on a fixture) shows Pulse `Live` with a
   heartbeat vital.
