# RexOps Cockpit — Phase B: Cockpit Screen + Widgets — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the RexOps `Dashboard` screen with a true **cockpit landing screen** — a grouped grid of component status cards (health light + role + one vital), reading the `OpsSnapshot.components` the Phase A registry walk already produces — built from new, domain-free cockpit widgets.

**Architecture:** Phase A put the *data* in place (`OpsSnapshot.components: Vec<ComponentStatus>`). Phase B is **pure presentation**: a small set of reusable, domain-free widgets (`StatusLight`, `StatusCard`, `CardGrid`, `IdentityBanner`) plus a `cockpit` screen that arranges them. The widgets live **locally in `rexops-tui` for this phase** (`crates/rexops-tui/src/ui/cockpit_widgets/`), deliberately written domain-free (they take a `Theme` + borrowed data + a `Rect` and draw into a `Frame`; they own no app state and import no app types) so a later phase can lift them verbatim into the shared `thomas-tui` toolkit and re-export through `suite-ui`. No cross-repo work this phase. The cockpit screen reads only already-resolved snapshot data, so it is a pure render function, unit-testable off-screen exactly like the current `render_dashboard`.

**Tech Stack:** Rust 2021, `ratatui`, `suite-ui` (existing `Theme`/`pane` from the pinned git dep), `rexops-core` (`ComponentStatus`, `AdapterHealth`, `Freshness`), `cargo test`/`clippy`/`fmt`. Off-screen render tests use `ratatui::backend::TestBackend` (the existing dashboard/launchpad test pattern).

## Global Constraints

- Files stay **under 300 LOC** (ideally < 200); each `.rs` ends with a `// Learning Notes` footer (existing project convention).
- All four cargo gates (`build`, `test`, `clippy --workspace -- -D warnings`, `fmt --check`) green at **every** task's commit.
- **Domain-free widgets:** every widget in `cockpit_widgets/` takes only `(theme: Theme, <borrowed data>, area: Rect, f: &mut Frame)` (or returns ratatui `Line`/`Span` by shape). It must NOT import `crate::app`, `crate::App`, `OpsSnapshot`, or any rexops domain type. It may import `rexops_core::{AdapterHealth, Freshness}` ONLY for the single mapping helper in Task 1 — prefer accepting a tiny widget-local input type so the widget is liftable to `thomas-tui` (which cannot depend on `rexops-core`). The mapping from `ComponentStatus` → widget input happens in the **screen**, not the widget.
- **Reuse, don't fork, suite-ui:** use the existing `suite_ui::{pane, Theme}` and the theme styling already used by `screens/dashboard.rs` / `ui/widgets.rs`. Do not duplicate theme/color logic; derive card colors from `Theme`.
- **Behavior preserved elsewhere:** screens 2–7, the palette, help overlay, confirm gate, jobs, and all keymaps are **unchanged** this phase. Only the Dashboard screen's *rendering* changes. Launch hotkeys on cards are explicitly **out of scope** (Phase C) — this phase is the read-only status board.
- **Keyboard model intact:** the cockpit keeps the existing global keys (`1`–`7`, `q`, `r`, `?`, `/`, `j`/`k`, palette, cancel). It adds no new key bindings this phase.
- Conventional commits: `feat(rexops): … (Phase B)` / `test(rexops): …` / `refactor(rexops): …`.
- Run all `cargo` commands from the worktree root.

---

## File Structure

- **Create** `crates/rexops-tui/src/ui/cockpit_widgets/mod.rs` — module index + the shared widget-input types (`LightState`/`CardInput`) and the `light_state_from_health` mapping helper. One responsibility: *the domain-free vocabulary the cockpit widgets speak.*
- **Create** `crates/rexops-tui/src/ui/cockpit_widgets/status_light.rs` — `StatusLight`: the one-glyph health lamp (`● ◍ ○ ✗`) + canonical color.
- **Create** `crates/rexops-tui/src/ui/cockpit_widgets/status_card.rs` — `StatusCard`: a single component card (light + name + role + vital), drawn in a `pane`.
- **Create** `crates/rexops-tui/src/ui/cockpit_widgets/card_grid.rs` — `CardGrid`: lay a slice of cards into responsive columns, grouped by section label; stacks to one column on narrow widths.
- **Create** `crates/rexops-tui/src/ui/cockpit_widgets/identity_banner.rs` — `IdentityBanner`: the top bar (host · kernel · uptime · clock · "N/M live · K alerts" rollup).
- **Create** `crates/rexops-tui/src/screens/cockpit.rs` — `render_cockpit(f, app, area, theme)`: maps `app.snapshot.components` + `app.snapshot.system` + risk into widget inputs and arranges banner + grid + a risk/hint strip. The Dashboard screen's replacement.
- **Modify** `crates/rexops-tui/src/screens/mod.rs` — add `pub mod cockpit; pub use cockpit::render_cockpit;`.
- **Modify** `crates/rexops-tui/src/ui/mod.rs` — add `pub mod cockpit_widgets;`.
- **Modify** `crates/rexops-tui/src/ui/layout.rs` — route `Screen::Dashboard` to `render_cockpit` instead of `render_dashboard`.
- **Keep (do not delete this phase)** `crates/rexops-tui/src/screens/dashboard.rs` — left in place but no longer routed, to keep the diff reviewable and the fallback obvious; its removal/retention is decided in Task 7.

> Why local widgets, not thomas-tui this phase: the user chose "build locally first, upstream later." The widgets are written domain-free precisely so the later lift is a file move + re-export, not a rewrite.

---

### Task 1: Widget vocabulary — `LightState`, `CardInput`, and the health mapping

**Files:**
- Create: `crates/rexops-tui/src/ui/cockpit_widgets/mod.rs`
- Modify: `crates/rexops-tui/src/ui/mod.rs` (add `pub mod cockpit_widgets;`)
- Test: inline `#[cfg(test)]` in `mod.rs`

**Interfaces:**
- Consumes: `rexops_core::AdapterHealth` (for the mapping helper only).
- Produces:
  - `pub enum LightState { Healthy, Degraded, Neutral, Down }` — the domain-free health-lamp state (NOT `AdapterHealth`, so the widget is liftable to thomas-tui).
  - `pub fn light_state_from_health(h: AdapterHealth) -> LightState` — maps `Healthy→Healthy`, `Degraded→Degraded`, `Unavailable→Down`, `Unknown→Neutral`.
  - `pub struct CardInput<'a> { pub name: &'a str, pub role: &'a str, pub light: LightState, pub vital: Option<&'a str>, pub dim: bool }` — the borrowed input a `StatusCard` renders. `dim: true` renders a planned/inactive card muted.

