# RexOps Cockpit — Phase C: Interactive Cockpit — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Phase B cockpit landing screen **interactive** — a focusable card grid where `j`/`k`/arrows move a highlighted card, a per-card **letter marker** (`a`,`s`,`d`,…) arms that component through the existing confirm gate in a single keypress, `Enter` arms the focused card, and `Enter`-on-a-card (when not launchable) or a dedicated drill-down opens a per-component **detail view** — all without touching the global `1`–`7` screen keys, the `/` filter, or any Phase B rendering.

**Architecture:** Phase B made the cockpit a **pure projection** of `OpsSnapshot.components`. Phase C adds exactly three things and nothing else: (1) a cockpit **focus + marker model** living in `App` (`selected_component: Option<String>`, keyed by component `id`, plus a pure helper that flattens the displayed cards into the same visit order the grid draws them in); (2) **keymap + action wiring** so `j`/`k`, a card letter, and `Enter` route to that model and reuse the *already-built* `arm_tool → pending_action → confirm_pending` launch gate; and (3) a small **`CockpitDetail`** screen (a new `Screen` variant) reached by drilling into the focused card, rendered from the static `rexops_core::component_by_id(id)` registry row plus the live `ComponentStatus`. The cockpit stays a pure render of borrowed state — the new selection is *read* on the render path and *mutated* only in `on_action`, exactly like `selected_tool` is today. **Launching is not re-implemented:** a card's letter/Enter calls the existing `App::arm_tool(id, name)`, which already resolves the command, applies the health gate, and opens the same confirm popup the Launcher and palette use. There is **no new launch path, no new confirm modal, and no new process plumbing.**

**Tech Stack:** Rust 2021, `ratatui`, `suite-ui` (`Theme`/`pane`/`keys` from the pinned git dep), `rexops-core` (`ComponentStatus`, `component_by_id`, `COMPONENTS`, `AdapterHealth`, `Maturity`/`RunMode` via the registry), `crossterm` key events, `cargo test`/`clippy`/`fmt`. Off-screen render tests use `ratatui::backend::TestBackend`; interaction tests drive the **real** `App::on_action` path with a no-op `ForegroundRunner` (the existing launcher/esc test pattern in `crates/rexops-tui/src/app/tests/`).

## Global Constraints

- Files stay **under 300 LOC** (ideally < 200); each `.rs` ends with a `// Learning Notes` footer (existing project convention).
- All four cargo gates (`build`, `test`, `clippy --workspace -- -D warnings`, `fmt --check`) green at **every** task's commit. Baseline before Task 1: `cargo test -p rexops-tui --lib` = **139 passed**; the workspace is green at `c439caf`.
- **Phase B rendering is frozen.** `render_cockpit`, the four `cockpit_widgets`, `GROUP_ORDER`, the banner, and the risk strip keep their current output for the *non-interactive* case. Phase C only **adds** a focus highlight + a letter marker to each card and adds the detail screen — it must not move, recolor, or reorder any existing card content. All three existing `screens/cockpit.rs` tests must still pass unchanged.
- **Global keys are sacrosity.** The cockpit must NOT bind the digits `1`–`7` (screen switches), `q` (quit), `r` (refresh), `?` (help), `x` (cancel job), `^P`/`:` (palette), or `Esc`/`^G` (cancel). Card letter markers are drawn ONLY from a curated alphabet that **excludes** every bound navigation letter (`q r x j k h y n`) and every digit — see Task 2's `MARKER_ALPHABET`.
- **Reuse the launch gate, don't fork it.** A card actuation calls the existing `App::arm_tool(id, name)` (commands/dispatch.rs). That function already: returns early + logs for a non-launchable id, applies the `is_tool_available` health gate, and opens the shared `pending_action` confirm popup. Do **not** add a second confirm modal, a second availability check, or a second process spawn.
- **The cockpit keeps its `/` filter.** Phase B made `Screen::Dashboard` a `filter_screen()`; that stays. Letter markers and `j`/`k` are **Navigation-mode only** — while `filtering` (Text mode) every printable key still types into the filter, so the markers never collide with filter input. `Enter` while filtering still *applies the filter* (current behavior), not "launch a card".
- **Selection is keyed by component `id`, not an index.** Card focus survives a refresh that reorders/adds components (a `usize` index would silently point at a different card). Unknown/emptied selection falls back to the first visited card.
- Conventional commits: `feat(rexops): … (Phase C)` / `test(rexops): …` / `refactor(rexops): …`.
- Run all `cargo` commands from the worktree root (`/home/tom/projects/rexops/.claude/worktrees/rexops-cockpit-redesign-doc`).

---

## File Structure

- **Create** `crates/rexops-tui/src/screens/cockpit_nav.rs` — the cockpit's pure focus/marker model: `cockpit_visit_order(components) -> Vec<&ComponentStatus>` (flattens `GROUP_ORDER` exactly as the grid draws it), `marker_for(visit_index) -> Option<char>`, and `component_for_marker(components, key) -> Option<&str>` (key → component id). One responsibility: *the deterministic mapping between a displayed card, its focus position, and its letter.* Domain logic only — no `Frame`, no `App`. `GROUP_ORDER` moves here (re-exported to the screen) so the order has a single source of truth shared by the renderer and the navigator.
- **Modify** `crates/rexops-tui/src/screens/cockpit.rs` — render the focus highlight + letter marker on each card (reading `app.selected_component`); add the empty-state focus guard; import `GROUP_ORDER` from `cockpit_nav`. Render-only changes; the screen stays pure.
- **Create** `crates/rexops-tui/src/screens/cockpit_detail.rs` — `render_cockpit_detail(f, app, area, theme)`: the per-component drill-down, built from `rexops_core::component_by_id(id)` (role, group, maturity, launch spec) + the live `ComponentStatus` (health, vital, freshness) for `app.selected_component`. Pure render.
- **Modify** `crates/rexops-tui/src/ui/cockpit_widgets/status_card.rs` — extend `CardInput` with `marker: Option<char>` and `focused: bool`; draw a dim `[a]` marker on line 1 and an accent selection rail/border when focused. Domain-free still (no rexops types).
- **Modify** `crates/rexops-tui/src/app/navigation.rs` — add the `Screen::CockpitDetail` variant; add cockpit selection helpers (`move_cockpit_selection`, `select_first_cockpit_card`, `keep_cockpit_selection_visible`, `arm_selected_component`, `drill_into_selected_component`, `cockpit_back`).
- **Modify** `crates/rexops-tui/src/app/state.rs` — add the `selected_component: Option<String>` field + init; reconcile it in `apply_snapshot`.
- **Modify** `crates/rexops-tui/src/app/update.rs` — route `Action::Up/Down` and `Action::Activate` for `Screen::Dashboard`/`Screen::CockpitDetail`; handle the new `Action::CardKey(char)` and `Action::Drill`; make `Esc` back out of `CockpitDetail`.
- **Modify** `crates/rexops-tui/src/input/action.rs` — add `Action::CardKey(char)` and `Action::Drill`.
- **Modify** `crates/rexops-tui/src/input/keymap.rs` — in Navigation mode, map a printable non-bound char to `Action::CardKey(c)` (replacing today's blanket `InputChar(c)` for the cockpit's letters — see Task 5 for how this stays compatible with the filter trigger), and map a drill key (`g`) to `Action::Drill`.
- **Modify** `crates/rexops-tui/src/ui/layout.rs` — route `Screen::CockpitDetail → render_cockpit_detail`; add its header name.
- **Modify** `crates/rexops-tui/src/ui/status_bar.rs` — cockpit hint row gains `("a-z", "launch")` and `("g", "detail")`; add a `CockpitDetail` hint row.
- **Modify** `crates/rexops-tui/src/ui/palette.rs` — help sheet documents the cockpit letters + drill key.

> Why a separate `cockpit_nav.rs`: the focus order and the letter assignment must match the *rendered* order byte-for-byte, and they're needed in two places (the renderer draws markers; `on_action` resolves a keypress to a card). Extracting the order into one pure, unit-testable function is what guarantees "the `a` you see is the `a` that fires." Putting it in the screen would force the App layer to import the screen.

---

### Task 1: Cockpit visit order + marker alphabet (the pure nav model)

**Files:**
- Create: `crates/rexops-tui/src/screens/cockpit_nav.rs`
- Modify: `crates/rexops-tui/src/screens/mod.rs` (add `pub mod cockpit_nav;` and re-export the three fns + `GROUP_ORDER`)
- Modify: `crates/rexops-tui/src/screens/cockpit.rs` (delete its local `GROUP_ORDER`; import it from `cockpit_nav`)
- Test: inline `#[cfg(test)]` in `cockpit_nav.rs`

**Interfaces:**
- Consumes: `rexops_core::ComponentStatus`.
- Produces:
  - `pub const GROUP_ORDER: &[(&str, &[&str])]` — moved verbatim from `cockpit.rs` (the metaphor groups + the `group` strings they match). Single source of truth.
  - `pub fn cockpit_visit_order(comps: &[ComponentStatus]) -> Vec<&ComponentStatus>` — the displayed cards in render order: for each `GROUP_ORDER` entry in order, every component whose `group` is in that entry's id list, in `comps` order. Components whose group isn't listed are omitted (matches the renderer, which skips them).
  - `pub const MARKER_ALPHABET: &[char]` — the letters used as card markers, in assignment order, **excluding** every bound nav letter (`q r x j k h y n`) and all digits: `['a','s','d','f','g'... ]` — see the impl for the exact safe set.
  - `pub fn marker_for(visit_index: usize) -> Option<char>` — the marker for the Nth visited card, or `None` past the alphabet.
  - `pub fn component_for_marker<'a>(comps: &'a [ComponentStatus], key: char) -> Option<&'a str>` — maps a pressed key to the `id` of the card it labels (via `cockpit_visit_order` + `MARKER_ALPHABET`), or `None` if the key labels nothing.

