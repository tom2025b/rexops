//! jobs.rs — the in-TUI background job runner.
//!
//! Runs a suite tool in the BACKGROUND (the opposite of `launcher.rs`, which
//! hands the whole terminal to a foreground child) and streams its output into a
//! pane while the TUI keeps drawing. Reader threads push output lines over an
//! mpsc channel; the main loop drains them every iteration with `drain_into`,
//! which also reports when the channel has disconnected (the race-free signal
//! that a finished job's output is complete).
//!
//! Scope: non-interactive tools only (they emit output and exit). Interactive
//! tools keep `launcher.rs`'s foreground hand-over — piping a TUI into a pane
//! would not work. The app decides which path a tool takes.
//!
//! Concurrency: one job at a time. The app holds a single [`JobHandle`]; arming a
//! new job while one is active is refused upstream. [`JobHandle::cancel`] kills
//! the child so a hung tool can always be stopped from the TUI.
//!
//! Completion is detected by the main loop, not a waiter thread: the `Child`
//! stays in the handle (so `cancel` can kill it), and the loop polls
//! [`JobHandle::poll_done`] — a non-blocking `try_wait` — once it has drained the
//! pending output. That keeps the model to two reader threads and no extra
//! synchronisation around the child.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

/// One output line from a running job, delivered over the channel. Exit is NOT
/// an event — the loop reaps it via [`JobHandle::poll_done`] — so this carries
/// pure output, never control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobOutput {
    /// A line from the child's stdout (newline stripped).
    Stdout(String),
    /// A line from the child's stderr (newline stripped).
    Stderr(String),
}

/// How a finished job ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobExit {
    /// Exited with this status code.
    Code(i32),
    /// Terminated by a signal (e.g. our own cancel) or the code was unavailable.
    Signalled,
}

/// A handle to the one running background job. Holds the child (so we can kill
/// it) and the receiving end of its output stream.
pub struct JobHandle {
    /// Display name of the tool being run (for the pane title / status).
    pub name: String,
    /// The resolved command that was spawned (shown in the pane header).
    pub command: String,
    child: Child,
    rx: Receiver<JobOutput>,
}

impl JobHandle {
    /// Drain every output line available *right now* into `sink`, and report
    /// whether the output channel has **disconnected** — i.e. both reader threads
    /// have hit EOF and dropped their senders, so no more output can ever arrive.
    ///
    /// This is the race-free completion signal: `try_wait` reporting the child
    /// gone can win against a reader thread still mid-flush of its last line, so
    /// the child being dead does NOT mean the output is complete. The channel
    /// disconnecting does — it can only happen after both readers finish. The
    /// caller drains each tick until this returns `true`, never blocking the UI.
    pub fn drain_into(&self, sink: &mut impl Extend<JobOutput>) -> bool {
        loop {
            match self.rx.try_recv() {
                Ok(line) => sink.extend(std::iter::once(line)),
                // Nothing buffered, but the senders are still alive: more may come.
                Err(TryRecvError::Empty) => return false,
                // Both readers are gone and the buffer is empty: output is final.
                Err(TryRecvError::Disconnected) => return true,
            }
        }
    }

    /// Non-blocking check for completion. Returns `Some(exit)` once the child has
    /// exited, `None` while it is still running. The caller should first drain
    /// any remaining output with [`drain_into`](Self::drain_into) so no trailing
    /// lines are lost when the job finishes.
    pub fn poll_done(&mut self) -> Option<JobExit> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(match status.code() {
                Some(code) => JobExit::Code(code),
                None => JobExit::Signalled,
            }),
            _ => None,
        }
    }

    /// Kill the running child (cancel). Best-effort: a child that already exited
    /// returns an error from `kill`, which we ignore. The next `poll_done` then
    /// reports `Signalled`, so the app finishes its bookkeeping uniformly.
    pub fn cancel(&mut self) {
        let _ = self.child.kill();
    }
}