> IMPORTANT module-wiring rule for this whole phase: the child `pub mod` lines in `cockpit_widgets/mod.rs` are added in the task that CREATES each child file, never up front — so every task's `cargo` gate stays green. Task 1 adds NO child `pub mod` lines (it creates no child files); Task 2 adds `pub mod status_light;`, Task 3 `pub mod status_card;`, Task 4 `pub mod card_grid;`, Task 5 `pub mod identity_banner;`.

- [ ] **Step 1: Write the failing test**

Create `crates/rexops-tui/src/ui/cockpit_widgets/mod.rs` with the vocabulary + this test (no child `pub mod` lines yet):

```rust
//! cockpit_widgets — the domain-free widgets the cockpit screen is built from.
//!
//! These take a `Theme`, borrowed data, and a `Rect`, and draw into a `Frame`.
//! They own no application state and import no rexops domain types beyond the
//! tiny mapping helper below — so a later phase can lift this whole module into
//! the shared `thomas-tui` toolkit (and re-export via `suite-ui`) almost
//! verbatim. The screen maps `ComponentStatus` into these inputs; the widgets
//! never see a `ComponentStatus`.

use rexops_core::AdapterHealth;

/// The domain-free state of a health lamp. Deliberately NOT `AdapterHealth`, so
/// the widgets stay liftable into a toolkit that cannot depend on rexops-core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightState {
    Healthy,
    Degraded,
    Neutral,
    Down,
}

/// Map a probed `AdapterHealth` to a lamp state. `Unknown → Neutral` on purpose:
/// an unprobed or planned component is not a fault, so it must read dim, never
/// red. Only `Unavailable` is `Down`.
pub fn light_state_from_health(h: AdapterHealth) -> LightState {
    match h {
        AdapterHealth::Healthy => LightState::Healthy,
        AdapterHealth::Degraded => LightState::Degraded,
        AdapterHealth::Unavailable => LightState::Down,
        AdapterHealth::Unknown => LightState::Neutral,
    }
}

/// The borrowed input a `StatusCard` renders. The screen builds one of these per
/// component from a `ComponentStatus`; the widget never sees the domain type.
#[derive(Debug, Clone, Copy)]
pub struct CardInput<'a> {
    pub name: &'a str,
    pub role: &'a str,
    pub light: LightState,
    pub vital: Option<&'a str>,
    /// Render muted (a planned / inactive component).
    pub dim: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_maps_to_the_right_light_state() {
        assert_eq!(light_state_from_health(AdapterHealth::Healthy), LightState::Healthy);
        assert_eq!(light_state_from_health(AdapterHealth::Degraded), LightState::Degraded);
        assert_eq!(light_state_from_health(AdapterHealth::Unavailable), LightState::Down);
        assert_eq!(light_state_from_health(AdapterHealth::Unknown), LightState::Neutral);
    }
}

// Learning Notes
// - LightState exists so the widgets never name `AdapterHealth`. That single
//   indirection is what keeps this module liftable into thomas-tui later (a
//   toolkit crate can't depend on rexops-core). The mapping lives here, at the
//   seam, not scattered through the widgets.
// - CardInput borrows (`&'a str`) rather than owning, matching suite-ui's widget
//   contract ("borrowed values only") — no per-frame allocation.
```

- [ ] **Step 2: Run test to verify it fails (then passes once the file is saved)**

Run: `cargo test -p rexops-tui cockpit_widgets:: 2>&1 | tail -20`
Expected: the test COMPILES and PASSES once `mod.rs` exists and is wired (Step 3). Before wiring `pub mod cockpit_widgets;` in `ui/mod.rs`, the module isn't compiled at all — so do Step 3 first if the test reports "module not found," then re-run. (This task's "red" is the missing module; the green is after wiring.)

- [ ] **Step 3: Wire the module into `ui/mod.rs`**

Add to `crates/rexops-tui/src/ui/mod.rs`, with the other `pub mod` lines (e.g. after `pub mod widgets;`):

```rust
pub mod cockpit_widgets;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rexops-tui cockpit_widgets:: 2>&1 | tail -20`
Expected: `health_maps_to_the_right_light_state` PASSES.

- [ ] **Step 5: Gates green, then commit**

Run: `cargo fmt && cargo clippy -p rexops-tui -- -D warnings && cargo test -p rexops-tui 2>&1 | tail -5`
Expected: all green.

```bash
git add crates/rexops-tui/src/ui/cockpit_widgets/mod.rs crates/rexops-tui/src/ui/mod.rs
git commit -m "feat(rexops): cockpit widget vocabulary — LightState + CardInput (Phase B)"
```

---

### Task 2: `StatusLight` widget

**Files:**
- Create: `crates/rexops-tui/src/ui/cockpit_widgets/status_light.rs`
- Modify: `crates/rexops-tui/src/ui/cockpit_widgets/mod.rs` (add `pub mod status_light;`)
- Test: inline `#[cfg(test)]` in `status_light.rs`

**Interfaces:**
- Consumes: `LightState` (Task 1), `suite_ui::Theme`.
- Produces:
  - `pub fn light_glyph(state: LightState) -> &'static str` — `Healthy→"●"`, `Degraded→"◍"`, `Neutral→"○"`, `Down→"✗"`.
  - `pub fn light_span(state: LightState, theme: Theme) -> ratatui::text::Span<'static>` — the glyph styled with the canonical color for that state, derived from `theme`.

> Theme-accessor confirmation (do this BEFORE writing Step 3): the pinned suite-ui re-exports `Health` and a `Theme`. The existing bridge from `AdapterHealth` → `suite_ui::Health` already lives in `crates/rexops-tui/src/ui/widgets.rs` (function `health_to_suite`) and `screens/dashboard.rs` calls `theme.health(...)` and `theme.dim()`. Grep both to learn the EXACT `Health` variant names and the `theme.health(...)`/`theme.dim()` signatures, and reuse them. If a `Health` variant is not named `Ok`/`Degraded`/`Down`, use the real names.

- [ ] **Step 1: Write the failing test**

