//! jobs.rs — the Jobs screen: live (or last) output of a background job, plus a
//! roll-up history of finished jobs this session.
//!
//! Renders the one running job's streamed output into a scroll-tailing pane.
//! While a job runs the header shows a live marker; when idle it shows the last
//! job's outcome (or a hint to launch one). stderr lines get the shared failure
//! style plus an `[err]` marker so they stay distinguishable under `NO_COLOR`.
//! Below the output, a history pane lists finished jobs (newest first) with the
//! same glyph/colour outcome cues the status bar uses.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use suite_ui::{pane, Theme};

use crate::app::{App, JobRecord};
use crate::jobs::JobOutput;

/// Render the Jobs screen into the given area.
pub fn render_jobs(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(5),    // streamed output
            Constraint::Length(8), // history roll-up
        ])
        .split(area);

    render_jobs_header(f, app, chunks[0], theme);
    render_jobs_output(f, app, chunks[1], theme);
    render_jobs_history(f, app, chunks[2], theme);
}

fn render_jobs_header(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let line = if let Some(job) = &app.job {
        // A live marker (● green/bold, or bold under NO_COLOR) signals streaming.
        Line::from(vec![
            Span::styled("● ", theme.live_marker()),
            Span::styled(format!("running {} ", job.name), theme.title()),
            Span::styled(format!("({})", job.command), theme.dim()),
        ])
    } else if let Some(last) = &app.last_job {
        Line::from(vec![
            Span::raw("idle — "),
            Span::styled(last.clone(), theme.dim()),
        ])
    } else {
        Line::from(Span::styled(
            "no jobs yet — run one from the palette (^P) or the Launcher",
            theme.dim(),
        ))
    };

    let header = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.dim()),
    );
    f.render_widget(header, area);
}

fn render_jobs_output(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let title = if app.job.is_some() {
        "output (live)"
    } else {
        "output (last job)"
    };

    let lines: Vec<Line> = if app.job_output.is_empty() {
        vec![Line::from(Span::styled("(no output yet)", theme.dim()))]
    } else {
        // Tail the output to what fits the pane height (minus border + padding),
        // so the newest lines are always visible without a scroll model.
        let visible = area.height.saturating_sub(2) as usize;
        let start = app.job_output.len().saturating_sub(visible);
        app.job_output
            .iter()
            .skip(start)
            .map(|out| match out {
                JobOutput::Stdout(text) => Line::from(Span::raw(text.clone())),
                // stderr: failure style + an explicit marker so it still reads as
                // stderr when the red hue drops under NO_COLOR.
                JobOutput::Stderr(text) => Line::from(vec![
                    Span::styled("[err] ", theme.status_error()),
                    Span::styled(text.clone(), theme.stderr()),
                ]),
            })
            .collect()
    };

    let output = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(pane(title, theme));
    f.render_widget(output, area);
}

/// One history row for a finished job: a leading outcome glyph + the exit summary,
/// styled to match the status bar / job-event toasts — `✓` green (clean exit),
/// `✗` red (non-zero), `■` yellow (cancelled). The glyph carries the outcome under
/// `NO_COLOR`, where the hues drop away.
fn history_line(record: &JobRecord, theme: Theme) -> Line<'static> {
    // Same (glyph, style) source the status bar and footer toast use, via the
    // shared Outcome — so a history row can never drift from how the same outcome
    // reads elsewhere.
    let (glyph, style) = record.outcome.as_outcome().glyph_style(theme);
    Line::from(vec![
        Span::styled(glyph, style),
        Span::styled(record.summary.clone(), style),
    ])
}

/// The history pane: finished jobs this session, newest first, capped to what
/// fits. A roll-up of outcomes (not a log archive) so the user can see what ran
/// without scrolling back through live output.
fn render_jobs_history(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let lines: Vec<Line> = if app.job_history.is_empty() {
        vec![Line::from(Span::styled(
            "(no finished jobs yet)",
            theme.dim(),
        ))]
    } else {
        // Newest first; show only what fits the pane (minus border).
        let visible = area.height.saturating_sub(2) as usize;
        app.job_history
            .iter()
            .rev()
            .take(visible)
            .map(|record| history_line(record, theme))
            .collect()
    };

    let title = format!("history ({})", app.job_history.len());
    let history = Paragraph::new(lines).block(pane(&title, theme));
    f.render_widget(history, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::LastOutcome;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rexops_core::AppConfig;
    use std::sync::mpsc;

    /// Render the Jobs screen off-screen and flatten it to text (glyphs only), the
    /// same buffer-to-string approach the launchpad tests and suite-ui gallery use.
    fn render_to_text(app: &App) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test backend");
        let theme = Theme::with_color(true);
        terminal
            .draw(|f| render_jobs(f, app, f.area(), theme))
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

    /// A fresh App with the given finished-job history applied.
    fn app_with_history(records: Vec<JobRecord>) -> App {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(tx, AppConfig::default());
        app.job_history = records;
        app
    }

    fn record(name: &str, ok: bool, cancelled: bool, summary: &str) -> JobRecord {
        JobRecord {
            name: name.to_owned(),
            outcome: LastOutcome {
                name: name.to_owned(),
                ok,
                cancelled,
            },
            summary: summary.to_owned(),
        }
    }

    #[test]
    fn empty_history_shows_the_placeholder() {
        let app = app_with_history(Vec::new());
        let text = render_to_text(&app);
        assert!(
            text.contains("no finished jobs yet"),
            "empty history must show a placeholder:\n{text}"
        );
        assert!(
            text.contains("history (0)"),
            "title shows the count:\n{text}"
        );
    }

    #[test]
    fn history_lists_finished_jobs_with_outcome_glyphs() {
        let app = app_with_history(vec![
            record("backup", true, false, "backup: finished (exit 0)"),
            record("rescan", false, false, "rescan: finished (exit 2)"),
            record("deploy", false, true, "deploy: cancelled / signalled"),
        ]);
        let text = render_to_text(&app);
        // Each outcome leads with its distinguishing glyph (carries it under
        // NO_COLOR too) and shows the exit summary.
        assert!(
            text.contains("✓ backup: finished (exit 0)"),
            "clean exit row:\n{text}"
        );
        assert!(
            text.contains("✗ rescan: finished (exit 2)"),
            "failed row:\n{text}"
        );
        assert!(
            text.contains("■ deploy: cancelled"),
            "cancelled row:\n{text}"
        );
        assert!(
            text.contains("history (3)"),
            "title counts all records:\n{text}"
        );
    }

    #[test]
    fn history_shows_newest_first() {
        // Records are stored oldest-first (push order); the pane reverses them, so
        // the most recently finished job appears on the first history line.
        let app = app_with_history(vec![
            record("old", true, false, "old: finished (exit 0)"),
            record("new", true, false, "new: finished (exit 0)"),
        ]);
        let text = render_to_text(&app);
        let new_pos = text.find("new: finished").expect("new present");
        let old_pos = text.find("old: finished").expect("old present");
        assert!(new_pos < old_pos, "newest job must render first:\n{text}");
    }
}