> Marker-alphabet rule (read before writing the const): the cockpit runs in Navigation mode, where these single letters are claimed as actions. So the alphabet must NOT contain any letter the global keymap already binds in nav mode: `q`(quit) `r`(refresh) `x`(cancel job) `j`/`k`(nav) `h`(reserved/none) `y`/`n`(confirm answers) and no digits (`1`-`7` switch screens). `g` is the drill key (Task 5) so it is ALSO excluded from markers. The safe ordered set this plan uses: `a s d f w e t z c v b m p l u i o` (17 letters — comfortably more than the 11 components). Pick from this set; if you change it, keep it disjoint from `{q,r,x,j,k,h,y,n,g}` and digits, and add an assertion (Step 1 already asserts disjointness).

- [ ] **Step 1: Write the failing test**

Create `crates/rexops-tui/src/screens/cockpit_nav.rs`:

```rust
//! cockpit_nav.rs — the cockpit's pure focus/marker model.
//!
//! The one place that decides the order cards are visited (identical to the
//! order the grid draws them) and which letter labels each card. The renderer
//! draws the markers from here; `App::on_action` resolves a pressed letter to a
//! component id from here — so "the `a` you see is the `a` that fires" is true by
//! construction, not by two lists happening to agree. No `Frame`, no `App`: this
//! is domain logic over a borrowed `&[ComponentStatus]`.

use rexops_core::ComponentStatus;

/// The metaphor groups, in display order, with the `ComponentStatus.group`
/// strings they match. Single source of truth: the renderer (`screens/cockpit`)
/// and the navigator both read THIS, so the on-screen order and the focus order
/// can never drift.
pub const GROUP_ORDER: &[(&str, &[&str])] = &[
    ("BRAIN", &["brain"]),
    ("MONITORS", &["monitor"]),
    ("BLACK BOX", &["black box"]),
    ("FIELD TOOLS", &["field tool"]),
    ("MECHANICS", &["mechanic"]),
    ("FACTORY", &["factory"]),
];

/// Letters used as card markers, in assignment order. Deliberately disjoint from
/// every nav-mode binding (`q r x j k h y n`, the drill key `g`) and all digits
/// (`1`-`7` switch screens) so a marker keypress can never shadow a global key.
pub const MARKER_ALPHABET: &[char] = &[
    'a', 's', 'd', 'f', 'w', 'e', 't', 'z', 'c', 'v', 'b', 'm', 'p', 'l', 'u', 'i', 'o',
];

/// The displayed cards in render order (the order the grid draws them):
/// group-by-group per `GROUP_ORDER`, each group's members in `comps` order.
/// A component whose group isn't listed is omitted — exactly as the renderer
/// skips it.
pub fn cockpit_visit_order(comps: &[ComponentStatus]) -> Vec<&ComponentStatus> {
    let mut out = Vec::new();
    for (_, ids) in GROUP_ORDER {
        for c in comps {
            if ids.contains(&c.group.as_str()) {
                out.push(c);
            }
        }
    }
    out
}

/// The marker letter for the Nth visited card, or `None` past the alphabet.
pub fn marker_for(visit_index: usize) -> Option<char> {
    MARKER_ALPHABET.get(visit_index).copied()
}

/// Map a pressed key to the id of the card it labels, or `None` if it labels no
/// visible card. Case-insensitive on the marker letter.
pub fn component_for_marker(comps: &[ComponentStatus], key: char) -> Option<&str> {
    let key = key.to_ascii_lowercase();
    let pos = MARKER_ALPHABET.iter().position(|&m| m == key)?;
    cockpit_visit_order(comps).get(pos).map(|c| c.id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rexops_core::{AdapterHealth, ComponentStatus};

    fn comp(id: &str, group: &str) -> ComponentStatus {
        ComponentStatus {
            id: id.into(),
            name: id.into(),
            group: group.into(),
            maturity: "live".into(),
            health: AdapterHealth::Healthy,
            freshness: None,
            vital: None,
            launchable: false,
        }
    }

    #[test]
    fn visit_order_follows_group_order_not_input_order() {
        // Input is shuffled across groups; visit order must be BRAIN then
        // FIELD TOOLS (per GROUP_ORDER), regardless of input order.
        let comps = vec![
            comp("bulwark", "field tool"),
            comp("workstate", "brain"),
            comp("proto", "field tool"),
        ];
        let order: Vec<&str> = cockpit_visit_order(&comps).iter().map(|c| c.id.as_str()).collect();
        assert_eq!(order, vec!["workstate", "bulwark", "proto"]);
    }

    #[test]
    fn marker_alphabet_excludes_every_bound_nav_key() {
        // The whole point: a marker letter must never be a global command.
        for bound in ['q', 'r', 'x', 'j', 'k', 'h', 'y', 'n', 'g'] {
            assert!(
                !MARKER_ALPHABET.contains(&bound),
                "marker alphabet must not contain bound nav key '{bound}'"
            );
        }
        // And it must be long enough for the whole registry (11 components).
        assert!(MARKER_ALPHABET.len() >= 11, "need a marker per component");
    }

    #[test]
    fn pressed_letter_resolves_to_the_card_it_labels() {
        let comps = vec![comp("workstate", "brain"), comp("bulwark", "field tool")];
        // First visited card → first marker ('a'); second → 's'.
        assert_eq!(marker_for(0), Some('a'));
        assert_eq!(marker_for(1), Some('s'));
        assert_eq!(component_for_marker(&comps, 'a'), Some("workstate"));
        assert_eq!(component_for_marker(&comps, 's'), Some("bulwark"));
        // Case-insensitive; an unlabeled letter resolves to nothing.
        assert_eq!(component_for_marker(&comps, 'A'), Some("workstate"));
        assert_eq!(component_for_marker(&comps, 'd'), None);
    }
}

// Learning Notes
// - cockpit_visit_order mirrors render_grid's group-by-group walk EXACTLY; the
//   marker the user presses lands on the card they see because both sides flatten
//   through this one function. Two independent lists would eventually disagree.
// - The marker alphabet is curated, not `'a'..='z'`, precisely so a card letter
//   can never collide with a global nav key. Disjointness is asserted, not hoped.
```

- [ ] **Step 2: Run test to verify it fails (then passes once wired)**

Run: `cargo test -p rexops-tui cockpit_nav:: 2>&1 | tail -20`
Expected: FAIL — module not declared / not found until Step 3 wires it.

- [ ] **Step 3: Wire the module + move `GROUP_ORDER` to its single home**

In `crates/rexops-tui/src/screens/mod.rs`, add `pub mod cockpit_nav;` and re-export:

```rust
pub use cockpit_nav::{cockpit_visit_order, marker_for, component_for_marker, GROUP_ORDER};
```

In `crates/rexops-tui/src/screens/cockpit.rs`, **delete** the local `const GROUP_ORDER` block (lines defining the metaphor groups) and import it instead:

```rust
use crate::screens::cockpit_nav::GROUP_ORDER;
```