Create `crates/rexops-tui/src/ui/cockpit_widgets/status_light.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::cockpit_widgets::LightState;

    #[test]
    fn each_state_has_its_distinct_glyph() {
        assert_eq!(light_glyph(LightState::Healthy), "●");
        assert_eq!(light_glyph(LightState::Degraded), "◍");
        assert_eq!(light_glyph(LightState::Neutral), "○");
        assert_eq!(light_glyph(LightState::Down), "✗");
        let all = [
            light_glyph(LightState::Healthy),
            light_glyph(LightState::Degraded),
            light_glyph(LightState::Neutral),
            light_glyph(LightState::Down),
        ];
        let mut uniq = all;
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), 4, "all four lamp glyphs must be distinct");
    }

    #[test]
    fn span_carries_the_glyph_text() {
        let theme = suite_ui::Theme::with_color(true);
        let span = light_span(LightState::Healthy, theme);
        assert_eq!(span.content.as_ref(), "●");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-tui status_light:: 2>&1 | tail -20`
Expected: FAIL — `light_glyph`/`light_span` not found / module not declared.

- [ ] **Step 3: Write the minimal implementation**

Put above the test module in `status_light.rs` (adjust `Health::*` variant names to the real ones found via the grep above):

```rust
//! status_light.rs — the one-glyph health lamp.
//!
//! `●`/`◍`/`○`/`✗` for healthy/degraded/neutral/down. One glyph, one colour —
//! the warning light the cockpit can't-miss. Colour comes from the shared
//! `Theme` so "green = healthy" reads identically across the whole suite.

use ratatui::style::Style;
use ratatui::text::Span;
use suite_ui::Theme;

use crate::ui::cockpit_widgets::LightState;

/// The lamp glyph for a state. Distinct per state so it's unambiguous.
pub fn light_glyph(state: LightState) -> &'static str {
    match state {
        LightState::Healthy => "●",
        LightState::Degraded => "◍",
        LightState::Neutral => "○",
        LightState::Down => "✗",
    }
}

/// The lamp glyph styled with the canonical colour for its state, derived from
/// the shared theme. Healthy/Degraded/Down reuse the theme's health styling so
/// the cockpit matches every other suite surface; Neutral is the theme's dim.
pub fn light_span(state: LightState, theme: Theme) -> Span<'static> {
    let style: Style = match state {
        LightState::Healthy => theme.health(suite_ui::Health::Ok),
        LightState::Degraded => theme.health(suite_ui::Health::Degraded),
        LightState::Down => theme.health(suite_ui::Health::Down),
        LightState::Neutral => theme.dim(),
    };
    Span::styled(light_glyph(state), style)
}

// Learning Notes
// - The glyph fn is split from the styled-span fn so tests can assert the glyph
//   identity without depending on theme colours (which vary with NO_COLOR).
// - We map onto suite-ui's existing `Health` styling rather than inventing new
//   colours, so the lamp is consistent with the health strip/badges already in
//   the suite. Only "Neutral" has no Health equivalent → theme.dim().
```

- [ ] **Step 4: Wire the module + run tests**

Add `pub mod status_light;` to `crates/rexops-tui/src/ui/cockpit_widgets/mod.rs`.

Run: `cargo test -p rexops-tui status_light:: 2>&1 | tail -20`
Expected: both tests PASS.

- [ ] **Step 5: Gates green, then commit**

Run: `cargo fmt && cargo clippy -p rexops-tui -- -D warnings && cargo test -p rexops-tui 2>&1 | tail -5`
Expected: all green.

```bash
git add crates/rexops-tui/src/ui/cockpit_widgets/status_light.rs crates/rexops-tui/src/ui/cockpit_widgets/mod.rs
git commit -m "feat(rexops): StatusLight cockpit widget (Phase B)"
```

---

### Task 3: `StatusCard` widget

**Files:**
- Create: `crates/rexops-tui/src/ui/cockpit_widgets/status_card.rs`
- Modify: `crates/rexops-tui/src/ui/cockpit_widgets/mod.rs` (add `pub mod status_card;`)
- Test: inline `#[cfg(test)]` in `status_card.rs`

**Interfaces:**
- Consumes: `CardInput` (Task 1), `light_span` (Task 2), `suite_ui::{pane, Theme}`, `ratatui`.
- Produces: `pub fn render_status_card(f: &mut Frame, input: CardInput, area: Rect, theme: Theme)` — draws one card: a `pane` framing three lines — line 1 `<light> <name>`, line 2 dim `<role>`, line 3 the `<vital>` (or `"—"` when `None`). When `input.dim`, the name/vital use the theme's dim style.

> Theme-accessor confirmation: this card uses a "plain text" style and a "dim" style. `theme.dim()` is confirmed (used in dashboard.rs). For plain text, grep `screens/dashboard.rs`/`ui/widgets.rs` for how normal text is styled — if there is a `theme.text()`/`theme.normal()` accessor, use it; otherwise use `ratatui::style::Style::default()`. Decide once here and reuse the same choice in Tasks 5–6.

- [ ] **Step 1: Write the failing test**

Create `crates/rexops-tui/src/ui/cockpit_widgets/status_card.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::cockpit_widgets::{CardInput, LightState};
    use ratatui::{backend::TestBackend, Terminal};

    fn render(input: CardInput) -> String {
        let backend = TestBackend::new(28, 6);
        let mut terminal = Terminal::new(backend).expect("backend");
        let theme = suite_ui::Theme::with_color(false); // NO_COLOR → assert text only
        terminal
            .draw(|f| render_status_card(f, input, f.area(), theme))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let w = buf.area.width as usize;
        let mut out = String::new();
        for (i, cell) in buf.content.iter().enumerate() {
            if i % w == 0 && i != 0 {
                out.push('\n');
            }
            out.push_str(cell.symbol());
        }
        out
    }

    #[test]
    fn card_shows_light_name_role_and_vital() {
        let text = render(CardInput {
            name: "Bulwark",
            role: "security",
            light: LightState::Healthy,
            vital: Some("1 crit 1 high"),
            dim: false,
        });
        assert!(text.contains("Bulwark"), "name on the card:\n{text}");
        assert!(text.contains("security"), "role on the card:\n{text}");
        assert!(text.contains("1 crit 1 high"), "vital on the card:\n{text}");
        assert!(text.contains('●'), "healthy lamp glyph on the card:\n{text}");
    }

    #[test]
    fn card_without_a_vital_shows_a_dash() {
        let text = render(CardInput {
            name: "Pulse",
            role: "heartbeat",
            light: LightState::Neutral,
            vital: None,
            dim: true,
        });
        assert!(text.contains("Pulse"), "name present:\n{text}");
        assert!(text.contains('—'), "missing vital renders as em dash:\n{text}");
        assert!(text.contains('○'), "neutral lamp glyph:\n{text}");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-tui status_card:: 2>&1 | tail -20`