/// Spawn `command` as a background job, returning a handle that streams its
/// output. Returns `None` if the process could not be spawned (bad path,
/// permission) — the caller reports that and stays idle.
///
/// `command` is a single program token (the resolved binary). The suite's tools
/// are invoked by name with no arguments, matching `launcher.rs`. Stdin is nulled:
/// background jobs are non-interactive by definition.
pub fn spawn(name: &str, command: &str) -> Option<JobHandle> {
    let mut child = Command::new(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    let (tx, rx) = mpsc::channel();

    // stdout reader: one line → one Stdout event. The thread ends at pipe EOF,
    // which happens when the child exits.
    if let Some(out) = child.stdout.take() {
        let tx = tx.clone();
        thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if tx.send(JobOutput::Stdout(line)).is_err() {
                    break; // receiver gone (app shutting down)
                }
            }
        });
    }

    // stderr reader: one line → one Stderr event.
    if let Some(err) = child.stderr.take() {
        thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                if tx.send(JobOutput::Stderr(line)).is_err() {
                    break;
                }
            }
        });
    }

    Some(JobHandle {
        name: name.to_owned(),
        command: command.to_owned(),
        child,
        rx,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    /// Drive completion the way the *production* loop now does: each tick, drain
    /// available output and only finish once the child has exited AND the output
    /// channel has disconnected (`drain_into` → true). No grace-window heuristic —
    /// the disconnect is the race-free completion signal, so this must capture all
    /// output without losing the trailing lines.
    fn run_via_drain_into(mut job: JobHandle) -> (Vec<JobOutput>, JobExit) {
        let mut out = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let drained = job.drain_into(&mut out);
            if let (Some(exit), true) = (job.poll_done(), drained) {
                return (out, exit);
            }
            assert!(Instant::now() < deadline, "job did not finish in time");
            sleep(Duration::from_millis(2));
        }
    }

    /// Write a small shell script to a temp path and return it, so a test can
    /// spawn a multi-line emitter (the suite spawns a single program token with no
    /// args, so we need a self-contained executable script).
    fn write_script(name: &str, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("rexops-test-{name}-{}", std::process::id()));
        std::fs::write(&path, body).expect("write script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod script");
        path
    }

    #[test]
    fn drain_into_captures_every_line_including_the_trailing_one() {
        // A multi-line emitter that prints N lines then exits. The production drain
        // path must collect all N — the regression this guards is losing the last
        // line(s) when `try_wait` wins the race against a reader's final flush.
        let script = write_script(
            "multiline",
            "#!/bin/sh\nfor i in $(seq 1 200); do echo line-$i; done\n",
        );
        let path = script.to_str().unwrap();
        let job = spawn("multiline", path).expect("spawn script");
        let (out, exit) = run_via_drain_into(job);
        let _ = std::fs::remove_file(&script);

        assert_eq!(exit, JobExit::Code(0));
        assert_eq!(out.len(), 200, "every line must be captured");
        assert_eq!(out.first(), Some(&JobOutput::Stdout("line-1".to_owned())));
        assert_eq!(
            out.last(),
            Some(&JobOutput::Stdout("line-200".to_owned())),
            "the trailing line must not be lost"
        );
    }

    #[test]
    fn drain_into_reports_disconnect_only_after_the_child_is_done() {
        // `true` exits immediately with no output. `drain_into` must eventually
        // report disconnect (senders dropped) — that is what lets the production
        // loop know the output is complete.
        let job = spawn("true", "true").expect("spawn true");
        let mut out = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if job.drain_into(&mut out) {
                break; // disconnected — readers finished and dropped their senders
            }
            assert!(Instant::now() < deadline, "channel never disconnected");
            sleep(Duration::from_millis(2));
        }
        assert!(out.is_empty(), "`true` produces no output");
    }

    #[test]
    fn spawn_streams_stdout_and_reports_zero_exit() {
        // `true` exits 0 with no output; use a tiny `sh -c` would need args, so we
        // run a guaranteed-present no-output success first.
        let job = spawn("true", "true").expect("spawn true");
        let (out, exit) = run_via_drain_into(job);
        assert_eq!(exit, JobExit::Code(0));
        assert!(out.is_empty(), "`true` produces no output");
    }

    #[test]
    fn spawn_captures_stdout_lines() {
        // `printf` is not guaranteed as a standalone binary, but `echo` is on
        // PATH on Linux and prints one line then exits 0.
        let job = spawn("echo", "echo").expect("spawn echo");
        let (out, exit) = run_via_drain_into(job);
        assert_eq!(exit, JobExit::Code(0));
        // `echo` with no args prints a single empty line.
        assert_eq!(out, vec![JobOutput::Stdout(String::new())]);
    }

    #[test]
    fn spawn_reports_nonzero_exit() {
        // `false` exits 1.
        let job = spawn("false", "false").expect("spawn false");
        let (_out, exit) = run_via_drain_into(job);
        assert_eq!(exit, JobExit::Code(1));
    }

    #[test]
    fn spawn_missing_binary_returns_none() {
        assert!(
            spawn("nope", "definitely-not-a-real-binary-xyz").is_none(),
            "a missing binary must not yield a handle"
        );
    }

    #[test]
    fn cancel_is_idempotent_and_leaves_a_terminal_poll() {
        // `cancel` is best-effort: it kills a live child, and is a harmless no-op
        // on one that already exited. After cancelling, `poll_done` must always
        // reach a terminal result (the child is gone either way). We use a
        // short-lived process; calling cancel twice exercises the no-op path too.
        let mut job = spawn("true", "true").expect("spawn true");
        job.cancel();
        job.cancel(); // idempotent — must not panic
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if job.poll_done().is_some() {
                break; // terminal result reached
            }
            assert!(
                Instant::now() < deadline,
                "cancelled job never reported done"
            );
            sleep(Duration::from_millis(5));
        }
    }
}