(The screen's `render_grid` / `pre_split_groups` keep using `GROUP_ORDER` exactly as before — only its definition site moves.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rexops-tui cockpit_nav:: 2>&1 | tail -20`
Expected: all three nav tests PASS.

Run: `cargo test -p rexops-tui cockpit:: 2>&1 | tail -10`
Expected: the three Phase B cockpit render tests STILL pass (the move of `GROUP_ORDER` is transparent to them).

- [ ] **Step 5: Gates green, then commit**

Run: `cargo fmt && cargo clippy -p rexops-tui -- -D warnings && cargo test -p rexops-tui --lib 2>&1 | tail -5`
Expected: all green; the lib test count is **142** (139 + 3 new nav tests).

```bash
git add crates/rexops-tui/src/screens/cockpit_nav.rs crates/rexops-tui/src/screens/mod.rs crates/rexops-tui/src/screens/cockpit.rs
git commit -m "feat(rexops): cockpit visit-order + marker alphabet (Phase C)"
```

---

### Task 2: `StatusCard` grows a marker + focus highlight

**Files:**
- Modify: `crates/rexops-tui/src/ui/cockpit_widgets/status_card.rs` (extend `CardInput`, draw marker + focus)
- Modify: `crates/rexops-tui/src/ui/cockpit_widgets/mod.rs` (add the two fields to the `CardInput` struct defined there)
- Test: extend the inline `#[cfg(test)]` in `status_card.rs`

**Interfaces:**
- Consumes: `suite_ui::{pane, Theme}`, `light_span` (Phase B), `ratatui`.
- Produces (additive — Phase B fields unchanged):
  - `CardInput<'a>` gains `pub marker: Option<char>` (the letter label, drawn dim as `[a]` before the name; `None` → no marker drawn) and `pub focused: bool` (when true, the card draws with the accent selection styling — `theme.selected_rail()` glyph + `theme.selection()` name — the same focus look the Launcher rows use).

> Theme accessors to reuse (confirmed in the codebase): `theme.selected_rail()` and `theme.selection()` are used by `screens/launchpad.rs::render_launcher_row` for the selected-row look; `theme.dim()` is everywhere. Use those three — do NOT invent a focus color. The marker is dim; the focus rail is the accent.

> IMPORTANT — Phase B callers must keep compiling. `CardInput` is constructed in `screens/cockpit.rs::render_grid` and in several `cockpit_widgets` tests. Adding non-`Default` fields to a struct literal breaks every construction site. This task adds the fields AND updates the in-crate construction sites in the SAME commit (the cockpit screen passes real values in Task 3; here, update the widget's own tests and add `marker: None, focused: false` to any other literal so the crate builds). Grep first: `grep -rn "CardInput {" crates/rexops-tui/src`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `status_card.rs` (keep the existing `render` helper and Phase B tests):

```rust
    #[test]
    fn card_draws_its_marker_letter() {
        let text = render(CardInput {
            name: "Bulwark",
            role: "security",
            light: LightState::Healthy,
            vital: Some("1 crit"),
            dim: false,
            marker: Some('s'),
            focused: false,
        });
        assert!(text.contains("[s]"), "marker label on the card:\n{text}");
        assert!(text.contains("Bulwark"), "name still present:\n{text}");
    }

    #[test]
    fn focused_card_shows_the_selection_rail() {
        let text = render(CardInput {
            name: "Workstate",
            role: "brain",
            light: LightState::Healthy,
            vital: Some("3/3 fresh"),
            dim: false,
            marker: Some('a'),
            focused: true,
        });
        // The accent rail glyph the suite uses for the focused row.
        assert!(text.contains('▌'), "focused card shows the rail glyph:\n{text}");
    }

    #[test]
    fn unfocused_card_without_marker_is_unchanged_from_phase_b() {
        // Back-compat: None marker + not focused renders name/role/vital as before.
        let text = render(CardInput {
            name: "Pulse",
            role: "heartbeat",
            light: LightState::Neutral,
            vital: None,
            dim: true,
            marker: None,
            focused: false,
        });
        assert!(text.contains("Pulse"), "name:\n{text}");
        assert!(text.contains('—'), "missing vital still an em dash:\n{text}");
        assert!(!text.contains('▌'), "no rail when unfocused:\n{text}");
        assert!(!text.contains('['), "no marker brackets when marker is None:\n{text}");
    }
```

(Update the two existing Phase B card tests — `card_shows_light_name_role_and_vital` and `card_without_a_vital_shows_a_dash` — to add `marker: None, focused: false` to their `CardInput` literals so they compile.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-tui status_card:: 2>&1 | tail -20`
Expected: FAIL — `CardInput` has no field `marker` / `focused`.

- [ ] **Step 3: Add the fields to `CardInput`**

In `crates/rexops-tui/src/ui/cockpit_widgets/mod.rs`, extend the struct (keep the Phase B fields and doc comments):

```rust
#[derive(Debug, Clone, Copy)]
pub struct CardInput<'a> {
    pub name: &'a str,
    pub role: &'a str,
    pub light: LightState,
    pub vital: Option<&'a str>,
    /// Render muted (a planned / inactive component).
    pub dim: bool,
    /// The single-letter hotkey label for this card, drawn dim as `[a]` before
    /// the name. `None` draws no marker (a card with no actuation letter).
    pub marker: Option<char>,
    /// Whether this card currently has cockpit focus — drawn with the accent
    /// selection rail + name, the same focus look the Launcher rows use.
    pub focused: bool,
}
```

- [ ] **Step 4: Draw the marker + focus in `render_status_card`**

Replace the body of `render_status_card` in `status_card.rs` (the Phase B three-row layout stays; line 1 gains the rail + marker, and the name style respects focus):

```rust
/// Draw a single status card into `area`.
pub fn render_status_card(f: &mut Frame, input: CardInput, area: Rect, theme: Theme) {
    let block = pane("", theme);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // rail? + marker? + light + name
            Constraint::Length(1), // role
            Constraint::Length(1), // vital
        ])
        .split(inner);

    let text_style: Style = Style::default();

    // Line 1: optional focus rail, optional dim marker, the lamp, then the name.
    let mut spans = Vec::new();
    if input.focused {
        spans.push(Span::styled("▌", theme.selected_rail()));
    }
    if let Some(m) = input.marker {
        spans.push(Span::styled(format!("[{m}] "), theme.dim()));
    } else if input.focused {
        // keep the lamp column aligned when focused but unlabeled
        spans.push(Span::raw(" "));
    }
    spans.push(light_span(input.light, theme));
    spans.push(Span::raw(" "));
    let name_style = if input.focused {
        theme.selection()
    } else if input.dim {
        theme.dim()
    } else {
        text_style
    };
    spans.push(Span::styled(input.name.to_owned(), name_style));
    f.render_widget(Paragraph::new(Line::from(spans)), rows[0]);

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
```

(Add `use crate::ui::cockpit_widgets::CardInput;` is already present; no new imports beyond `Span`/`Line`/`Style` which Phase B already imports. If `Span` isn't imported, add `use ratatui::text::Span;`.)

Then fix any other in-crate `CardInput { … }` literal the grep found (add `marker: None, focused: false`).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rexops-tui status_card:: 2>&1 | tail -20`
Expected: the three new card tests PASS and the two updated Phase B card tests PASS.

- [ ] **Step 6: Gates green, then commit**

Run: `cargo fmt && cargo clippy -p rexops-tui -- -D warnings && cargo test -p rexops-tui --lib 2>&1 | tail -5`
Expected: all green. (The cockpit *screen* still constructs `CardInput` without the new fields → it will fail to build only if Task 3 hasn't run yet; since Step 4 told you to patch every in-crate literal, the build is green here. The screen's literal is updated for real in Task 3.)

> If the screen literal in `screens/cockpit.rs` was NOT yet updated and the crate won't build, add `marker: None, focused: false` to it now as a placeholder — Task 3 replaces those `None/false` with the real focus+marker values. Keeping the crate green every commit is the constraint that wins ties.

```bash
git add crates/rexops-tui/src/ui/cockpit_widgets/status_card.rs crates/rexops-tui/src/ui/cockpit_widgets/mod.rs crates/rexops-tui/src/screens/cockpit.rs
git commit -m "feat(rexops): StatusCard marker + focus highlight (Phase C)"
```

---

### Task 3: Cockpit screen draws focus + markers from `selected_component`

**Files:**
- Modify: `crates/rexops-tui/src/screens/cockpit.rs` (`render_grid` passes real `marker`/`focused`; empty-state unchanged)
- Modify: `crates/rexops-tui/src/app/state.rs` (add the `selected_component` field + init so the screen can read it; reconciliation is Task 4)
- Test: extend the inline `#[cfg(test)]` in `cockpit.rs`

**Interfaces:**
- Consumes: `app.selected_component: Option<String>` (added here as a plain field; the *movement* helpers come in Task 4), `cockpit_nav::{cockpit_visit_order, marker_for}` (Task 1).
- Produces: `render_grid` now assigns each card its marker (by its index in `cockpit_visit_order`) and sets `focused = Some(id) == app.selected_component`. The marker index must be the card's **global** visit index (across all groups), not its index within its group — so markers run `a,s,d,…` continuously down the whole cockpit.

> The clean way to keep marker indices global while still rendering group-by-group: build the full visit order once (`cockpit_visit_order(comps)`), then for each card look up its position in that order to get its marker. A component id is unique, so `position(|c| c.id == this.id)` is unambiguous. This keeps Phase B's group-by-group render loop intact and just adds a marker lookup per card.

- [ ] **Step 1: Add the field so the screen can read it**

In `crates/rexops-tui/src/app/state.rs`, add to the `App` struct (near `selected_adapter`):

```rust
    /// The cockpit card currently focused, keyed by component `id` (NOT an index,
    /// so focus survives a refresh that reorders/adds components). `None` before
    /// the first snapshot or when no card is visible. Moved by the cockpit nav
    /// helpers; read on the cockpit render path.
    pub selected_component: Option<String>,
```

And initialize it in `App::new` (near `selected_adapter: None,`):

```rust
            selected_component: None,
```

- [ ] **Step 2: Write the failing test**

Add to the `tests` module in `screens/cockpit.rs` (reuse the existing `app_with_components` + `render` helpers):

```rust
    #[test]
    fn cards_show_their_marker_letters() {
        let app = app_with_components();
        let text = render(&app);
        // First visited card (Workstate, BRAIN) → 'a'; the monitor card → next.
        assert!(text.contains("[a]"), "first card labelled [a]:\n{text}");
    }

    #[test]
    fn focused_component_renders_with_the_rail() {
        let mut app = app_with_components();
        app.selected_component = Some("workstate".to_owned());
        let text = render(&app);
        assert!(text.contains('▌'), "focused workstate shows the rail:\n{text}");
    }

    #[test]
    fn no_focus_renders_no_rail() {
        let app = app_with_components(); // selected_component defaults to None
        let text = render(&app);
        assert!(!text.contains('▌'), "nothing focused → no rail:\n{text}");
    }
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p rexops-tui cockpit::tests::cards_show_their_marker 2>&1 | tail -20`
Expected: FAIL — cards render with `marker: None`/`focused: false` (the Task 2 placeholder), so no `[a]` / no rail.

- [ ] **Step 4: Pass real marker + focus in `render_grid`**

In `screens/cockpit.rs`, update `render_grid` to compute markers from the global visit order and focus from `selected_component`:

```rust
use crate::screens::cockpit_nav::{cockpit_visit_order, marker_for, GROUP_ORDER};

// ...

/// Render the grouped card grid. Card-input storage is built per group and kept
/// alive on the stack for that group's render (CardInput borrows from each row).
fn render_grid(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let comps = &app.snapshot.components;
    let group_rects = pre_split_groups(area, comps);

    // The global visit order — markers are assigned by a card's position HERE, so
    // letters run a,s,d,… continuously down the whole cockpit, not per group.
    let visit = cockpit_visit_order(comps);
    let focused = app.selected_component.as_deref();

    for ((label, ids), grect) in GROUP_ORDER.iter().zip(group_rects.iter()) {
        let inputs: Vec<CardInput> = comps
            .iter()
            .filter(|c| ids.contains(&c.group.as_str()))
            .map(|c| {
                let marker = visit
                    .iter()
                    .position(|v| v.id == c.id)
                    .and_then(marker_for);
                CardInput {
                    name: &c.name,
                    role: &c.group,
                    light: light_state_from_health(c.health),
                    vital: c.vital.as_deref(),
                    dim: c.maturity == "planned",
                    marker,
                    focused: focused == Some(c.id.as_str()),
                }
            })
            .collect();
        if inputs.is_empty() {
            continue;
        }
        let sections = [CardSection {
            label,
            cards: &inputs,
        }];
        render_card_grid(f, &sections, *grect, theme);
    }
}
```

(Remove the now-redundant `use crate::screens::cockpit_nav::GROUP_ORDER;` line from Task 1 if it duplicates — keep a single combined `use` as shown above.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rexops-tui cockpit:: 2>&1 | tail -20`
Expected: the three new screen tests PASS **and** all three Phase B cockpit tests still PASS (markers/focus are additive; the names/vitals/groups they assert are unchanged).

- [ ] **Step 6: Gates green, then commit**

Run: `cargo fmt && cargo clippy -p rexops-tui -- -D warnings && cargo test -p rexops-tui --lib 2>&1 | tail -5`
Expected: all green.

```bash
git add crates/rexops-tui/src/screens/cockpit.rs crates/rexops-tui/src/app/state.rs
git commit -m "feat(rexops): cockpit renders card markers + focus highlight (Phase C)"
```

---

### Task 4: Cockpit selection model — move, reconcile, arm

**Files:**
- Modify: `crates/rexops-tui/src/app/navigation.rs` (selection helpers + `arm_selected_component`)
- Modify: `crates/rexops-tui/src/app/state.rs` (reconcile `selected_component` in `apply_snapshot`)
- Test: a new `crates/rexops-tui/src/app/tests/cockpit.rs` (register it in `app/tests/mod.rs`)

**Interfaces:**
- Consumes: `cockpit_nav::{cockpit_visit_order, component_for_marker}` (Task 1), the existing `App::arm_tool(id, name)` (commands/dispatch.rs).
- Produces (all `pub(crate)`):
  - `fn select_first_cockpit_card(&mut self)` — set `selected_component` to the first visited card's id (or `None` if none).
  - `fn keep_cockpit_selection_visible(&mut self)` — after a snapshot change, keep the selection if its id is still visible; else fall back to the first card (or `None`).
  - `fn move_cockpit_selection(&mut self, down: bool)` — step focus along `cockpit_visit_order` with wraparound (mirrors `move_adapter_selection`, but over the cockpit order and keyed by id).
  - `fn arm_selected_component(&mut self)` — arm the focused card's component via `arm_tool` (id+name from the live `ComponentStatus`); a `None`/absent selection is a no-op.
  - `fn arm_component_by_marker(&mut self, key: char)` — resolve `key` → id via `component_for_marker`, set focus to it, and arm it. Unknown key → no-op.

> Reuse note: `arm_tool` already does ALL the gating — it logs "disabled" for a non-launchable id and "unavailable" for a down adapter, and only opens the confirm popup when the component is actually launchable+available. So `arm_selected_component` does NOT pre-check `launchable`; it just looks up the component's `name` and calls `arm_tool(id, name)`. Pressing a planned card's letter is therefore *safe and self-explaining* — it logs "X: disabled (no launch command)" instead of doing nothing silently.

- [ ] **Step 1: Write the failing test**

Create `crates/rexops-tui/src/app/tests/cockpit.rs`:

```rust
//! Cockpit interaction tests: focus movement, marker actuation, and the
//! confirm-gate handoff — driven through the real `App::on_action` path.
//!
//! This submodule is declared from `app/tests/mod.rs` (`mod cockpit;`) and gets
//! that module's shared imports + helpers via `use super::*` — including the
//! existing `FakeRunner` (a no-op `ForegroundRunner`), `App`, `Screen`, and
//! `Action`. We reuse `FakeRunner` rather than defining another runner.

use super::*; // App, Screen, Action, FakeRunner, mpsc, rexops_core::{AppConfig, OpsSnapshot}

use rexops_core::{AdapterConfig, AdapterHealth, ComponentStatus};

/// A fresh no-op runner. (`FakeRunner` from the parent module counts calls; we
/// only need a runner that succeeds, so either works — this names it locally.)
fn runner() -> FakeRunner {
    FakeRunner { calls: 0 }
}

fn comp(id: &str, name: &str, group: &str, maturity: &str, launchable: bool) -> ComponentStatus {
    ComponentStatus {
        id: id.into(),
        name: name.into(),
        group: group.into(),
        maturity: maturity.into(),
        health: AdapterHealth::Healthy,
        freshness: None,
        vital: None,
        launchable,
    }
}

/// An app on the cockpit with two cards: Workstate (brain, not launchable) and
/// Bulwark (field tool, launchable). Bulwark's launch command is forced
/// resolvable via the config-binary fallback so `arm_tool` opens the gate
/// without depending on a `bulwark` binary on the dev PATH.
fn cockpit_app() -> App {
    let (tx, _rx) = mpsc::channel();
    let mut cfg = AppConfig::default();
    cfg.adapters.insert(
        "bulwark".to_owned(),
        AdapterConfig { enabled: true, binary: Some("/bin/true".to_owned()), timeout_secs: None },
    );
    let mut app = App::new(tx, cfg, None);
    let mut snap = OpsSnapshot::new();
    snap.push_component(comp("workstate", "Workstate", "brain", "live", false));
    snap.push_component(comp("bulwark", "Bulwark", "field tool", "live", true));
    app.apply_snapshot(snap);
    app
}

#[test]
fn snapshot_selects_the_first_card() {
    let app = cockpit_app();
    assert_eq!(app.selected_component.as_deref(), Some("workstate"));
}

#[test]
fn down_moves_focus_to_the_next_card_and_wraps() {
    let mut app = cockpit_app();
    let mut r = runner();
    app.on_action(Action::Down, &mut r);
    assert_eq!(app.selected_component.as_deref(), Some("bulwark"));
    app.on_action(Action::Down, &mut r); // wrap back to the first
    assert_eq!(app.selected_component.as_deref(), Some("workstate"));
}

#[test]
fn pressing_a_launchable_cards_letter_opens_the_confirm_gate() {
    let mut app = cockpit_app();
    let mut r = runner();
    // Bulwark is the 2nd visited card → marker 's'.
    app.on_action(Action::CardKey('s'), &mut r);
    assert!(app.pending_action.is_some(), "a launchable card's letter arms it");
    assert_eq!(app.selected_component.as_deref(), Some("bulwark"), "and focuses it");
}

#[test]
fn pressing_a_non_launchable_cards_letter_does_not_open_the_gate() {
    let mut app = cockpit_app();
    let mut r = runner();
    // Workstate (brain) is not launchable → 'a' must NOT open the confirm gate.
    app.on_action(Action::CardKey('a'), &mut r);
    assert!(app.pending_action.is_none(), "a non-launchable card cannot be armed");
}

#[test]
fn enter_on_a_launchable_focused_card_opens_the_gate() {
    let mut app = cockpit_app();
    let mut r = runner();
    app.on_action(Action::Down, &mut r); // focus Bulwark
    app.on_action(Action::Activate, &mut r);
    assert!(app.pending_action.is_some(), "Enter arms the focused launchable card");
}

#[test]
fn focus_survives_a_reordering_refresh() {
    let mut app = cockpit_app();
    let mut r = runner();
    app.on_action(Action::Down, &mut r); // focus Bulwark
    // A refresh arrives with the components in a different order.
    let mut snap = OpsSnapshot::new();
    snap.push_component(comp("bulwark", "Bulwark", "field tool", "live", true));
    snap.push_component(comp("workstate", "Workstate", "brain", "live", false));
    app.apply_snapshot(snap);
    assert_eq!(app.selected_component.as_deref(), Some("bulwark"), "focus tracks the id, not the slot");
}

#[test]
fn focus_falls_back_when_the_focused_card_disappears() {
    let mut app = cockpit_app();
    let mut r = runner();
    app.on_action(Action::Down, &mut r); // focus Bulwark
    // Bulwark vanishes from the next snapshot.
    let mut snap = OpsSnapshot::new();
    snap.push_component(comp("workstate", "Workstate", "brain", "live", false));
    app.apply_snapshot(snap);
    assert_eq!(app.selected_component.as_deref(), Some("workstate"), "falls back to the first card");
}
```

Register the module: in `crates/rexops-tui/src/app/tests/mod.rs`, add `mod cockpit;`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-tui app::tests::cockpit 2>&1 | tail -25`
Expected: FAIL — `Action::CardKey` not found (added in Task 5) and the helpers/reconciliation not implemented.

> CardKey is defined in Task 5. To keep Task 4 independently testable, **add the `Action::CardKey(char)` and `Action::Drill` variants to `input/action.rs` now** (they're tiny enums) as part of this task — Task 5 only wires the *keymap* that emits them and the *help/hints*. This avoids a forward-reference that won't compile. So: add the two variants in Step 3 below.

- [ ] **Step 3: Add the action variants + the selection helpers**

In `crates/rexops-tui/src/input/action.rs`, add to the `Action` enum:

```rust
    /// A cockpit card hotkey (its dim letter marker) was pressed — arm that card.
    CardKey(char),

    /// Drill into the focused cockpit card's detail view (g).
    Drill,
```

In `crates/rexops-tui/src/app/navigation.rs`, add the `Screen::CockpitDetail` variant and the cockpit helpers:

```rust
/// Top-level screen selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Dashboard,
    Adapters,
    System,
    Scripts,
    Tools,
    Launcher,
    Jobs,
    /// The per-component drill-down reached from a focused cockpit card.
    CockpitDetail,
}

impl App {
    // ... existing helpers unchanged ...

    /// Focus the first cockpit card (in visit order), or clear focus if none.
    pub(crate) fn select_first_cockpit_card(&mut self) {
        let first = crate::screens::cockpit_visit_order(&self.snapshot.components)
            .first()
            .map(|c| c.id.clone());
        self.selected_component = first;
    }

    /// After a snapshot change, keep the focused id if it is still visible; else
    /// fall back to the first visited card (or `None`).
    pub(crate) fn keep_cockpit_selection_visible(&mut self) {
        let visible: Vec<String> = crate::screens::cockpit_visit_order(&self.snapshot.components)
            .iter()
            .map(|c| c.id.clone())
            .collect();
        match &self.selected_component {
            Some(id) if visible.iter().any(|v| v == id) => {}
            _ => self.selected_component = visible.into_iter().next(),
        }
    }

    /// Step cockpit focus along the visit order with wraparound.
    pub(crate) fn move_cockpit_selection(&mut self, down: bool) {
        let order: Vec<String> = crate::screens::cockpit_visit_order(&self.snapshot.components)
            .iter()
            .map(|c| c.id.clone())
            .collect();
        if order.is_empty() {
            return;
        }
        let next = match &self.selected_component {
            Some(cur) => order.iter().position(|id| id == cur).map(|pos| {
                let n = if down {
                    (pos + 1) % order.len()
                } else if pos > 0 {
                    pos - 1
                } else {
                    order.len() - 1
                };
                order[n].clone()
            }),
            None => order.first().cloned(),
        };
        if let Some(id) = next {
            self.selected_component = Some(id);
        }
    }

    /// Arm the focused cockpit card through the shared confirm gate. A `None`
    /// selection is a no-op. Gating (launchable / available) is `arm_tool`'s job.
    pub(crate) fn arm_selected_component(&mut self) {
        let Some(id) = self.selected_component.clone() else {
            return;
        };
        let name = self
            .snapshot
            .components
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone());
        if let Some(name) = name {
            self.arm_tool(id, name);
        }
    }

    /// Resolve a pressed marker letter to a card, focus it, and arm it.
    pub(crate) fn arm_component_by_marker(&mut self, key: char) {
        let id = crate::screens::component_for_marker(&self.snapshot.components, key)
            .map(|s| s.to_owned());
        if let Some(id) = id {
            self.selected_component = Some(id);
            self.arm_selected_component();
        }
    }
}
```

> Note the borrow dance: each helper collects the visit order into owned `Vec<String>` before mutating `selected_component`, so no immutable borrow of `snapshot.components` is alive across the mutable write — the same pattern `move_adapter_selection` uses.

In `crates/rexops-tui/src/app/state.rs`, reconcile the selection inside `apply_snapshot` (add the call alongside the existing `keep_selected_adapter_visible()`):

```rust
        self.keep_selected_adapter_visible();
        self.keep_cockpit_selection_visible();
```

- [ ] **Step 4: Wire movement + Enter into `on_action` (cockpit arm)**

In `crates/rexops-tui/src/app/update.rs`, three edits:

(a) Route cockpit movement in `move_selection` — change the `Screen::Dashboard` arm so the cockpit moves *its own* card focus, not the adapter table:

```rust
    fn move_selection(&mut self, down: bool) {
        match self.current_screen {
            // The cockpit moves its own card focus (keyed by component id).
            Screen::Dashboard => self.move_cockpit_selection(down),
            // Adapters keeps the filtered adapter table + its shared selection.
            Screen::Adapters => self.move_adapter_selection(down),
            Screen::Jobs => self.scroll_jobs_output(!down),
            Screen::Launcher => {
                let len = tools::CATALOG.len();
                if len > 0 {
                    let cur = self.selected_tool.min(len - 1);
                    self.selected_tool = if down {
                        (cur + 1) % len
                    } else {
                        (cur + len - 1) % len
                    };
                }
            }
            _ => {}
        }
    }
```

(b) Arm on Enter in `activate_selection` — add the cockpit arm (and, for a non-launchable focused card, drill into it so Enter is never inert):

```rust
    fn activate_selection(&mut self) {
        match self.current_screen {
            Screen::Adapters => {
                if let Some(name) = &self.selected_adapter {
                    self.snapshot.add_note(format!(
                        "selected adapter detail: {name} (press r to refresh for live)"
                    ));
                }
            }
            Screen::Launcher => {
                if let Some(tool) = tools::CATALOG.get(self.selected_tool) {
                    self.arm_tool(tool.id.to_owned(), tool.name.to_owned());
                }
            }
            // On the cockpit, Enter arms the focused card if it is launchable;
            // otherwise it drills into the card's detail (so Enter is never a
            // silent no-op on a planned/read-only card). `arm_tool` itself gates
            // launchability, so we check `launchable` here only to choose between
            // arm vs. drill.
            Screen::Dashboard => {
                let launchable = self
                    .selected_component
                    .as_deref()
                    .and_then(|id| self.snapshot.components.iter().find(|c| c.id == id))
                    .map(|c| c.launchable)
                    .unwrap_or(false);
                if launchable {
                    self.arm_selected_component();
                } else {
                    self.drill_into_selected_component();
                }
            }
            _ => {}
        }
    }
```

(`drill_into_selected_component` is added in Task 6; for THIS task's commit, add a temporary stub so the crate builds and the arm tests pass — Task 6 fills it in:)

```rust
    // In app/navigation.rs, temporary stub (Task 6 implements the screen switch):
    pub(crate) fn drill_into_selected_component(&mut self) {
        // Filled in by Task 6 (switch to Screen::CockpitDetail). Stubbed here so
        // Enter on a non-launchable card compiles; arm tests don't exercise it.
    }
```

(c) Handle `Action::CardKey` and `Action::Drill` in the bottom `match` of `on_action` (alongside the other top-level arms, BEFORE the `InputChar`/filter arms — but note these only fire in Navigation mode, so a filtering user never reaches them because the keymap emits `InputChar` while filtering):

```rust
            Action::CardKey(c) => {
                // Cockpit-only: a card's letter arms it. On any other screen it's
                // inert (no card grid). Navigation mode only — while filtering the
                // keymap emits InputChar, so letters type into the filter instead.
                if self.current_screen == Screen::Dashboard {
                    self.arm_component_by_marker(c);
                }
                false
            }
            Action::Drill => {
                if self.current_screen == Screen::Dashboard {
                    self.drill_into_selected_component();
                }
                false
            }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rexops-tui app::tests::cockpit 2>&1 | tail -25`
Expected: all seven cockpit interaction tests PASS.

Run: `cargo test -p rexops-tui app::tests:: 2>&1 | tail -8`
Expected: the existing app tests (esc, filters, launcher, palette, refresh, jobs, help) still PASS — the only behavioral change is `Screen::Dashboard` movement, and the existing dashboard tests assert filter behavior (unchanged) not j/k-moves-the-adapter-table.

> If an existing test asserted that `j`/`k` on the Dashboard moves the *adapter* selection, that assumption changed by design (the cockpit now moves card focus). Re-read such a test: update it to assert cockpit focus movement, or move that assertion to a `Screen::Adapters` setup where the adapter table still owns j/k. Note the change in the task report.

- [ ] **Step 6: Gates green, then commit**

Run: `cargo fmt && cargo clippy -p rexops-tui -- -D warnings && cargo test -p rexops-tui --lib 2>&1 | tail -5`
Expected: all green.

```bash
git add crates/rexops-tui/src/app/navigation.rs crates/rexops-tui/src/app/state.rs crates/rexops-tui/src/app/update.rs crates/rexops-tui/src/input/action.rs crates/rexops-tui/src/app/tests/cockpit.rs crates/rexops-tui/src/app/tests/mod.rs
git commit -m "feat(rexops): cockpit card focus movement + marker/Enter arming (Phase C)"
```

---

### Task 5: Keymap — emit `CardKey` + `Drill` without stealing globals or the filter

**Files:**
- Modify: `crates/rexops-tui/src/input/keymap.rs` (Navigation-mode bindings for the drill key + card letters)
- Test: extend the inline `#[cfg(test)]` in `keymap.rs`

**Interfaces:**
- Consumes: `Action::{CardKey, Drill}` (Task 4).
- Produces: in **Navigation** mode only, `handle_key_navigation` maps `g` → `Action::Drill` and any *other* printable char that isn't an existing binding → `Action::CardKey(c)` (instead of today's blanket `InputChar(c)`). **Text mode is unchanged** — every printable key is still `InputChar` there, so the `/` filter keeps capturing letters.

> The delicate bit — `/` must still trigger the filter. Today the *navigation* `Char(c)` catch-all returns `InputChar(c)`, and `on_action` turns `InputChar('/')` into "enter filter mode" on a filter screen. If we blanket-replace the nav catch-all with `CardKey`, `/` would become `CardKey('/')` and the filter trigger breaks. So the nav catch-all must special-case `/` (and any other char `on_action` consumes as a command) to stay `InputChar`, and only map the *marker-eligible* letters to `CardKey`. The simplest correct rule: keep `/` (and space) as `InputChar`; map every other non-bound `Char` to `CardKey`. `on_action` ignores `CardKey` on non-cockpit screens, so this is safe app-wide. Concretely: add an explicit `KeyCode::Char('/')`→`InputChar('/')` arm before the catch-all, and change the final catch-all from `InputChar(c)` to `CardKey(c)`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `keymap.rs`:

```rust
    #[test]
    fn nav_mode_maps_card_letters_to_cardkey() {
        // A marker-eligible letter in nav mode is a CardKey now (the cockpit arms
        // it). The bound nav keys are unaffected (covered by other tests).
        assert_eq!(handle_key(ch('a'), InputMode::Navigation), Some(Action::CardKey('a')));
        assert_eq!(handle_key(ch('s'), InputMode::Navigation), Some(Action::CardKey('s')));
        // `g` is the drill key, not a CardKey.
        assert_eq!(handle_key(ch('g'), InputMode::Navigation), Some(Action::Drill));
    }

    #[test]
    fn nav_mode_keeps_slash_as_the_filter_trigger() {
        // `/` must stay InputChar so on_action can enter filter mode — it must NOT
        // become a CardKey, or the cockpit/adapters filter would break.
        assert_eq!(handle_key(ch('/'), InputMode::Navigation), Some(Action::InputChar('/')));
    }

    #[test]
    fn text_mode_letters_are_unchanged_literal_input() {
        // The filter (Text mode) still captures every letter as input — CardKey is
        // a NAV-only concept. This guards that we didn't leak CardKey into Text.
        for c in ['a', 's', 'g', '/', 'q', '1'] {
            assert_eq!(handle_key(ch(c), InputMode::Text), Some(Action::InputChar(c)),
                "Text mode: '{c}' stays literal input");
        }
    }
```

> The existing tests `h_is_no_longer_a_help_toggle_in_either_mode` and `ctrl_g_cancels_in_both_modes_as_the_esc_free_escape` assert that bare `h` and bare `g` map to `InputChar` in **Navigation** mode. Those assertions change: bare `g` is now `Action::Drill`, and bare `h` is now `Action::CardKey('h')` **only if `h` is in the marker alphabet** — it is NOT (excluded), so to keep `h` harmless, the catch-all maps it to `CardKey('h')` which the cockpit ignores (no card uses `h`). Update those two tests: `g`→`Drill` in nav, `h`→`CardKey('h')` in nav (still `InputChar` in Text). The *intent* (these keys don't toggle help/quit) is preserved.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-tui keymap:: 2>&1 | tail -25`
Expected: FAIL — `g`/`a`/`s` still map to `InputChar` (old catch-all).

- [ ] **Step 3: Update `handle_key_navigation`**

In `keymap.rs`, change the tail of `handle_key_navigation` (keep every existing bound arm above it exactly as-is):

```rust
fn handle_key_navigation(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('r') => Some(Action::Refresh),
        KeyCode::Char('?') => Some(Action::ToggleHelp),
        KeyCode::Char('1') => Some(Action::SwitchToDashboard),
        KeyCode::Char('2') => Some(Action::SwitchToAdapters),
        KeyCode::Char('3') => Some(Action::SwitchToSystem),
        KeyCode::Char('4') => Some(Action::SwitchToScripts),
        KeyCode::Char('5') => Some(Action::SwitchToTools),
        KeyCode::Char('6') => Some(Action::SwitchToLauncher),
        KeyCode::Char('7') => Some(Action::SwitchToJobs),
        KeyCode::Char('x') => Some(Action::CancelJob),
        KeyCode::Char('j') | KeyCode::Down => Some(Action::Down),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::Up),
        // Drill into the focused cockpit card. Inert on other screens.
        KeyCode::Char('g') => Some(Action::Drill),
        KeyCode::Enter => Some(Action::Activate),
        KeyCode::Backspace => Some(Action::Backspace),
        // `/` stays the filter trigger (on_action turns it into filter mode on a
        // filter screen); it must NOT become a CardKey or the filter breaks.
        KeyCode::Char('/') => Some(Action::InputChar('/')),
        // Every other printable char is a cockpit card hotkey. on_action only acts
        // on it on the cockpit screen (and ignores letters that label no card), so
        // this is a no-op everywhere else — never a stray InputChar.
        KeyCode::Char(c) => Some(Action::CardKey(c)),
        _ => None,
    }
}
```

(`handle_key_text` is **unchanged** — it keeps `KeyCode::Char(c) => Some(Action::InputChar(c))`, so the filter still captures every letter.)

- [ ] **Step 4: Update the two affected existing tests**

In `keymap.rs`, update `h_is_no_longer_a_help_toggle_in_either_mode`:

```rust
    #[test]
    fn h_is_no_longer_a_help_toggle_in_either_mode() {
        // `h` never opens help. In nav it's now a (harmless, unlabeled) CardKey;
        // in text it types normally. Either way it is NOT ToggleHelp.
        assert_eq!(handle_key(ch('h'), InputMode::Navigation), Some(Action::CardKey('h')));
        assert_eq!(handle_key(ch('h'), InputMode::Text), Some(Action::InputChar('h')));
        assert_ne!(handle_key(ch('h'), InputMode::Navigation), Some(Action::ToggleHelp));
    }
```

And in `ctrl_g_cancels_in_both_modes_as_the_esc_free_escape`, change the bare-`g` nav expectation (the Ctrl-G assertions stay — Ctrl-G is a global cancel, handled before mode dispatch):

```rust
        // A bare `g` is NOT cancel. In nav it's the drill key now; in text it
        // types. Only the Ctrl chord cancels.
        assert_eq!(handle_key(ch('g'), InputMode::Text), Some(Action::InputChar('g')));
        assert_eq!(handle_key(ch('g'), InputMode::Navigation), Some(Action::Drill));
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rexops-tui keymap:: 2>&1 | tail -25`
Expected: the three new keymap tests PASS and the two updated existing tests PASS; all other keymap tests (global escapes, digits, j/k, etc.) unaffected.

Run the cockpit interaction tests again to confirm the end-to-end keypress path now works through the real keymap shape:

Run: `cargo test -p rexops-tui app::tests::cockpit 2>&1 | tail -10`
Expected: still PASS (they drive `on_action` directly; this task only confirms the keymap emits the right Action).

- [ ] **Step 6: Gates green, then commit**

Run: `cargo fmt && cargo clippy -p rexops-tui -- -D warnings && cargo test -p rexops-tui --lib 2>&1 | tail -5`
Expected: all green.

```bash
git add crates/rexops-tui/src/input/keymap.rs
git commit -m "feat(rexops): nav keymap emits CardKey + Drill (Phase C)"
```

---

### Task 6: The `CockpitDetail` drill-down screen + routing + back

**Files:**
- Create: `crates/rexops-tui/src/screens/cockpit_detail.rs`
- Modify: `crates/rexops-tui/src/screens/mod.rs` (declare + re-export)
- Modify: `crates/rexops-tui/src/app/navigation.rs` (real `drill_into_selected_component` + `cockpit_back`)
- Modify: `crates/rexops-tui/src/app/update.rs` (`CockpitDetail` movement/activate/Esc routing)
- Modify: `crates/rexops-tui/src/ui/layout.rs` (route `CockpitDetail` + header name)
- Test: inline `#[cfg(test)]` in `cockpit_detail.rs`

**Interfaces:**
- Consumes: `App` (read-only), `app.selected_component`, `rexops_core::{component_by_id, ComponentStatus}`, `suite_ui::{pane, Theme}`.
- Produces:
  - `pub fn render_cockpit_detail(f: &mut Frame, app: &App, area: Rect, theme: Theme)` — the per-component detail: title (`name` + health lamp), the static registry facts (`role`, `group`, `maturity`, whether it has a launch spec), the live `ComponentStatus` (`vital`, `freshness`), and a hint line (`Enter` launches if launchable, `Esc` backs to the cockpit). Pure render. If `selected_component` is `None` or unknown, render a "no component selected" pane.
  - `App::drill_into_selected_component(&mut self)` (replaces the Task-4 stub): if a component is focused, switch `current_screen = Screen::CockpitDetail`; else no-op + log.
  - `App::cockpit_back(&mut self)`: from `CockpitDetail`, return to `Screen::Dashboard` (keeping the focus).

> Registry lookup: `rexops_core::component_by_id(id)` returns `Option<&'static Component>` with `name`, `role`, `group: ComponentGroup`, `maturity: Maturity`, `launch: Option<LaunchSpec>`. For display strings, `ComponentGroup` and `Maturity` are enums — render them via their existing `label()`/`Debug` (grep `crates/rexops-core/src/component.rs` for a `label()`/`as_str()` on `ComponentGroup`/`Maturity`; if absent, use `{:?}`). The live half (`health`, `vital`, `freshness`) comes from the `ComponentStatus` in `app.snapshot.components` (the registry row has no live state).

- [ ] **Step 1: Write the failing test**

Create `crates/rexops-tui/src/screens/cockpit_detail.rs`:

```rust
//! screens/cockpit_detail.rs — the per-component drill-down (Phase C).
//!
//! Reached by pressing Enter on a non-launchable card or `g` on any focused
//! card. Shows the one component in depth: its registry identity (role, group,
//! maturity, whether it launches) joined with its live status (health, vital,
//! freshness). Pure render over `app.selected_component`. Esc backs out to the
//! cockpit; Enter launches it if it is launchable.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use suite_ui::{pane, Theme};

use crate::app::App;
use crate::ui::cockpit_widgets::status_light::light_span;
use crate::ui::cockpit_widgets::light_state_from_health;

/// Render the detail screen for `app.selected_component`.
pub fn render_cockpit_detail(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let Some(id) = app.selected_component.as_deref() else {
        let msg = Paragraph::new(Line::from("No component selected — press 1 for the cockpit."))
            .block(pane("Detail", theme));
        f.render_widget(msg, area);
        return;
    };

    let live = app.snapshot.components.iter().find(|c| c.id == id);
    let reg = rexops_core::component_by_id(id);

    let mut lines: Vec<Line> = Vec::new();

    // Title: lamp + name.
    let (name, health) = match live {
        Some(c) => (c.name.as_str(), c.health),
        None => (id, rexops_core::AdapterHealth::Unknown),
    };
    lines.push(Line::from(vec![
        light_span(light_state_from_health(health), theme),
        Span::raw(" "),
        Span::styled(name.to_owned(), theme.title()),
    ]));

    // Registry identity.
    if let Some(r) = reg {
        lines.push(Line::from(Span::styled(
            format!("role: {}", r.role),
            theme.dim(),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "launch: {}",
                if r.launch.is_some() { "yes (Enter to run)" } else { "none (read-only)" }
            ),
            theme.dim(),
        )));
    }

    // Live status.
    if let Some(c) = live {
        let vital = c.vital.as_deref().unwrap_or("—");
        lines.push(Line::from(Span::styled(format!("vital: {vital}"), theme.dim())));
        lines.push(Line::from(Span::styled(
            format!("status: {}", c.maturity),
            theme.dim(),
        )));
    }

    lines.push(Line::from(Span::styled(
        "Enter launches (if launchable) · Esc back to cockpit",
        theme.dim(),
    )));

    f.render_widget(Paragraph::new(lines).block(pane("Component", theme)), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    use rexops_core::{AdapterHealth, AppConfig, ComponentStatus, OpsSnapshot};
    use std::sync::mpsc;

    fn app_focused_on(id: &str) -> App {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default(), None);
        let mut snap = OpsSnapshot::new();
        snap.push_component(ComponentStatus {
            id: "bulwark".into(),
            name: "Bulwark".into(),
            group: "field tool".into(),
            maturity: "live".into(),
            health: AdapterHealth::Healthy,
            freshness: None,
            vital: Some("1 crit 1 high".into()),
            launchable: true,
        });
        app.apply_snapshot(snap);
        app.selected_component = Some(id.to_owned());
        app
    }

    fn render(app: &App) -> String {
        let backend = TestBackend::new(60, 12);
        let mut terminal = Terminal::new(backend).expect("backend");
        let theme = Theme::with_color(false);
        terminal.draw(|f| render_cockpit_detail(f, app, f.area(), theme)).unwrap();
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
    fn detail_shows_identity_and_live_vital() {
        let text = render(&app_focused_on("bulwark"));
        assert!(text.contains("Bulwark"), "name:\n{text}");
        assert!(text.contains("security"), "registry role:\n{text}");
        assert!(text.contains("1 crit 1 high"), "live vital:\n{text}");
        assert!(text.contains("Esc"), "back hint:\n{text}");
    }

    #[test]
    fn detail_without_a_selection_guides_the_user() {
        let (tx, _rx) = mpsc::channel();
        let app = App::new(tx, AppConfig::default(), None); // nothing selected
        let text = render(&app);
        assert!(
            text.to_lowercase().contains("no component selected"),
            "empty detail guides instead of blank:\n{text}"
        );
    }
}

// Learning Notes
// - The detail JOINS two sources: the static registry row (component_by_id) for
//   identity that never changes (role, whether it launches) and the live
//   ComponentStatus for state (health, vital). Neither alone is the full story.
// - It reads only app.selected_component — the same id the cockpit focus uses —
//   so "drill into the focused card" needs no extra plumbing than the screen swap.
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rexops-tui cockpit_detail:: 2>&1 | tail -20`
Expected: FAIL — module not declared / `render_cockpit_detail` not found.

- [ ] **Step 3: Wire the screen + implement drill/back**

In `crates/rexops-tui/src/screens/mod.rs`: add `pub mod cockpit_detail;` and `pub use cockpit_detail::render_cockpit_detail;`.

In `crates/rexops-tui/src/app/navigation.rs`, **replace** the Task-4 stub with the real drill + add `cockpit_back`:

```rust
    /// Drill into the focused cockpit card's detail. No-op (logged) if nothing
    /// is focused.
    pub(crate) fn drill_into_selected_component(&mut self) {
        if self.selected_component.is_some() {
            self.current_screen = Screen::CockpitDetail;
            self.log_event("Cockpit: opened component detail (Esc to go back)");
        } else {
            self.log_event("Cockpit: no card focused to open");
        }
    }

    /// Back out of the detail screen to the cockpit, keeping the focus.
    pub(crate) fn cockpit_back(&mut self) {
        self.current_screen = Screen::Dashboard;
        self.log_event("Detail: back to cockpit");
    }
```

In `crates/rexops-tui/src/ui/layout.rs`: add the route + header name:

```rust
        Screen::CockpitDetail => screens::render_cockpit_detail(f, app, chunks[1], theme),
```

and in `render_header`'s `screen_name` match:

```rust
        Screen::CockpitDetail => "Component",
```

- [ ] **Step 4: Route detail-screen actions in `on_action`**

In `crates/rexops-tui/src/app/update.rs`:

(a) `Esc` must back out of the detail. In `cancel_current_context`, add a branch BEFORE the top-level no-op (after the Launcher branch):

```rust
        } else if self.current_screen == Screen::CockpitDetail {
            self.cockpit_back();
```

(b) On the detail screen, `Enter` launches the (already-focused) component if launchable. Extend `activate_selection`'s match with a `CockpitDetail` arm that reuses the cockpit arm:

```rust
            // On the detail screen, Enter launches the focused component if it is
            // launchable (same arm path as the cockpit). A read-only component's
            // Enter is a no-op here — there's nothing deeper to drill into.
            Screen::CockpitDetail => {
                self.arm_selected_component();
            }
```

(`arm_selected_component` → `arm_tool` already refuses a non-launchable id, logging "disabled", so no extra guard is needed.)

(c) `j`/`k` on the detail screen: leave inert (the `_ => {}` arm in `move_selection` already covers `CockpitDetail`). No edit needed — note it in the report.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rexops-tui cockpit_detail:: 2>&1 | tail -20`
Expected: both detail render tests PASS.

Add one interaction test to `crates/rexops-tui/src/app/tests/cockpit.rs` proving the drill/back round-trip through `on_action`:

```rust
#[test]
fn drill_and_back_round_trip() {
    let mut app = cockpit_app();
    let mut r = runner();
    // Focus Workstate (not launchable) → Enter drills into detail.
    app.on_action(Action::Activate, &mut r);
    assert_eq!(app.current_screen, Screen::CockpitDetail);
    // Esc backs out to the cockpit, focus preserved.
    app.on_action(Action::Cancel, &mut r);
    assert_eq!(app.current_screen, Screen::Dashboard);
    assert_eq!(app.selected_component.as_deref(), Some("workstate"));
}

#[test]
fn drill_key_opens_detail_for_a_launchable_card_too() {
    let mut app = cockpit_app();
    let mut r = runner();
    app.on_action(Action::Down, &mut r); // focus Bulwark (launchable)
    app.on_action(Action::Drill, &mut r); // `g` drills even though it's launchable
    assert_eq!(app.current_screen, Screen::CockpitDetail);
    assert_eq!(app.selected_component.as_deref(), Some("bulwark"));
}
```

Run: `cargo test -p rexops-tui app::tests::cockpit 2>&1 | tail -12`
Expected: all cockpit interaction tests (now nine) PASS.

- [ ] **Step 6: Full workspace gate, then commit**

Run: `cargo fmt && cargo clippy --workspace -- -D warnings && cargo test --workspace 2>&1 | tail -8`
Expected: all green across the workspace.

```bash
git add crates/rexops-tui/src/screens/cockpit_detail.rs crates/rexops-tui/src/screens/mod.rs crates/rexops-tui/src/app/navigation.rs crates/rexops-tui/src/ui/layout.rs crates/rexops-tui/src/app/update.rs crates/rexops-tui/src/app/tests/cockpit.rs
git commit -m "feat(rexops): cockpit drill-down detail screen + Esc/Enter routing (Phase C)"
```

---

### Task 7: Surface the new keys — status-bar hints + help sheet

**Files:**
- Modify: `crates/rexops-tui/src/ui/status_bar.rs` (cockpit hint row + a `CockpitDetail` row)
- Modify: `crates/rexops-tui/src/ui/palette.rs` (help sheet rows for the cockpit letters + drill)
- Test: the existing `every_screen_has_non_empty_hints` test (status_bar) + the help-sheet test (palette) already guard these; extend the help test to assert the new rows.

**Interfaces:** none (copy/UX only). The hint tuples are `(&'static str, &'static str)`.

- [ ] **Step 1: Update the cockpit hint row + add the detail row**

In `crates/rexops-tui/src/ui/status_bar.rs`, change the `Screen::Dashboard` hint row to advertise the card actuation + drill (keep it within the footer width — drop nothing essential; the cockpit isn't a `j/k`-scroll-a-list screen, it's a card grid):

```rust
        Screen::Dashboard => &[
            ("a-z", "launch card"),
            ("g", "detail"),
            ("/", "filter"),
            ("r", "refresh"),
            ("?", "help"),
            ("1-7", "screens"),
        ],
```

And add a `CockpitDetail` arm to the `match` (place it beside `Screen::Launcher`):

```rust
        Screen::CockpitDetail => &[
            ("Enter", "launch"),
            ("Esc", "back"),
            ("r", "refresh"),
            ("1", "cockpit"),
        ],
```

- [ ] **Step 2: Run the hints guard test**

Run: `cargo test -p rexops-tui ui::tests::every_screen_has_non_empty_hints 2>&1 | tail -10`
Expected: PASS (every `Screen` variant — now including `CockpitDetail` — returns a non-empty hint slice; a missing arm would fail to compile due to the exhaustive match).

- [ ] **Step 3: Update the help sheet**

In `crates/rexops-tui/src/ui/palette.rs`, add rows to `render_help_popup`'s `rows` array (insert near the navigation rows):

```rust
        ("a-z (cockpit)", "press a card's letter to launch it (confirm first)"),
        ("g", "drill into the focused cockpit card's detail"),
```

And update the `Enter` row's copy to mention the cockpit (optional but accurate):

```rust
        ("Enter", "activate selection / launch a launchable card / run enabled tools"),
```

- [ ] **Step 4: Extend the help-sheet test**

In `palette.rs`'s test module, add an assertion to the existing help test (or a focused new one):

```rust
    #[test]
    fn help_documents_the_cockpit_card_hotkeys() {
        let text = help_text();
        assert!(text.contains("card"), "help mentions card hotkeys:\n{text}");
        assert!(text.to_lowercase().contains("drill"), "help mentions drill:\n{text}");
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p rexops-tui ui:: 2>&1 | tail -10`
Expected: the hints test + the help tests PASS.

- [ ] **Step 6: Gates green, then commit**

Run: `cargo fmt && cargo clippy -p rexops-tui -- -D warnings && cargo test -p rexops-tui --lib 2>&1 | tail -5`
Expected: all green.

```bash
git add crates/rexops-tui/src/ui/status_bar.rs crates/rexops-tui/src/ui/palette.rs
git commit -m "feat(rexops): document cockpit card hotkeys + drill in hints/help (Phase C)"
```

---

### Task 8: Manual smoke + docs + final workspace gate

**Files:**
- Modify (optional): `docs/TUI_DESIGN.md` (one paragraph: the cockpit is now interactive), `LAST_WORK.md` at the suite root (per project rule).

**Interfaces:** none (verification + docs).

- [ ] **Step 1: Manual smoke test of the interactive cockpit**

Pipe the Workstate fixture so cards populate, launch the TUI, and exercise the new interactions:

Run: `cat crates/rexops-adapters/fixtures/workstate/snapshot_v3.json | cargo run -q -p rexops-cli 2>/dev/null`

Confirm, on screen 1 (Cockpit):
- Each card shows a dim `[letter]` marker; letters run `a, s, d, …` down the grid (skipping `q/r/x/j/k/h/y/n/g` and digits).
- `j`/`k` (and ↑/↓) move a visible focus rail between cards.
- Pressing a **launchable** card's letter (e.g. Bulwark's) opens the `⚠ Confirm` popup; `n`/`Esc` cancels, `y`/`Enter` launches.
- Pressing a **planned** card's letter (e.g. Pulse's) logs "disabled" (no popup) — visible in the activity log, no silent no-op.
- `Enter` on the focused card: launches if launchable, else opens the detail screen.
- `g` opens the detail screen for the focused card (launchable or not); `Esc` returns to the cockpit with focus preserved.
- `/` still filters; while filtering, letters type into the filter (no card fires).
- `1`–`7` still switch screens; `q` quits; `?` shows help (now listing the card hotkeys + drill).

> If interactive runs are impractical in this environment, the off-screen render + `on_action` tests already cover the shape; at minimum confirm `cargo run -q -p rexops-cli` builds and the suite of `app::tests::cockpit` + `cockpit::`/`cockpit_detail::`/`cockpit_nav::` tests pass. Paste a short description into the Task 8 report.

- [ ] **Step 2: Update docs**

Add a short paragraph to `docs/TUI_DESIGN.md` noting the cockpit is interactive in Phase C: card focus (`j`/`k`), per-card letter launch through the confirm gate, and `g`/Enter drill-down to `CockpitDetail`. Update `LAST_WORK.md` at the suite root (`/home/tom/projects/linux-ops-suite/LAST_WORK.md`) per the project rule before declaring the phase complete.

- [ ] **Step 3: Final workspace gate**

Run: `cargo fmt && cargo clippy --workspace -- -D warnings && cargo test --workspace 2>&1 | tail -8`
Expected: all green across the workspace. Lib-test count for `rexops-tui` is up by the Phase C additions (≈ +20 over the 139 baseline).

```bash
git add docs/TUI_DESIGN.md /home/tom/projects/linux-ops-suite/LAST_WORK.md
git commit -m "docs(rexops): cockpit interactivity notes + LAST_WORK (Phase C)"
```

---

## Self-Review

**1. Spec coverage (against the Phase C handoff's four requirements):**
- **#1 Card selection + hotkeys without conflicting with `1`–`7`** → Task 1 (curated `MARKER_ALPHABET` disjoint from every bound nav key + digits, asserted), Task 2 (markers drawn on cards), Task 3 (focus + markers rendered from `selected_component`), Task 4 (focus movement + marker arming), Task 5 (keymap emits `CardKey`, keeps digits/`q`/`r`/… global, keeps `/` as the filter trigger). ✓
- **#2 One-keypress launch through the existing confirm gate** → Task 4's `arm_component_by_marker` → existing `App::arm_tool` → existing `pending_action` confirm popup. No new launch/confirm path. ✓
- **#3 Drill-down (Enter on a card opens its detail screen)** → Task 6 `CockpitDetail` screen; Enter on a non-launchable focused card drills (Task 4 `activate_selection`), `g` drills any focused card (Task 5/6), `Esc` backs out (Task 6). ✓ (Design choice, surfaced to and approved by the user: Enter on a *launchable* card *launches* rather than drills, since one-key launch is requirement #2; `g` is the universal drill key so every card — launchable or not — can still be inspected.)
- **#4 Keep all existing Phase B rendering intact** → Global Constraint "Phase B rendering is frozen"; all three `screens/cockpit.rs` Phase B tests asserted to still pass in Tasks 1 & 3; `StatusCard` changes are additive (`marker: None, focused: false` reproduces Phase B output, asserted in Task 2's `unfocused_card_without_marker_is_unchanged_from_phase_b`). ✓

**2. Placeholder scan:** No `TBD`/`TODO`/"handle edge cases". Every code step shows complete code. The one deliberate stub (`drill_into_selected_component` in Task 4) is explicitly called out as a stub that Task 6 replaces, with the reason (keep the crate building so Task 4's arm tests run) — and Task 6 Step 3 replaces it with the real body. The registry `label()`-vs-`{:?}` note in Task 6 is a concrete grep instruction with a named fallback, not a placeholder.

**3. Type consistency:**
- `selected_component: Option<String>` — defined in Task 3 (state.rs), read in Task 3 (cockpit.rs render), mutated in Task 4 (navigation.rs), reconciled in Task 4 (apply_snapshot). Same name throughout. ✓
- `GROUP_ORDER` — single definition in Task 1 (`cockpit_nav.rs`), imported by `cockpit.rs` (Task 1/3). No duplicate. ✓
- `cockpit_visit_order` / `marker_for` / `component_for_marker` (Task 1) — used by `cockpit.rs` (Task 3) and `navigation.rs` (Task 4). Signatures match (`&[ComponentStatus]` in, ids/markers out). ✓
- `CardInput` gains `marker: Option<char>` + `focused: bool` (Task 2) — constructed with those exact fields in `cockpit.rs` (Task 3) and the widget tests (Task 2). ✓
- `Action::CardKey(char)` + `Action::Drill` — defined in Task 4 (action.rs), emitted in Task 5 (keymap.rs), handled in Task 4 (update.rs). ✓ (Defined in Task 4, not Task 5, to avoid a forward-reference — noted in Task 4 Step 2.)
- `arm_tool(id: String, name: String)` — the existing function (commands/dispatch.rs), called by `arm_selected_component` (Task 4) with owned `String`s from the live `ComponentStatus`. ✓
- `Screen::CockpitDetail` — added in Task 4 (navigation.rs), routed in Task 6 (layout.rs), given hints in Task 7 (status_bar.rs). The status-bar match is exhaustive, so a missing arm fails the build — caught at Task 6/7. ✓
- `drill_into_selected_component` / `cockpit_back` — stub in Task 4, real in Task 6; `cockpit_back` defined + called (Esc) in Task 6. ✓

**4. Reuse check (KISS / no over-engineering):** No new launch engine, confirm modal, or process plumbing — the card path funnels into the existing `arm_tool`/`pending_action`/`confirm_pending`. The detail screen is a single pure render fn joining the existing registry table + live status — no new data model. Selection mirrors the existing `selected_adapter`/`move_adapter_selection` pattern rather than inventing a new one. The marker alphabet is a `const` array, not a config or plugin. Total new files: 3 (`cockpit_nav.rs`, `cockpit_detail.rs`, `app/tests/cockpit.rs`); the rest are additive edits, every file under 300 LOC.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-06-21-rexops-cockpit-phase-c-interactive-cockpit.md`.
