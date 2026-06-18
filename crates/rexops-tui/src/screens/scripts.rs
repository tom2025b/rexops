//! scripts.rs — Scripts screen (4th screen).
//!
//! Shows the structured scripts section from the Workstate snapshot.
//! Lists scripts with neutral inventory metadata.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

use suite_ui::{pane, pane_blank, Theme};

use crate::app::App;
use crate::ui::widgets;
use rexops_core::Script;

/// Render the Scripts screen.
pub fn render_scripts(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(5),    // list of scripts
        ])
        .split(area);

    render_scripts_header(f, app, chunks[0], theme);
    render_scripts_list(f, app, chunks[1], theme);
}

fn render_scripts_header(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    // Scripts is a Workstate *section*, not an adapter — it carries FRESHNESS, not
    // health, and is deliberately absent from `adapter_health` (see app/snapshot).
    // Querying adapter_health here always missed and rendered a permanent
    // "? Unknown" badge. Badge the section's freshness instead, read from the
    // typed `WorkstateInfo` the snapshot already holds.
    let freshness = app
        .snapshot
        .workstate
        .as_ref()
        .map(|ws| rexops_core::status_to_freshness(&ws.scripts.status));
    let badge = widgets::render_freshness_badge(freshness, theme);

    let header = Paragraph::new(Line::from(vec![Span::raw("Scripts / Vault "), badge]))
        .block(pane_blank(theme));

    f.render_widget(header, area);
}

fn render_scripts_list(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let mut lines: Vec<Line> = Vec::new();

    if let Some(sv) = &app.snapshot.scripts {
        if sv.scripts.is_empty() {
            lines.push(Line::from("No scripts found."));
        } else {
            for s in &sv.scripts {
                let info = s.description.as_deref().unwrap_or("");
                let item = widgets::render_script_item(
                    s.label(),
                    info,
                    sv.is_favorite(s),
                    is_recent(&sv.recents, s),
                    theme,
                );
                lines.push(item);
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "Total: {} scripts, {} favorites, {} recents",
            sv.total(),
            sv.favorites_count(),
            sv.recents_count()
        )));
    } else {
        lines.push(Line::from(
            "No scripts data yet — press 'r' to load Workstate.",
        ));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Tip: Press '1' for Dashboard, '2' for Adapters, '3' for System.",
        theme.dim(),
    )));

    let list = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .block(pane("Scripts", theme));

    f.render_widget(list, area);
}

fn is_recent(recents: &[String], script: &Script) -> bool {
    recents.iter().any(|recent| {
        Some(recent.as_str()) == script.id.as_deref()
            || Some(recent.as_str()) == script.name.as_deref()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rexops_core::{AppConfig, OpsSnapshot, Script, ScriptsInfo, WorkstateInfo};
    use std::sync::mpsc;

    fn render_to_text(app: &App) -> String {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).expect("test backend");
        let theme = Theme::with_color(false);
        terminal
            .draw(|f| render_scripts(f, app, f.area(), theme))
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

    /// An App whose snapshot carries a Workstate envelope with the scripts section
    /// at the given freshness `status`.
    fn app_with_scripts_status(status: &str) -> App {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default(), None);
        let mut ws = WorkstateInfo::default();
        ws.scripts.status = status.to_owned();
        ws.scripts.data = Some(ScriptsInfo::default());
        let mut snap = OpsSnapshot::new();
        snap.workstate = Some(ws);
        snap.scripts = Some(ScriptsInfo::default());
        app.apply_snapshot(snap);
        app
    }

    fn app_with_script_inventory() -> App {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default(), None);
        let scripts = ScriptsInfo {
            scripts: vec![Script {
                id: Some("deploy-prod".to_owned()),
                name: Some("deploy-prod.sh".to_owned()),
                description: Some("Deploy to production with safety checks".to_owned()),
                ..Script::default()
            }],
            favorites: vec!["deploy-prod".to_owned()],
            recents: vec!["deploy-prod".to_owned()],
            ..ScriptsInfo::default()
        };
        let mut ws = WorkstateInfo::default();
        ws.scripts.status = "Fresh".to_owned();
        ws.scripts.data = Some(scripts.clone());
        let mut snap = OpsSnapshot::new();
        snap.workstate = Some(ws);
        snap.scripts = Some(scripts);
        app.apply_snapshot(snap);
        app
    }

    #[test]
    fn header_badges_section_freshness_not_a_permanent_unknown() {
        // THE P2 FIX: with a Fresh scripts section present, the header must read
        // its freshness ("fresh") — never the old permanent "? Unknown" that came
        // from looking the section up in adapter_health (where it never appears).
        let text = render_to_text(&app_with_scripts_status("Fresh"));
        assert!(
            text.contains("Scripts / Vault") && text.contains("fresh"),
            "fresh scripts section must badge 'fresh':\n{text}"
        );
        assert!(
            !text.contains("? Unknown"),
            "a present section must not render the permanent Unknown badge:\n{text}"
        );
    }

    #[test]
    fn header_shows_stale_neutrally_when_section_is_stale() {
        let text = render_to_text(&app_with_scripts_status("Stale"));
        assert!(
            text.contains("stale"),
            "a stale section must surface its freshness:\n{text}"
        );
    }

    #[test]
    fn header_is_unknown_only_when_no_snapshot_has_been_read() {
        // No Workstate envelope at all → genuinely Unknown (pre-probe), which is
        // the ONLY time the Unknown badge is correct.
        let (tx, _rx) = mpsc::channel();
        let app = App::new(tx, AppConfig::default(), None);
        let text = render_to_text(&app);
        assert!(
            text.contains("? Unknown"),
            "with no snapshot read, Unknown is the correct badge:\n{text}"
        );
    }

    #[test]
    fn script_rows_do_not_invent_adapter_health() {
        let text = render_to_text(&app_with_script_inventory());
        assert!(
            text.contains("deploy-prod") && text.contains("Deploy to production"),
            "script row should render the inventory item:\n{text}"
        );
        assert!(
            !text.contains("Healthy"),
            "script inventory rows must not render fake adapter health:\n{text}"
        );
    }

    #[test]
    fn script_rows_show_inventory_metadata_as_neutral_tags() {
        let text = render_to_text(&app_with_script_inventory());
        assert!(
            text.contains("favorite") && text.contains("recent"),
            "favorite/recent should be shown as script metadata, not health:\n{text}"
        );
    }
}
