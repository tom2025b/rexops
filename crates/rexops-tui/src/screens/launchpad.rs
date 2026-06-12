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
use crate::tools::{RunMode, ToolEntry, CATALOG};
use crate::ui::widgets;

/// Width the tool name is padded to so the badges and tags line up into columns.
/// The catalog names are short ("Workstate" is the longest at 9), so 10 leaves a
/// single space of gutter before the badge.
const NAME_COL: usize = 10;

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
    let tag = if app.is_tool_available(tool.id) {
        match tool.run_mode {
            RunMode::Background => "· streams".to_string(),
            RunMode::Foreground => "· interactive".to_string(),
        }
    } else if app.is_tool_launchable(tool.id) {
        // Command resolves, but the adapter probe says it's down: don't invite a
        // launch the suite just reported as unavailable.
        "· unavailable".to_string()
    } else {
        "· disabled".to_string()
    };

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

fn render_launcher_list(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let lines: Vec<Line> = CATALOG
        .iter()
        .enumerate()
        .map(|(i, tool)| render_launcher_row(app, i, tool, theme))
        .collect();

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
        let mut app = App::new(tx, AppConfig::default());
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
        // Select "Proto" (index 1) and confirm its full description shows in the
        // detail pane, not just the row.
        let app = app_with_selection(1);
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
        let app = app_with_selection(2);
        let text = render_to_text(&app);
        assert!(
            text.contains("Scripts:"),
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
        let mut app = App::new(tx, AppConfig::default());

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
        let mut app = App::new(tx, AppConfig::default());
        app.set_tool_launchable("bulwark", true);

        app.snapshot
            .adapter_health
            .insert("bulwark".to_owned(), AdapterHealth::Unavailable);
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
            .adapter_health
            .insert("bulwark".to_owned(), AdapterHealth::Healthy);
        let row = row_line(&render_to_text(&app), "Bulwark");
        assert!(
            row.contains("interactive"),
            "resolvable + Healthy must render interactive:\n{row}"
        );

        // Degraded and Unknown stay launchable on purpose (run-to-diagnose /
        // pre-probe), so they must NOT read unavailable.
        for h in [AdapterHealth::Degraded, AdapterHealth::Unknown] {
            app.snapshot.adapter_health.insert("bulwark".to_owned(), h);
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
        let mut app = App::new(tx, AppConfig::default());

        // scripts is Background → launchable renders "streams", else "disabled".
        // Default config + no PATH binary → starts disabled.
        let row = row_line(&render_to_text(&app), "Scripts");
        assert!(
            row.contains("disabled"),
            "scripts must start disabled under default config:\n{row}"
        );

        // Pin a config binary through the mutation path. No manual refresh.
        app.modify_config(|cfg| {
            cfg.adapters.insert(
                "scripts".to_owned(),
                AdapterConfig {
                    enabled: true,
                    binary: Some("/tmp/scripts".to_owned()),
                    timeout_secs: None,
                },
            );
        });
        let row = row_line(&render_to_text(&app), "Scripts");
        assert!(
            row.contains("streams"),
            "modify_config must refresh the cache → scripts now launchable:\n{row}"
        );

        // And disabling it again through the same path must flip it back.
        app.modify_config(|cfg| {
            cfg.adapters.get_mut("scripts").unwrap().enabled = false;
        });
        let row = row_line(&render_to_text(&app), "Scripts");
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
}
