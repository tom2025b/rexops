//! launchpad.rs — The Launcher screen (6th screen).
//!
//! Lists the available specialist tools with a short description and lets the
//! user pick one (↑/↓) and launch enabled entries (Enter). Launch orchestration
//! and catalog metadata live in `crate::tools`; this screen only renders the
//! launcher view.
//!
//! The catalog is a small static list. Not every entry is launchable; sections
//! sourced from Workstate have no executable, so the activation path treats
//! unresolved commands as disabled instead of opening the confirmation modal.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

use suite_ui::{pane, Theme};

use crate::app::App;
use crate::tools::{Category, ToolEntry, CATALOG};
use crate::ui::widgets;

/// Width the tool name is padded to so the badges and tags line up into columns.
/// Sized to the longest catalog names ("ScriptVault"/"ToolFoundry" at 11), so 12
/// leaves a single space of gutter before the badge and nothing renders cramped.
const NAME_COL: usize = 12;

/// Render the Launcher screen.
pub fn render_launcher(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(5),    // tool list
            Constraint::Length(4), // selected-tool detail
        ])
        .split(area);

    render_launcher_header(f, chunks[0], theme);
    render_launcher_list(f, app, chunks[1], theme);
    render_launcher_detail(f, app, chunks[2], theme);
}

fn render_launcher_header(f: &mut Frame, area: Rect, theme: Theme) {
    let line = Line::from(Span::styled(
        "Pick a tool with ↑/↓; Enter confirms enabled tools.",
        theme.dim(),
    ));
    let header = Paragraph::new(line).block(pane("Launcher", theme));
    f.render_widget(header, area);
}

/// Render a single launcher row: an accent selection rail + row tint on the
/// selected row, the name padded into a column, the health badge, and a dim
/// run-mode / availability tag so the user can see at a glance what Enter does.
fn render_launcher_row(app: &App, index: usize, tool: &ToolEntry, theme: Theme) -> Line<'static> {
    let selected = index == app.selected_tool;

    // The suite's selection look: an accent rail glyph on the selected row, a
    // plain gutter otherwise. The rail keeps its accent because `selection()`
    // sets no foreground (see Theme::selection docs).
    let rail = if selected {
        Span::styled("▌ ", theme.selected_rail())
    } else {
        Span::raw("  ")
    };

    let health = app
        .snapshot
        .adapter_health
        .get(tool.id)
        .copied()
        .unwrap_or(rexops_core::AdapterHealth::Unknown);
    let badge = widgets::render_health_badge(health, theme);

    // Run-mode + availability tag. Resolvability is read from the cache (computed
    // once from config + PATH) so this hot render path — redrawn every ~100ms —
    // never shells out to `which`; live health is folded in via a cheap snapshot
    // lookup. The three states never contradict the health badge beside them:
    //   - resolvable AND health != Unavailable → streams / interactive
    //   - resolvable BUT health == Unavailable → unavailable (badge agrees)
    //   - not resolvable at all                → disabled
    // The 3-state tag is computed by App::availability_tag — the single source
    // of truth shared with the command palette, so the two run surfaces can
    // never disagree. We frame it with the leading "· " the rows use.
    let tag = format!("· {}", app.availability_tag(tool.id));

    let name = format!("{:<width$}", tool.name, width = NAME_COL);
    let name_span = if selected {
        Span::styled(name, theme.selection())
    } else {
        Span::styled(name, theme.title())
    };

    Line::from(vec![
        rail,
        name_span,
        Span::raw(" "),
        badge,
        Span::raw("  "),
        Span::styled(tag, theme.dim()),
    ])
}

/// A dim, non-selectable section header introducing a category's rows.
fn render_category_header(title: &str, theme: Theme) -> Line<'static> {
    Line::from(Span::styled(title.to_owned(), theme.dim()))
}