Expected: FAIL — `render_status_card` not found.

- [ ] **Step 3: Write the minimal implementation**

Put above the test module in `status_card.rs` (use the plain-text style decided above; the sample uses a `text_style` local you set once):

```rust
//! status_card.rs — one component as a card: a lamp + name, its role, one vital.
//!
//! The cockpit's core widget. Three short lines inside the shared rounded
//! `pane`: `<light> <name>` / `<role>` / `<vital>`. One number per instrument —
//! depth lives one keypress away on the detail screens, not here.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use suite_ui::{pane, Theme};

use crate::ui::cockpit_widgets::status_light::light_span;
use crate::ui::cockpit_widgets::CardInput;

/// Draw a single status card into `area`.
pub fn render_status_card(f: &mut Frame, input: CardInput, area: Rect, theme: Theme) {
    let block = pane("", theme);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // light + name
            Constraint::Length(1), // role
            Constraint::Length(1), // vital
        ])
        .split(inner);

    // Plain-text style chosen once (see Task 3 note). Default works under NO_COLOR.
    let text_style: Style = Style::default();
    let name_style = if input.dim { theme.dim() } else { text_style };

    let title = Line::from(vec![
        light_span(input.light, theme),
        Span::raw(" "),
        Span::styled(input.name.to_owned(), name_style),
    ]);
    f.render_widget(Paragraph::new(title), rows[0]);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(input.role.to_owned(), theme.dim()))),
        rows[1],
    );

    let vital = input.vital.unwrap_or("—");
    let vital_style = if input.dim { theme.dim() } else { text_style };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(vital.to_owned(), vital_style))),
        rows[2],
    );
}

// Learning Notes
// - The card draws the pane, then splits its INNER rect into three 1-line rows.
//   Using `block.inner(area)` before rendering the block is the standard ratatui
//   "frame + content" pattern and keeps the text off the border.
// - `vital.unwrap_or("—")` makes "no number yet" explicit (a planned component)
//   rather than a blank line that reads as a render bug.
```

- [ ] **Step 4: Wire the module + run tests**

Add `pub mod status_card;` to `cockpit_widgets/mod.rs`.

Run: `cargo test -p rexops-tui status_card:: 2>&1 | tail -20`
Expected: both tests PASS.

- [ ] **Step 5: Gates green, then commit**

Run: `cargo fmt && cargo clippy -p rexops-tui -- -D warnings && cargo test -p rexops-tui 2>&1 | tail -5`
Expected: all green.

```bash
git add crates/rexops-tui/src/ui/cockpit_widgets/status_card.rs crates/rexops-tui/src/ui/cockpit_widgets/mod.rs
git commit -m "feat(rexops): StatusCard cockpit widget (Phase B)"
```

---

### Task 4: `CardGrid` — grouped, responsive layout

**Files:**
- Create: `crates/rexops-tui/src/ui/cockpit_widgets/card_grid.rs`
- Modify: `crates/rexops-tui/src/ui/cockpit_widgets/mod.rs` (add `pub mod card_grid;`)
- Test: inline `#[cfg(test)]` in `card_grid.rs`

