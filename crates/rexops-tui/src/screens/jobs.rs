//! jobs.rs — the Jobs screen: live (or last) output of a background job.
//!
//! Renders the one running job's streamed output into a scroll-tailing pane.
//! While a job runs the header shows a live marker; when idle it shows the last
//! job's outcome (or a hint to launch one). stderr lines get the shared failure
//! style plus an `[err]` marker so they stay distinguishable under `NO_COLOR`.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use suite_ui::{pane, Theme};

use crate::app::App;
use crate::jobs::JobOutput;

/// Render the Jobs screen into the given area.
pub fn render_jobs(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(5),    // streamed output
        ])
        .split(area);

    render_jobs_header(f, app, chunks[0], theme);
    render_jobs_output(f, app, chunks[1], theme);
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
        vec![Line::from(Span::styled(
            "(no output yet)",
            theme.dim(),
        ))]
    } else {
        // Tail the output to what fits the pane height (minus border + padding),
        // so the newest lines are always visible without a scroll model.
        let visible = area.height.saturating_sub(2) as usize;
        let start = app.job_output.len().saturating_sub(visible);
        app.job_output[start..]
            .iter()
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