/// Render the grouped tool list: walk `Category::ORDER`, and for each category
/// that has at least one tool emit a header followed by that category's rows.
///
/// Rows keep their ORIGINAL catalog index, so `selected_tool` stays a flat index
/// over `CATALOG` (headers are display-only and never selectable) — navigation
/// and selection are unchanged by the grouping; only the rendered output gains
/// headers. Empty categories are skipped (no orphan header).
fn render_launcher_list(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let mut lines: Vec<Line> = Vec::new();

    for category in Category::ORDER {
        let mut rows: Vec<Line> = CATALOG
            .iter()
            .enumerate()
            .filter(|(_, tool)| tool.category == category)
            .map(|(i, tool)| render_launcher_row(app, i, tool, theme))
            .collect();

        if rows.is_empty() {
            continue;
        }
        lines.push(render_category_header(category.title(), theme));
        lines.append(&mut rows);
    }

    let list = Paragraph::new(lines).block(pane("Tools", theme));
    f.render_widget(list, area);
}

/// The detail pane: the full description of the currently selected tool (so a
/// long one is never clipped in its row), plus whether the row can be activated.
fn render_launcher_detail(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let mut lines: Vec<Line> = Vec::new();

    if let Some(tool) = CATALOG.get(app.selected_tool) {
        lines.push(Line::from(vec![
            Span::styled(format!("{}: ", tool.name), theme.title()),
            Span::raw(tool.description.to_string()),
        ]));
        let availability = if app.is_tool_available(tool.id) {
            "Enabled: Enter opens a confirmation before launch."
        } else if app.is_tool_launchable(tool.id) {
            "Unavailable: the adapter probe reports this tool is down."
        } else {
            "Disabled: no launch command is configured."
        };
        lines.push(Line::from(Span::styled(availability, theme.dim())));
    }

    let detail = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .block(pane("Detail", theme));
    f.render_widget(detail, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rexops_core::{AdapterConfig, AdapterHealth, AppConfig};
    use std::sync::mpsc;

    /// Render the Launcher into an off-screen buffer and flatten it to text, so a
    /// test can assert on what actually appears (glyphs + tags). Mirrors the
    /// suite-ui gallery's buffer-to-string approach.
    fn render_to_text(app: &App) -> String {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).expect("test backend");
        let theme = Theme::with_color(true);
        terminal
            .draw(|f| render_launcher(f, app, f.area(), theme))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let width = buffer.area.width as usize;
        let mut out = String::new();
        for (i, cell) in buffer.content.iter().enumerate() {
            if i % width == 0 && i != 0 {
                out.push('\n');
            }
            out.push_str(cell.symbol());
        }
        out
    }

    /// A fresh App with the given selected tool index (no probed snapshot, so
    /// every tool reads as Unknown health — fine for layout/tag assertions).
    fn app_with_selection(selected: usize) -> App {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default(), None);
        app.selected_tool = selected;
        app
    }

    #[test]
    fn selected_row_shows_the_accent_rail_glyph() {
        let app = app_with_selection(0);
        let text = render_to_text(&app);
        // The first catalog row is selected → the rail glyph precedes its name.
        assert!(
            text.contains("▌ Bulwark"),
            "selected row must show the rail:\n{text}"
        );
        // A non-selected row has no rail before its name.
        assert!(
            !text.contains("▌ Proto"),
            "non-selected rows have no rail:\n{text}"
        );
    }

    #[test]
    fn rows_carry_a_run_mode_or_install_tag() {
        // With a default config and no `which` hits in the test environment,
        // tools resolve to no command → "disabled". The tag column is what
        // we assert is present (the polish that tells the user what Enter does).
        let app = app_with_selection(0);
        let text = render_to_text(&app);
        assert!(
            text.contains("disabled") || text.contains("interactive") || text.contains("streams"),
            "every row should carry a run-mode/install tag:\n{text}"
        );
    }

    #[test]
    fn detail_pane_echoes_the_selected_tools_description() {
        // Select "Proto" (catalog index 2: Bulwark, ScriptVault, Proto) and
        // confirm its full description shows in the detail pane, not just the row.
        let app = app_with_selection(2);
        let text = render_to_text(&app);
        assert!(
            text.contains("Proto:"),
            "detail names the selected tool:\n{text}"
        );
        assert!(
            text.contains("protocol") || text.contains("checklist"),
            "detail shows the selected tool's description:\n{text}"
        );
    }

    #[test]
    fn detail_pane_marks_unresolved_tools_disabled() {
        // Proto (catalog index 2) with no resolvable command is the "disabled"
        // state. Force the cache false so the test doesn't depend on a `proto`
        // binary being absent from the dev PATH.
        let mut app = app_with_selection(2);
        app.set_tool_launchable("proto", false);
        let text = render_to_text(&app);
        assert!(
            text.contains("Proto:"),
            "detail names the selected disabled tool:\n{text}"
        );
        assert!(
            text.contains("Disabled: no launch command is configured."),
            "detail explains that Enter is inert for disabled rows:\n{text}"
        );
    }

    #[test]
    fn catalog_includes_proto_as_launchable() {
        // Proto is a real PATH binary (installed via `cargo install --path .`),
        // so RexOps's `which proto` resolves it. It must be in the catalog to be
        // offered on the Launcher screen.
        let proto = CATALOG.iter().find(|t| t.id == "proto");
        let proto = proto.expect("Proto must be registered in the launcher catalog");
        assert_eq!(proto.name, "Proto");
        assert!(!proto.description.is_empty());
    }

    /// Extract the single rendered row line that names the given tool, so a test
    /// can assert on that row's tag without other rows' tags leaking in.
    fn row_line(text: &str, tool_name: &str) -> String {
        text.lines()
            .find(|line| line.contains(tool_name))
            .unwrap_or_else(|| panic!("no rendered row for {tool_name}:\n{text}"))
            .to_owned()
    }

    #[test]
    fn render_reads_cached_availability_not_a_live_resolve() {
        // The Launcher redraws every ~100ms. Availability must come from a cache
        // computed once — never a per-render `resolve_launch_command` (which
        // shells out to `which`). We prove render reads the cache by overriding a
        // single tool's cached availability and asserting its row reflects the
        // cache, not a live resolve. Asserting on the specific row avoids other
        // rows' tags (or a real PATH hit elsewhere) leaking into the check.
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default(), None);

        // Bulwark is Foreground: launchable → "interactive", otherwise "disabled".
        // Seed the cache to launchable; with default config + no PATH binary a
        // live resolve would say "disabled", so the tag proves the source.
        app.set_tool_launchable("bulwark", true);
        let bulwark_row = row_line(&render_to_text(&app), "Bulwark");
        assert!(
            bulwark_row.contains("interactive"),
            "cached launchable=true must render interactive:\n{bulwark_row}"
        );
        assert!(
            !bulwark_row.contains("disabled"),
            "cached launchable=true must not render disabled:\n{bulwark_row}"
        );

        // Flip the cache to not-launchable: the same row must now read disabled,
        // independent of whether `which bulwark` would hit on this machine.
        app.set_tool_launchable("bulwark", false);
        let bulwark_row = row_line(&render_to_text(&app), "Bulwark");
        assert!(
            bulwark_row.contains("disabled"),
            "cached launchable=false must render disabled:\n{bulwark_row}"
        );
    }

    #[test]
    fn render_tag_folds_in_live_health_not_just_resolvability() {
        // A resolvable tool whose adapter probe says Unavailable must NOT render
        // as launchable ("interactive"/"streams") — the tag would then contradict
        // the red health badge beside it. It must read "unavailable". Flipping
        // health back to Healthy restores the launchable tag. Resolvability is held
        // constant (cache=true) so this isolates the HEALTH contribution.
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default(), None);
        app.set_tool_launchable("bulwark", true);

        let bulwark = rexops_core::AdapterId::new("bulwark").expect("test adapter id");
        app.snapshot
            .set_adapter_health(&bulwark, AdapterHealth::Unavailable);
        let row = row_line(&render_to_text(&app), "Bulwark");
        assert!(
            row.contains("unavailable"),
            "resolvable + Unavailable health must render unavailable:\n{row}"
        );
        assert!(
            !row.contains("interactive"),
            "an Unavailable tool must not be tagged launchable:\n{row}"
        );

        // Healthy → launchable again (proves the tag tracks live health).
        app.snapshot
            .set_adapter_health(&bulwark, AdapterHealth::Healthy);
        let row = row_line(&render_to_text(&app), "Bulwark");
        assert!(
            row.contains("interactive"),
            "resolvable + Healthy must render interactive:\n{row}"
        );

        // Degraded and Unknown stay launchable on purpose (run-to-diagnose /
        // pre-probe), so they must NOT read unavailable.
        for h in [AdapterHealth::Degraded, AdapterHealth::Unknown] {
            app.snapshot.set_adapter_health(&bulwark, h);
            let row = row_line(&render_to_text(&app), "Bulwark");
            assert!(
                row.contains("interactive"),
                "{h:?} must stay launchable, not unavailable:\n{row}"
            );
        }
    }

    #[test]
    fn modify_config_keeps_availability_cache_coherent() {
        // The coherence invariant: changing config through the only mutation
        // path (`modify_config`) must refresh the availability cache, with NO
        // manual refresh call. We pin a fake id that is never on PATH, so the
        // only way `scripts` becomes launchable is the config-binary fallback —
        // and the only way the *rendered* row reflects that is if modify_config
        // refreshed the cache. No `refresh_launch_availability` appears here by
        // design: if the cache could go stale, this test would fail.
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default(), None);

        // proto is Background → launchable renders "streams", else "disabled".
        // Pin a fake off-PATH id so config is the ONLY thing that can make it
        // launchable; force the starting cache to false so the test doesn't
        // depend on a real `proto` binary on the dev PATH.
        app.set_tool_launchable("proto", false);
        let row = row_line(&render_to_text(&app), "Proto");
        assert!(
            row.contains("disabled"),
            "proto must start disabled here:\n{row}"
        );

        // Pin a config binary through the mutation path. No manual refresh.
        app.modify_config(|cfg| {
            cfg.adapters.insert(
                "proto".to_owned(),
                AdapterConfig {
                    enabled: true,
                    binary: Some("/tmp/proto".to_owned()),
                    timeout_secs: None,
                },
            );
        });
        let row = row_line(&render_to_text(&app), "Proto");
        assert!(
            row.contains("streams"),
            "modify_config must refresh the cache → proto now launchable:\n{row}"
        );

        // And disabling it again through the same path must flip it back.
        app.modify_config(|cfg| {
            cfg.adapters.get_mut("proto").unwrap().enabled = false;
        });
        let row = row_line(&render_to_text(&app), "Proto");
        assert!(
            row.contains("disabled"),
            "modify_config must refresh the cache → disabled adapter unlaunchable:\n{row}"
        );
    }

    #[test]
    fn catalog_ids_are_unique() {
        // Ids key both the `which` lookup and the config-binary fallback, so a
        // duplicate id would make two rows resolve to the same command.
        let mut ids: Vec<&str> = CATALOG.iter().map(|t| t.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "catalog tool ids must be unique");
    }

    /// A ForegroundRunner that does nothing — the detail-pane navigation test
    /// never launches, it only moves the selection.
    struct NoopRunner;
    impl crate::tools::ForegroundRunner for NoopRunner {
        fn run_foreground(
            &mut self,
            _command: &crate::tools::LaunchCommand,
        ) -> std::io::Result<crate::tools::ChildExit> {
            Ok(crate::tools::ChildExit::Success)
        }
    }

    #[test]
    fn detail_pane_follows_the_selection_as_it_moves() {
        use crate::app::Screen;
        use crate::input::Action;

        // The detail pane is derived purely from `selected_tool`, so moving the
        // selection must swap which tool the pane describes. Drive the REAL
        // navigation path (`Action::Down` through `on_action` on the Launcher
        // screen) rather than poking the index, so this guards the whole
        // keypress → selection → re-render chain, not just the render fn.
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default(), None);
        app.current_screen = Screen::Launcher;
        let mut runner = NoopRunner;

        // Selection starts at 0 → Bulwark. The detail pane names it and shows its
        // description, and does NOT yet name the next tool.
        let text = render_to_text(&app);
        assert!(text.contains("Bulwark:"), "starts on Bulwark:\n{text}");
        assert!(
            !text.contains("ScriptVault:"),
            "ScriptVault's detail must not show while Bulwark is selected:\n{text}"
        );

        // Down → ScriptVault (catalog index 1). The pane must follow, dropping
        // Bulwark's detail line.
        app.on_action(Action::Down, &mut runner);
        let text = render_to_text(&app);
        assert!(
            text.contains("ScriptVault:") && text.contains("search engine"),
            "detail must follow the selection to ScriptVault:\n{text}"
        );
        assert!(
            !text.contains("Bulwark:"),
            "Bulwark's detail must no longer show after moving off it:\n{text}"
        );

        // Down again → Proto (index 2), proving tracking continues past the first
        // step within the Scripts category.
        app.on_action(Action::Down, &mut runner);
        let text = render_to_text(&app);
        assert!(
            text.contains("Proto:") && text.contains("checklist"),
            "detail must follow the selection to Proto:\n{text}"
        );

        // Three more Downs walk Workstate (3) → ToolFoundry (4) → wrap to Bulwark
        // (0). Confirms the wrap still lands on the first entry with 5 tools.
        for _ in 0..3 {
            app.on_action(Action::Down, &mut runner);
        }
        let text = render_to_text(&app);
        assert!(
            text.contains("Bulwark:"),
            "detail must follow the wrap back to Bulwark:\n{text}"
        );
        assert!(
            !text.contains("Proto:"),
            "Proto's detail must clear once moved past:\n{text}"
        );
    }

    #[test]
    fn list_groups_rows_under_category_headers() {
        // The grouped list emits a header per non-empty category in ORDER, with
        // each tool's row beneath its category. Headers are display-only; this
        // asserts they appear and that a tool lands under the right one.
        let app = app_with_selection(0);
        let text = render_to_text(&app);

        for header in ["Scripts", "System", "Inventory"] {
            assert!(
                text.contains(header),
                "expected a `{header}` category header:\n{text}"
            );
        }

        let lines: Vec<&str> = text.lines().collect();
        let pos = |needle: &str| {
            lines
                .iter()
                .position(|l| l.contains(needle))
                .unwrap_or_else(|| panic!("`{needle}` not rendered:\n{text}"))
        };

        // Scripts header precedes Bulwark/ScriptVault/Proto; System precedes
        // Workstate; Inventory precedes ToolFoundry — and the section order holds.
        assert!(
            pos("Scripts") < pos("Bulwark"),
            "Bulwark under Scripts:\n{text}"
        );
        assert!(
            pos("Scripts") < pos("ScriptVault"),
            "ScriptVault under Scripts:\n{text}"
        );
        assert!(
            pos("System") < pos("Workstate"),
            "Workstate under System:\n{text}"
        );
        assert!(
            pos("Inventory") < pos("ToolFoundry"),
            "ToolFoundry under Inventory:\n{text}"
        );
        assert!(
            pos("Scripts") < pos("System") && pos("System") < pos("Inventory"),
            "category order must be Scripts, System, Inventory:\n{text}"
        );
    }
}