**Interfaces:**
- Consumes: `CardInput` (Task 1), `render_status_card` (Task 3), `ratatui`, `suite_ui::Theme`.
- Produces:
  - `pub struct CardSection<'a> { pub label: &'a str, pub cards: &'a [CardInput<'a>] }` — one labelled group of cards.
  - `pub fn render_card_grid(f: &mut Frame, sections: &[CardSection], area: Rect, theme: Theme)` — renders each section as a dim label row followed by its cards in responsive columns. Sections stack vertically.
  - `pub fn columns_for_width(width: u16) -> usize` — responsive column count (extracted so it's unit-testable without a Frame).

- [ ] **Step 1: Write the failing test**

Create `crates/rexops-tui/src/ui/cockpit_widgets/card_grid.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::cockpit_widgets::{CardInput, LightState};
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn column_count_is_responsive_to_width() {
        assert_eq!(columns_for_width(80), 3, "wide → 3 columns");
        assert_eq!(columns_for_width(50), 2, "medium → 2 columns");
        assert_eq!(columns_for_width(30), 1, "narrow → 1 column (stacks)");
    }

    #[test]
    fn grid_renders_section_label_and_all_cards() {
        let cards = [
            CardInput { name: "Workstate", role: "brain", light: LightState::Healthy, vital: Some("3/3 fresh"), dim: false },
            CardInput { name: "Bulwark", role: "security", light: LightState::Degraded, vital: Some("1 crit"), dim: false },
        ];
        let sections = [CardSection { label: "BRAIN", cards: &cards }];

        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).expect("backend");
        let theme = suite_ui::Theme::with_color(false);
        terminal
            .draw(|f| render_card_grid(f, &sections, f.area(), theme))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let w = buf.area.width as usize;
        let mut out = String::new();
        for (i, cell) in buf.content.iter().enumerate() {
            if i % w == 0 && i != 0 { out.push('\n'); }
            out.push_str(cell.symbol());
        }
        assert!(out.contains("BRAIN"), "section label rendered:\n{out}");
        assert!(out.contains("Workstate"), "first card rendered:\n{out}");
        assert!(out.contains("Bulwark"), "second card rendered:\n{out}");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-tui card_grid:: 2>&1 | tail -20`
Expected: FAIL — `columns_for_width`/`render_card_grid`/`CardSection` not found.

- [ ] **Step 3: Write the minimal implementation**

Put above the test module in `card_grid.rs`:

```rust
//! card_grid.rs — lay grouped status cards into responsive columns.
//!
//! Each section is a dim label row ("BRAIN") followed by its cards in 1–3
//! columns depending on terminal width, so the cockpit reflows gracefully from
//! a wide terminal down to a narrow one (one column, stacked). The grouping is
//! the metaphor made visible: Brain / Monitors / Field Tools as on-screen rows.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use suite_ui::Theme;

use crate::ui::cockpit_widgets::status_card::render_status_card;
use crate::ui::cockpit_widgets::CardInput;

/// One labelled group of cards.
pub struct CardSection<'a> {
    pub label: &'a str,
    pub cards: &'a [CardInput<'a>],
}

/// Responsive column count. 3 wide, 2 medium, 1 narrow — thresholds chosen so a
/// card (~22 cols + borders) is never crushed below readability.
pub fn columns_for_width(width: u16) -> usize {
    if width >= 66 {
        3
    } else if width >= 44 {
        2
    } else {
        1
    }
}

/// Card height in rows: 3 content lines + 2 border rows.
const CARD_HEIGHT: u16 = 5;

/// Render all sections top-to-bottom into `area`.
pub fn render_card_grid(f: &mut Frame, sections: &[CardSection], area: Rect, theme: Theme) {
    let cols = columns_for_width(area.width);

    let section_constraints: Vec<Constraint> = sections
        .iter()
        .map(|s| {
            let rows = s.cards.len().div_ceil(cols).max(1) as u16;
            Constraint::Length(1 + rows * CARD_HEIGHT)
        })
        .collect();

    let section_rects = Layout::default()
        .direction(Direction::Vertical)
        .constraints(section_constraints)
        .split(area);

    for (section, srect) in sections.iter().zip(section_rects.iter()) {
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(*srect);

        f.render_widget(
            Paragraph::new(Line::from(section.label.to_owned()).style(theme.dim())),
            parts[0],
        );

        render_cards_in_columns(f, section.cards, parts[1], cols, theme);
    }
}

/// Lay a section's cards into `cols` columns across `area`, wrapping into rows.
fn render_cards_in_columns(
    f: &mut Frame,
    cards: &[CardInput],
    area: Rect,
    cols: usize,
    theme: Theme,
) {
    if cards.is_empty() {
        return;
    }
    let row_count = cards.len().div_ceil(cols);
    let row_constraints: Vec<Constraint> =
        (0..row_count).map(|_| Constraint::Length(CARD_HEIGHT)).collect();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .split(area);

    for (r, row_rect) in rows.iter().enumerate() {
        let col_constraints: Vec<Constraint> =
            (0..cols).map(|_| Constraint::Ratio(1, cols as u32)).collect();
        let cells = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(col_constraints)
            .split(*row_rect);

        for c in 0..cols {
            let idx = r * cols + c;
            if let Some(card) = cards.get(idx) {
                render_status_card(f, *card, cells[c], theme);
            }
        }
    }
}

// Learning Notes
// - `div_ceil` gives the row count without float math; pre-sizing sections by
//   their card-row count means the vertical layout never clips a card.
// - Columns use `Ratio(1, cols)` so they share width evenly and reflow when the
//   terminal resizes — the responsive behaviour is just `columns_for_width`
//   feeding the constraint count, which is why that fn is extracted + tested.
```

> Implementer note: `div_ceil` on `usize` is stable. `Line::from(s).style(...)` is used above to avoid relying on `Line::styled` (which may not exist in the pinned ratatui); if `.style()` on `Line` is unavailable, use `Line::from(Span::styled(s, theme.dim()))`. `theme.dim()` is confirmed in dashboard.rs.

- [ ] **Step 4: Wire the module + run tests**

Add `pub mod card_grid;` to `cockpit_widgets/mod.rs`.

Run: `cargo test -p rexops-tui card_grid:: 2>&1 | tail -20`
Expected: both tests PASS.

- [ ] **Step 5: Gates green, then commit**

Run: `cargo fmt && cargo clippy -p rexops-tui -- -D warnings && cargo test -p rexops-tui 2>&1 | tail -5`
Expected: all green.

```bash
git add crates/rexops-tui/src/ui/cockpit_widgets/card_grid.rs crates/rexops-tui/src/ui/cockpit_widgets/mod.rs
git commit -m "feat(rexops): CardGrid responsive grouped layout (Phase B)"
```

---

### Task 5: `IdentityBanner` widget

**Files:**
- Create: `crates/rexops-tui/src/ui/cockpit_widgets/identity_banner.rs`
- Modify: `crates/rexops-tui/src/ui/cockpit_widgets/mod.rs` (add `pub mod identity_banner;`)
- Test: inline `#[cfg(test)]` in `identity_banner.rs`

**Interfaces:**
- Consumes: `suite_ui::Theme`, `ratatui`.
- Produces:
  - `pub struct BannerInput<'a> { pub host: Option<&'a str>, pub kernel: Option<&'a str>, pub uptime: Option<&'a str>, pub clock: &'a str, pub live: usize, pub total: usize, pub alerts: usize }`.
  - `pub fn render_identity_banner(f: &mut Frame, input: BannerInput, area: Rect, theme: Theme)` — one line: `host · kernel · up <uptime>      <clock>   N/M live · K alerts`. Missing host/kernel/uptime are omitted (no empty `·` segments). `alerts == 0` styled calm; `> 0` uses the theme's loud/attention styling.

> Theme-accessor confirmation: the loud alert style needs an "attention"/emphasis style. Grep `screens/*.rs` and `ui/widgets.rs` for how emphasis/severity is styled today. If `theme.attention()` exists, use it; else use the "down" health style (`theme.health(suite_ui::Health::Down)` with the variant name confirmed in Task 2). Plain text style: reuse the same choice made in Task 3.

- [ ] **Step 1: Write the failing test**

Create `crates/rexops-tui/src/ui/cockpit_widgets/identity_banner.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn render(input: BannerInput) -> String {
        let backend = TestBackend::new(80, 3);
        let mut terminal = Terminal::new(backend).expect("backend");
        let theme = suite_ui::Theme::with_color(false);
        terminal
            .draw(|f| render_identity_banner(f, input, f.area(), theme))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let w = buf.area.width as usize;
        let mut out = String::new();
        for (i, cell) in buf.content.iter().enumerate() {
            if i % w == 0 && i != 0 { out.push('\n'); }
            out.push_str(cell.symbol());
        }
        out
    }

    #[test]
    fn banner_shows_identity_and_rollup() {
        let text = render(BannerInput {
            host: Some("rex-laptop"),
            kernel: Some("6.17"),
            uptime: Some("4d 2h"),
            clock: "2026-06-20 21:14Z",
            live: 3,
            total: 11,
            alerts: 2,
        });
        assert!(text.contains("rex-laptop"), "host:\n{text}");
        assert!(text.contains("6.17"), "kernel:\n{text}");
        assert!(text.contains("3/11 live"), "live rollup:\n{text}");
        assert!(text.contains("2 alerts"), "alert count:\n{text}");
    }

    #[test]
    fn banner_omits_absent_fields_without_empty_separators() {
        let text = render(BannerInput {
            host: None,
            kernel: None,
            uptime: None,
            clock: "now",
            live: 0,
            total: 0,
            alerts: 0,
        });
        assert!(!text.contains("· ·"), "no empty separator runs:\n{text}");
        assert!(text.contains("now"), "clock still shown:\n{text}");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-tui identity_banner:: 2>&1 | tail -20`
Expected: FAIL — `render_identity_banner`/`BannerInput` not found.

- [ ] **Step 3: Write the minimal implementation**

Put above the test module in `identity_banner.rs` (use the attention/plain styles decided above):

```rust
//! identity_banner.rs — the cockpit's top bar: who/where + a one-glance rollup.
//!
//! `host · kernel · up <uptime>      <clock>   N/M live · K alerts`. Absent
//! facts are omitted (no empty `·` runs). The alert count is the one place the
//! banner gets loud: 0 alerts reads calm, >0 uses the theme's attention styling.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use suite_ui::Theme;

/// Borrowed inputs for the banner.
pub struct BannerInput<'a> {
    pub host: Option<&'a str>,
    pub kernel: Option<&'a str>,
    pub uptime: Option<&'a str>,
    pub clock: &'a str,
    pub live: usize,
    pub total: usize,
    pub alerts: usize,
}

/// Render the banner into `area` (expects a 1-row-tall rect).
pub fn render_identity_banner(f: &mut Frame, input: BannerInput, area: Rect, theme: Theme) {
    let mut left: Vec<String> = Vec::new();
    if let Some(h) = input.host {
        left.push(h.to_owned());
    }
    if let Some(k) = input.kernel {
        left.push(format!("kernel {k}"));
    }
    if let Some(u) = input.uptime {
        left.push(format!("up {u}"));
    }
    let identity = left.join(" · ");

    let rollup = format!("{}/{} live", input.live, input.total);
    let alerts = format!("{} alerts", input.alerts);

    // Plain text + loud-alert styles (see Task 5 note for the confirmed accessors).
    let text_style: Style = Style::default();
    let alert_style = if input.alerts > 0 {
        theme.health(suite_ui::Health::Down)
    } else {
        theme.dim()
    };

    let line = Line::from(vec![
        Span::styled(identity, theme.dim()),
        Span::raw("    "),
        Span::styled(input.clock.to_owned(), theme.dim()),
        Span::raw("   "),
        Span::styled(rollup, text_style),
        Span::styled(" · ", theme.dim()),
        Span::styled(alerts, alert_style),
    ]);

    f.render_widget(Paragraph::new(line), area);
}

// Learning Notes
// - Building the identity from a Vec joined by " · " is what prevents the empty
//   "· ·" runs when host/kernel/uptime are absent — you can't get a separator
//   without two real segments.
// - The alert span is the only conditional style: calm at 0, loud above. That's
//   the "calm by default, loud on trouble" rule applied at the banner level.
```

> Implementer note: confirm the `suite_ui::Health::Down` variant name from Task 2's grep; if the suite exposes a dedicated emphasis style (e.g. `theme.attention()`/`theme.accent()`), prefer that for `alert_style`.

- [ ] **Step 4: Wire the module + run tests**

Add `pub mod identity_banner;` to `cockpit_widgets/mod.rs`.

Run: `cargo test -p rexops-tui identity_banner:: 2>&1 | tail -20`
Expected: both tests PASS.

- [ ] **Step 5: Gates green, then commit**

Run: `cargo fmt && cargo clippy -p rexops-tui -- -D warnings && cargo test -p rexops-tui 2>&1 | tail -5`
Expected: all green.

```bash
git add crates/rexops-tui/src/ui/cockpit_widgets/identity_banner.rs crates/rexops-tui/src/ui/cockpit_widgets/mod.rs
git commit -m "feat(rexops): IdentityBanner cockpit widget (Phase B)"
```

---

### Task 6: The `cockpit` screen — assemble + route

**Files:**
- Create: `crates/rexops-tui/src/screens/cockpit.rs`
- Modify: `crates/rexops-tui/src/screens/mod.rs` (declare + re-export)
- Modify: `crates/rexops-tui/src/ui/layout.rs` (route `Screen::Dashboard` → `render_cockpit`)
- Test: inline `#[cfg(test)]` in `cockpit.rs`

**Interfaces:**
- Consumes: `App` (read-only), `OpsSnapshot.components`/`.system`/`.risk`, the widgets (Tasks 2–5), `light_state_from_health` + `CardInput` (Task 1), `suite_ui::{pane, Theme}`.
- Produces: `pub fn render_cockpit(f: &mut Frame, app: &App, area: Rect, theme: Theme)` — banner (top), grouped card grid (middle, from `app.snapshot.components`), one-line risk/hint strip (bottom). Pure render.

- [ ] **Step 1: Write the failing test**

Create `crates/rexops-tui/src/screens/cockpit.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    use rexops_core::{AdapterHealth, AppConfig, ComponentStatus, OpsSnapshot};
    use std::sync::mpsc;

    fn app_with_components() -> App {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default(), None);
        let mut snap = OpsSnapshot::new();
        snap.push_component(ComponentStatus {
            id: "workstate".into(), name: "Workstate".into(), group: "brain".into(),
            maturity: "live".into(), health: AdapterHealth::Healthy,
            freshness: None, vital: Some("3/3 fresh".into()), launchable: false,
        });
        snap.push_component(ComponentStatus {
            id: "pulse".into(), name: "Pulse".into(), group: "monitor".into(),
            maturity: "planned".into(), health: AdapterHealth::Unknown,
            freshness: None, vital: None, launchable: false,
        });
        app.apply_snapshot(snap);
        app
    }

    fn render(app: &App) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("backend");
        let theme = suite_ui::Theme::with_color(false);
        terminal.draw(|f| render_cockpit(f, app, f.area(), theme)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let w = buf.area.width as usize;
        let mut out = String::new();
        for (i, cell) in buf.content.iter().enumerate() {
            if i % w == 0 && i != 0 { out.push('\n'); }
            out.push_str(cell.symbol());
        }
        out
    }

    #[test]
    fn cockpit_renders_a_card_per_component_grouped() {
        let app = app_with_components();
        let text = render(&app);
        assert!(text.contains("Workstate"), "workstate card:\n{text}");
        assert!(text.contains("Pulse"), "pulse card:\n{text}");
        assert!(text.contains("3/3 fresh"), "workstate vital:\n{text}");
        assert!(text.to_uppercase().contains("BRAIN"), "brain group:\n{text}");
        assert!(text.to_uppercase().contains("MONITOR"), "monitor group:\n{text}");
    }

    #[test]
    fn planned_component_renders_neutral_lamp() {
        let app = app_with_components();
        let text = render(&app);
        assert!(text.contains('○'), "neutral lamp for planned pulse:\n{text}");
    }

    #[test]
    fn empty_components_show_guidance_not_a_blank_screen() {
        let (tx, _rx) = mpsc::channel();
        let app = App::new(tx, AppConfig::default(), None); // no snapshot applied
        let text = render(&app);
        assert!(
            text.to_lowercase().contains("no components")
                || text.to_lowercase().contains("press 'r'"),
            "empty cockpit guides the user instead of rendering blank:\n{text}"
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-tui cockpit:: 2>&1 | tail -20`
Expected: FAIL — `render_cockpit` not found.

- [ ] **Step 3: Write the minimal implementation**

Put above the test module in `cockpit.rs`:

```rust
//! screens/cockpit.rs — the cockpit landing screen (replaces the Dashboard).
//!
//! The suite's state at a glance: an identity banner, a grid of component status
//! cards grouped by the metaphor (Brain / Monitors / Field Tools / …), and a
//! one-line risk/hint strip. Pure render — it reads only the already-resolved
//! `OpsSnapshot.components` the Phase A registry walk produced, plus system
//! facts and the risk rollup. No I/O, no app mutation.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use suite_ui::{pane, Theme};

use crate::app::App;
use crate::ui::cockpit_widgets::card_grid::{render_card_grid, CardSection};
use crate::ui::cockpit_widgets::identity_banner::{render_identity_banner, BannerInput};
use crate::ui::cockpit_widgets::{light_state_from_health, CardInput};

/// The metaphor groups, in display order, with the `ComponentStatus.group`
/// strings they match (group strings come from `ComponentGroup::label()`).
const GROUP_ORDER: &[(&str, &[&str])] = &[
    ("BRAIN", &["brain"]),
    ("MONITORS", &["monitor"]),
    ("BLACK BOX", &["black box"]),
    ("FIELD TOOLS", &["field tool"]),
    ("MECHANICS", &["mechanic"]),
    ("FACTORY", &["factory"]),
];

/// Render the cockpit into `area`.
pub fn render_cockpit(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // identity banner
            Constraint::Min(6),    // card grid
            Constraint::Length(1), // risk / hint strip
        ])
        .split(area);

    render_banner(f, app, chunks[0], theme);

    if app.snapshot.components.is_empty() {
        let msg = Paragraph::new(Line::from(
            "No components yet — press 'r' to probe the suite.",
        ))
        .block(pane("Cockpit", theme));
        f.render_widget(msg, chunks[1]);
    } else {
        render_grid(f, app, chunks[1], theme);
    }

    render_risk_strip(f, app, chunks[2], theme);
}

fn render_banner(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let sys = app.snapshot.system.as_ref();
    let live = app
        .snapshot
        .components
        .iter()
        .filter(|c| c.maturity == "live")
        .count();
    let total = app.snapshot.components.len();
    let input = BannerInput {
        host: sys.and_then(|s| s.hostname.as_deref()),
        kernel: sys.and_then(|s| s.kernel.as_deref()),
        uptime: sys.and_then(|s| s.uptime.as_deref()),
        clock: "",
        live,
        total,
        alerts: alerts_count(app),
    };
    render_identity_banner(f, input, area, theme);
}

/// Render the grouped card grid. Card-input storage is built per group and kept
/// alive on the stack for the duration of that group's render (CardInput borrows).
fn render_grid(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let comps = &app.snapshot.components;
    let group_rects = pre_split_groups(area, comps);

    for ((label, ids), grect) in GROUP_ORDER.iter().zip(group_rects.iter()) {
        let inputs: Vec<CardInput> = comps
            .iter()
            .filter(|c| ids.contains(&c.group.as_str()))
            .map(|c| CardInput {
                name: &c.name,
                role: &c.group,
                light: light_state_from_health(c.health),
                vital: c.vital.as_deref(),
                dim: c.maturity == "planned",
            })
            .collect();
        if inputs.is_empty() {
            continue;
        }
        let sections = [CardSection { label, cards: &inputs }];
        render_card_grid(f, &sections, *grect, theme);
    }
}

/// Vertically split `area` into one rect per GROUP_ORDER entry, proportional to
/// each group's card count (empty groups get a zero-share slice).
fn pre_split_groups(area: Rect, comps: &[rexops_core::ComponentStatus]) -> Vec<Rect> {
    let counts: Vec<u16> = GROUP_ORDER
        .iter()
        .map(|(_, ids)| comps.iter().filter(|c| ids.contains(&c.group.as_str())).count() as u16)
        .collect();
    let total: u32 = u32::from(counts.iter().copied().sum::<u16>().max(1));
    let constraints: Vec<Constraint> = counts
        .iter()
        .map(|&n| Constraint::Ratio(u32::from(n), total))
        .collect();
    Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area)
        .to_vec()
}

fn render_risk_strip(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let r = &app.snapshot.risk;
    let text = format!(
        "RISK  crit {} · high {} · med {} · low {}    [1] cockpit  [r] refresh  [?] help",
        r.critical, r.high, r.medium, r.low
    );
    f.render_widget(Paragraph::new(Line::from(text).style(theme.dim())), area);
}

/// Alerts = the count that makes the banner loud: critical + high findings.
fn alerts_count(app: &App) -> usize {
    let r = &app.snapshot.risk;
    r.critical + r.high
}

// Learning Notes
// - The cockpit is a PURE projection of OpsSnapshot.components — no probing, no
//   mutation — so it unit-tests off-screen exactly like render_dashboard did.
// - CardInput borrows from each ComponentStatus, so the owned `inputs` Vec is
//   built per group and kept on the stack for that group's render call; that's
//   why we render group-by-group rather than building one global slice.
// - GROUP_ORDER makes the metaphor the layout and fixes display order; a
//   component whose group string isn't listed simply isn't shown (none today).
```

> Implementer notes (resolve while implementing):
> 1. **Clock:** if there's an easy source (e.g. `rexops_core::format_unix_millis_utc(app.snapshot.generated_at_ms)` — already used by the CLI), pass it as `clock`; otherwise pass `""` and the banner omits it. Don't add a new time dependency. Confirm the snapshot's timestamp field name (`generated_at_ms`) via grep before using it.
> 2. Confirm `app.snapshot.system` field names (`hostname`/`kernel`/`uptime` are `Option<String>` on `SystemInfo` — verified in Phase A). Confirm `RiskSummary` has `critical/high/medium/low` (used in `screens/dashboard.rs`).
> 3. `Line::from(text).style(...)` — if `.style()` on `Line` is unavailable in the pinned ratatui, use `Paragraph::new(text).style(theme.dim())` instead. Keep consistent with Task 4's choice.
> 4. Remove any unused imports so `clippy -D warnings` passes (only import what the screen uses).

- [ ] **Step 4: Wire the screen into routing**

In `crates/rexops-tui/src/screens/mod.rs`, add `pub mod cockpit;` and in the `pub use` block `pub use cockpit::render_cockpit;`.

In `crates/rexops-tui/src/ui/layout.rs`, find the single `render_dashboard(...)` call (the `Screen::Dashboard` arm) and change it to `render_cockpit(...)` with the same arguments. Leave every other screen arm untouched.

Run: `grep -n "render_dashboard" crates/rexops-tui/src/ui/layout.rs`
Expected after edit: no matches in `layout.rs` (the call now goes to `render_cockpit`). `screens/dashboard.rs`'s own tests still call `render_dashboard` directly — that's fine.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rexops-tui cockpit:: 2>&1 | tail -30`
Expected: the three cockpit tests PASS.

Run: `cargo test -p rexops-tui 2>&1 | tail -6`
Expected: all rexops-tui tests pass (the old dashboard tests still pass — that module is intact, just no longer routed).

- [ ] **Step 6: Full workspace gates, then commit**

Run: `cargo fmt && cargo clippy --workspace -- -D warnings && cargo test --workspace 2>&1 | tail -8`
Expected: all green across the workspace.

```bash
git add crates/rexops-tui/src/screens/cockpit.rs crates/rexops-tui/src/screens/mod.rs crates/rexops-tui/src/ui/layout.rs
git commit -m "feat(rexops): cockpit landing screen replaces Dashboard render (Phase B)"
```

---

### Task 7: Manual smoke + dashboard-module cleanup decision

**Files:**
- Modify (optional): `crates/rexops-tui/src/screens/dashboard.rs` and/or `screens/mod.rs` (only if removing/annotating)

**Interfaces:** none (verification + a scoped cleanup decision).

- [ ] **Step 1: Manual smoke test of the live cockpit**

Pipe the Workstate fixture so the cards populate (the fixture the CLI used in Phase A), launch the TUI, press `r`, observe screen 1:

Run: `cat crates/rexops-adapters/fixtures/workstate/snapshot_v3.json | cargo run -q -p rexops-cli 2>/dev/null`
Expected: an identity banner (host/kernel/uptime + `N/M live · K alerts`), grouped cards (BRAIN: Workstate `3/3 fresh`; FIELD TOOLS: Bulwark, ScriptVault `3 scripts`, ToolFoundry `2 need review`; MONITORS/BLACK BOX/etc.: planned cards muted with `○`), and the risk strip at the bottom. Press `2`–`7` to confirm other screens still work, `?` for help, `q` to quit. Paste a short description into the Task 7 report.

> If running interactively in CI is impractical, at minimum confirm the binary builds and the cockpit render test covers the shape: `cargo test -p rexops-tui cockpit::`.

- [ ] **Step 2: Decide the dashboard module's fate**

The old `screens/dashboard.rs` is now unrouted but still compiled (its tests run). Pick one and note it in the report:
- **(a) Keep it** one more phase as a documented fallback (zero risk). Add a top-of-file note: `//! NOTE: superseded by screens/cockpit.rs as of Phase B; retained as a fallback, not routed.`
- **(b) Remove it** now: delete `dashboard.rs`, drop its `pub mod`/`pub use` from `screens/mod.rs`, and confirm nothing else references `render_dashboard` (grep first).

Default to **(a)** unless the reviewer prefers removal — additive + reversible is safer for a UI-replacing phase.

- [ ] **Step 3: Apply the chosen option and commit**

For (a):
```bash
git add crates/rexops-tui/src/screens/dashboard.rs
git commit -m "docs(rexops): mark dashboard.rs superseded by cockpit (Phase B)"
```
For (b):
```bash
git rm crates/rexops-tui/src/screens/dashboard.rs
git add crates/rexops-tui/src/screens/mod.rs
git commit -m "refactor(rexops): remove superseded dashboard screen (Phase B)"
```

- [ ] **Step 4: Final workspace gate**

Run: `cargo fmt && cargo clippy --workspace -- -D warnings && cargo test --workspace 2>&1 | tail -8`
Expected: all green.

---

## Self-Review

**1. Spec coverage (against design §3 "Cockpit Dashboard" + §5 "suite-ui widgets"):**
- StatusLight → Task 2. ✓
- StatusCard → Task 3. ✓
- CardGrid (responsive, grouped) → Task 4. ✓
- IdentityBanner → Task 5. ✓
- Cockpit landing screen (grid of cards grouped by metaphor + banner + risk strip) replacing Dashboard → Task 6. ✓
- Heartbeat widget (§5 item 3 / §4.2) → **deferred to Phase E** (needs Pulse liveness samples from the StatusCommand adapter, which don't exist yet; a sparkline with no data source is premature). Out of scope here; noted.
- Drill-down (Enter on a card → detail) and one-keypress launch hotkeys (§3.3/§3.4) → **deferred to Phase C** (design §9 Phase C owns interaction; this phase is the read-only status board). Noted, not a gap.
- Widgets' true home is thomas-tui (§5) → built locally this phase per the user's explicit decision ("build locally first, upstream later"); written domain-free so the lift is a move + re-export. The upstream is a named follow-up, not part of Phase B.

**2. Placeholder scan:** No `TBD`/`TODO`/"handle edge cases". Every code step shows complete code. The "Theme-accessor confirmation" notes are concrete grep instructions with named fallbacks, not placeholders.

**3. Type consistency:**
- `LightState`/`CardInput` (Task 1) consumed with identical field names in Tasks 2–6. ✓
- `light_state_from_health` (Task 1) used in Task 6. ✓
- `light_glyph`/`light_span` (Task 2) used by Task 3. ✓
- `render_status_card` + `CardInput` (Task 3) used by Task 4. ✓
- `CardSection` + `render_card_grid` (Task 4) used by Task 6. ✓
- `BannerInput` + `render_identity_banner` (Task 5) used by Task 6. ✓
- `render_cockpit` (Task 6) routed in `ui/layout.rs` and re-exported in `screens/mod.rs`. ✓
- suite-ui `Theme` accessors confirmed-or-fallback in every task; `Style::default()` is the named fallback for plain text and `theme.health(Health::Down)` for loud — so a missing accessor is handled, not assumed. ✓

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-06-21-rexops-cockpit-phase-b-cockpit-screen.md`.
