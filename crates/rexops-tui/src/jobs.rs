//! jobs.rs — the in-TUI background job runner.
//!
//! Runs a suite tool in the BACKGROUND (the opposite of `launcher.rs`, which
//! hands the whole terminal to a foreground child) and streams its output into a
//! pane while the TUI keeps drawing. Reader threads push output lines over an
//! mpsc channel; the main loop drains them every iteration with `try_recv`,
//! exactly like the existing snapshot-refresh thread.
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
use std::sync::mpsc::{self, Receiver};
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
    /// Non-blocking poll for the next output line. `None` means nothing is ready
    /// right now (the reader threads may still be running).
    pub fn try_recv(&self) -> Option<JobOutput> {
        self.rx.try_recv().ok()
    }

    /// Non-blocking check for completion. Returns `Some(exit)` once the child has
    /// exited, `None` while it is still running. The caller should first drain
    /// any remaining output with [`try_recv`](Self::try_recv) so no trailing
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

    /// Drain output + wait for completion the way the main loop does: poll
    /// `try_recv`/`poll_done` until the job exits or a timeout elapses. Returns
    /// the collected output lines and the exit.
    fn run_to_completion(mut job: JobHandle) -> (Vec<JobOutput>, JobExit) {
        let mut out = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            while let Some(line) = job.try_recv() {
                out.push(line);
            }
            if let Some(exit) = job.poll_done() {
                while let Some(line) = job.try_recv() {
                    out.push(line);
                }
                return (out, exit);
            }
            assert!(Instant::now() < deadline, "job did not finish in time");
            sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn spawn_streams_stdout_and_reports_zero_exit() {
        // `true` exits 0 with no output; use a tiny `sh -c` would need args, so we
        // run a guaranteed-present no-output success first.
        let job = spawn("true", "true").expect("spawn true");
        let (out, exit) = run_to_completion(job);
        assert_eq!(exit, JobExit::Code(0));
        assert!(out.is_empty(), "`true` produces no output");
    }

    #[test]
    fn spawn_captures_stdout_lines() {
        // `printf` is not guaranteed as a standalone binary, but `echo` is on
        // PATH on Linux and prints one line then exits 0.
        let job = spawn("echo", "echo").expect("spawn echo");
        let (out, exit) = run_to_completion(job);
        assert_eq!(exit, JobExit::Code(0));
        // `echo` with no args prints a single empty line.
        assert_eq!(out, vec![JobOutput::Stdout(String::new())]);
    }

    #[test]
    fn spawn_reports_nonzero_exit() {
        // `false` exits 1.
        let job = spawn("false", "false").expect("spawn false");
        let (_out, exit) = run_to_completion(job);
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
            assert!(Instant::now() < deadline, "cancelled job never reported done");
            sleep(Duration::from_millis(5));
        }
    }
}
