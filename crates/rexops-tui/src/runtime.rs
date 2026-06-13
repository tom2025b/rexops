//! Draw/input/runtime loop for the RexOps TUI.

use std::io;
use std::process::{Command, ExitStatus};
use std::sync::mpsc;
use std::time::Duration;

use crossterm::event;
use rexops_core::OpsSnapshot;
use suite_ui::{Theme, Tui};

use crate::input::keymap::Event;
use crate::tools::{ChildExit, ForegroundRunner, LaunchCommand};
use crate::{app::App, input, ui};

/// Outcome of one loop iteration: whether anything changed that needs a repaint
/// next tick, and whether the app asked to quit. Returned by [`step`] so the
/// orchestration (drain snapshots → poll job → handle one event) can be tested
/// without a real terminal or crossterm input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StepResult {
    /// Something happened this tick (snapshot applied, job output/finish, or a
    /// handled keypress) so the next iteration should redraw.
    pub dirty: bool,
    /// The handled action requested quit; the loop should return.
    pub quit: bool,
}

/// One iteration of the runtime loop, minus the actual drawing. Drains any
/// pending snapshots, polls the running job, then applies at most one input
/// event. Pure with respect to its inputs — the event is passed in rather than
/// polled — so tests can drive the exact orchestration the real loop runs.
///
/// `runner` is whatever services a foreground tool launch (the real `Tui`, or a
/// fake in tests); `event` is the already-polled input for this tick (`None`
/// on an idle tick / poll timeout).
pub(crate) fn step(
    app: &mut App,
    rx: &mpsc::Receiver<OpsSnapshot>,
    event: Option<Event>,
    runner: &mut impl ForegroundRunner,
) -> StepResult {
    let mut dirty = false;

    while let Ok(snapshot) = rx.try_recv() {
        app.apply_snapshot(snapshot);
        dirty = true;
    }

    if app.poll_job() {
        dirty = true;
    }

    if let Some(Event::Key(key)) = event {
        // The app decides how keys are read (navigating vs typing into a text
        // field) — pass that mode so bound letters reach a focused field as
        // input instead of being claimed as commands.
        if let Some(action) = input::keymap::handle_key(key, app.input_mode()) {
            if app.on_action(action, runner) {
                return StepResult { dirty, quit: true };
            }
            // A handled key may have mutated any state; repaint next tick.
            dirty = true;
        }
    }

    StepResult { dirty, quit: false }
}

struct TuiForegroundRunner<'a> {
    tui: &'a mut Tui,
}

/// Run a foreground child program on the user's real terminal.
///
/// The leave→run→re-enter dance (drop out of raw mode + the alternate screen,
/// run the child, then re-enter and clear) is owned by the shared
/// [`suite_ui::Tui::suspended`] guard, which guarantees re-entry even if the
/// child or a step fails — so the terminal is never left suspended. The trait
/// lives in rexops-app; this local wrapper is what lets the TUI implement it
/// without violating Rust's orphan rules for `suite_ui::Tui`.
impl ForegroundRunner for TuiForegroundRunner<'_> {
    fn run_foreground(&mut self, command: &LaunchCommand) -> io::Result<ChildExit> {
        let status: ExitStatus = self
            .tui
            .suspended(|| Command::new(&command.program).args(&command.args).status())??;
        drain_pending_events()?;
        if status.success() {
            Ok(ChildExit::Success)
        } else {
            Ok(ChildExit::Status(status.to_string()))
        }
    }
}

fn drain_pending_events() -> io::Result<()> {
    while event::poll(Duration::from_millis(0))? {
        let _ = event::read()?;
    }
    Ok(())
}

pub fn run(
    tui: &mut Tui,
    app: &mut App,
    rx: &mpsc::Receiver<OpsSnapshot>,
    theme: Theme,
) -> Result<(), Box<dyn std::error::Error>> {
    // Redraw only when something could have changed, not on every tick. An idle
    // TUI (no key, no snapshot, no job output) would otherwise full-render and
    // diff the whole buffer ~10×/s forever — wasted CPU and a flicker risk. We
    // draw once up front, then set `dirty` whenever a snapshot is applied, the
    // job produces output / finishes, or a keypress is handled. The 100 ms poll
    // still bounds streaming-output latency; nothing here needs a timer-based
    // redraw (toasts clear on the next action, not on a clock).
    let mut dirty = true;

    loop {
        if dirty {
            tui.terminal().draw(|f| ui::render(f, app, theme))?;
            dirty = false;
        }

        // Poll one input event (blocks up to 100 ms), then run the shared loop
        // body over it. `step` drains snapshots and the job before applying the
        // key so the most recent state is what the action sees.
        let event = input::keymap::next_event(Duration::from_millis(100))?;
        let mut runner = TuiForegroundRunner { tui };
        let result = step(app, rx, event, &mut runner);
        if result.quit {
            return Ok(());
        }
        if result.dirty {
            dirty = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{backend::TestBackend, Terminal};
    use rexops_core::{AdapterHealth, AdapterId, AppConfig};

    use crate::tools::{ChildExit, ForegroundRunner, LaunchCommand};
    use crate::ui;

    /// Records foreground launches so we can assert a Launcher activation
    /// actually reached the runner through the real action pipeline.
    struct FakeRunner {
        calls: usize,
    }
    impl ForegroundRunner for FakeRunner {
        fn run_foreground(&mut self, _command: &LaunchCommand) -> std::io::Result<ChildExit> {
            self.calls += 1;
            Ok(ChildExit::Success)
        }
    }

    fn key(code: KeyCode) -> Option<Event> {
        Some(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    /// Build (App, snapshot sender, snapshot receiver) wired exactly as `main`
    /// does, so the loop sees a real refresh channel.
    fn harness() -> (App, mpsc::Sender<OpsSnapshot>, mpsc::Receiver<OpsSnapshot>) {
        let (tx, rx) = mpsc::channel();
        let app = App::new(tx.clone(), AppConfig::default(), None);
        (app, tx, rx)
    }

    #[test]
    fn idle_tick_is_not_dirty_and_does_not_quit() {
        let (mut app, _tx, rx) = harness();
        let mut runner = FakeRunner { calls: 0 };
        // No snapshot, no job, no input: the loop must do nothing and report
        // clean so the real `run` skips the redraw (the whole point of `dirty`).
        let r = step(&mut app, &rx, None, &mut runner);
        assert_eq!(
            r,
            StepResult {
                dirty: false,
                quit: false
            }
        );
    }

    #[test]
    fn applied_snapshot_marks_dirty_and_lands_in_app_state() {
        let (mut app, tx, rx) = harness();
        let mut runner = FakeRunner { calls: 0 };

        let mut snap = OpsSnapshot::new();
        let id = AdapterId::new("bulwark").expect("id");
        snap.set_adapter_health(&id, AdapterHealth::Healthy);
        tx.send(snap).expect("send snapshot");

        let r = step(&mut app, &rx, None, &mut runner);
        assert!(r.dirty, "a fresh snapshot must trigger a repaint");
        assert!(!r.quit);
        // The snapshot was actually applied (not just flagged): adapter_names is
        // derived from it in apply_snapshot.
        assert_eq!(app.adapter_names, vec!["bulwark".to_owned()]);
    }

    #[test]
    fn quit_key_returns_quit() {
        let (mut app, _tx, rx) = harness();
        let mut runner = FakeRunner { calls: 0 };
        let r = step(&mut app, &rx, key(KeyCode::Char('q')), &mut runner);
        assert!(r.quit, "'q' must end the loop");
    }

    #[test]
    fn navigation_key_changes_screen_and_marks_dirty() {
        let (mut app, _tx, rx) = harness();
        let mut runner = FakeRunner { calls: 0 };
        let start = app.current_screen;
        // '2' selects the second screen in the keymap.
        let r = step(&mut app, &rx, key(KeyCode::Char('2')), &mut runner);
        assert!(r.dirty, "a handled navigation key must request a repaint");
        assert!(!r.quit);
        assert_ne!(app.current_screen, start, "screen should have changed");
    }

    #[test]
    fn unhandled_key_is_not_dirty() {
        let (mut app, _tx, rx) = harness();
        let mut runner = FakeRunner { calls: 0 };
        // A key the navigation keymap maps to no action (Tab hits the `_ => None`
        // arm) must not force a redraw — `step` only marks dirty when a key
        // actually produces an Action.
        let r = step(&mut app, &rx, key(KeyCode::Tab), &mut runner);
        assert_eq!(
            r,
            StepResult {
                dirty: false,
                quit: false
            }
        );
    }

    #[test]
    fn full_sequence_navigate_then_quit_drives_a_real_render_each_dirty_tick() {
        // End-to-end: snapshot arrives, user navigates, then quits — and every
        // dirty tick renders cleanly to a real (test) backend without panicking.
        let (mut app, tx, rx) = harness();
        let mut runner = FakeRunner { calls: 0 };
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        let theme = Theme::with_color(false);

        let mut renders = 0usize;
        let mut draw = |app: &App| {
            terminal
                .draw(|f| ui::render(f, app, theme))
                .expect("render must not fail");
            renders += 1;
        };

        // Initial draw (mirrors `run`'s up-front paint).
        draw(&app);

        // Tick 1: a snapshot arrives.
        let mut snap = OpsSnapshot::new();
        let id = AdapterId::new("bulwark").expect("id");
        snap.set_adapter_health(&id, AdapterHealth::Healthy);
        tx.send(snap).expect("send");
        let r = step(&mut app, &rx, None, &mut runner);
        assert!(r.dirty);
        draw(&app);

        // Tick 2: navigate to another screen.
        let r = step(&mut app, &rx, key(KeyCode::Char('2')), &mut runner);
        assert!(r.dirty && !r.quit);
        draw(&app);

        // Tick 3: idle — nothing changes, so the real loop would NOT redraw.
        let r = step(&mut app, &rx, None, &mut runner);
        assert!(!r.dirty && !r.quit);

        // Tick 4: quit.
        let r = step(&mut app, &rx, key(KeyCode::Char('q')), &mut runner);
        assert!(r.quit);

        // We drew exactly on the up-front paint + the two dirty ticks; the idle
        // tick added no render. This is the dirty-flag contract the loop relies
        // on to stay quiet when nothing is happening.
        assert_eq!(
            renders, 3,
            "one initial + two dirty redraws, no idle redraw"
        );
    }
}
