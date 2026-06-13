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
//! would not work. The app decides which path a tool takes. A job is spawned
//! from a resolved program plus its catalog args (the same `resolve_launch_command`
//! the confirm-gate preview renders), so what runs matches what the user approved.
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

/// Bound on the output channel between the reader threads and the UI. A
/// `sync_channel` of this size applies **backpressure**: when the UI falls
/// behind, `send` blocks the reader thread, the child's stdout/stderr pipe
/// fills, and the child blocks on write — so a chatty or runaway job throttles
/// itself instead of piling unbounded lines in memory. Lines are never dropped;
/// they wait in order until the UI drains them. Sized generously so a normal
/// burst never blocks, only a sustained flood the UI can't keep up with.
const CHANNEL_CAP: usize = 4096;

/// Most lines `drain_into` will move per call. The visible buffer the UI keeps
/// is itself bounded (`JOB_OUTPUT_CAP`), so pulling more than this per tick is
/// wasted work — the surplus would be popped before it could ever be shown.
/// Capping the per-tick drain keeps the UI thread's work per frame bounded even
/// when the channel is full, so the draw loop never stalls behind a flood.
const DRAIN_BUDGET: usize = 1024;

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
    /// Drain up to [`DRAIN_BUDGET`] output lines available *right now* into
    /// `sink`, and report whether the output channel has **disconnected** — i.e.
    /// both reader threads have hit EOF and dropped their senders, so no more
    /// output can ever arrive.
    ///
    /// This is the race-free completion signal: `try_wait` reporting the child
    /// gone can win against a reader thread still mid-flush of its last line, so
    /// the child being dead does NOT mean the output is complete. The channel
    /// disconnecting does — it can only happen after both readers finish. The
    /// caller drains each tick until this returns `true`, never blocking the UI.
    ///
    /// The per-tick budget bounds the UI thread's work per frame: under a flood
    /// we move at most `DRAIN_BUDGET` lines and return `false` (more pending),
    /// so the draw loop keeps ticking and drains the rest on later frames rather
    /// than stalling on a full channel. Disconnect is only ever reported once the
    /// channel is genuinely drained AND empty, so hitting the budget never
    /// signals completion early — the trailing-line guarantee is preserved.
    pub fn drain_into(&self, sink: &mut impl Extend<JobOutput>) -> bool {
        for _ in 0..DRAIN_BUDGET {
            match self.rx.try_recv() {
                Ok(line) => sink.extend(std::iter::once(line)),
                // Nothing buffered, but the senders are still alive: more may come.
                Err(TryRecvError::Empty) => return false,
                // Both readers are gone and the buffer is empty: output is final.
                Err(TryRecvError::Disconnected) => return true,
            }
        }
        // Budget exhausted with lines still flowing: not disconnected, more to come.
        false
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

/// Dropping the handle kills and reaps the child. This is what makes quitting
/// the TUI cancel a running job instead of orphaning it: `std::process::Child`
/// does NOT kill on drop, so without this, `q` mid-job leaked a live process
/// to init with no record of it. On the normal finish path the child has
/// already been reaped by `poll_done`'s `try_wait`, so `kill` errors (ignored
/// — std refuses to kill an already-waited child, no pid-reuse risk) and
/// `wait` just returns the cached status.
///
/// Two deliberate limits: quitting with a job still running kills it
/// immediately and WITHOUT a confirmation modal (instant quit is the chosen
/// trade-off, unlike the confirm-gated mutations elsewhere in the TUI); and —
/// like exec.rs — `kill` reaches the direct child only, never grandchildren,
/// which is fine today because jobs spawn suite binaries directly.
///
/// Backpressure note: with the bounded channel a reader thread can be blocked in
/// `send` on a full channel while the child is blocked writing to a full pipe.
/// `kill` is SIGKILL — uncatchable and not deferred by the pending write — so
/// `wait` still reaps promptly and never deadlocks. The `rx` field drops after
/// this body returns, which releases any blocked `send` (it returns `Err`) and
/// lets the reader threads exit. Do NOT replace `kill` with a graceful
/// terminate here: a child ignoring the signal while blocked on a full pipe
/// could then hang `wait`.
impl Drop for JobHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Spawn `program` (with `args`) as a background job, returning a handle that
/// streams its output. Returns `None` if the process could not be spawned (bad
/// path, permission) — the caller reports that and stays idle.
///
/// `program` is the resolved binary and `args` its catalog-owned arguments
/// (e.g. a `tui`/`status` subcommand). The pair must match exactly what the
/// confirm-gate preview rendered via `resolve_launch_command`, so what the user
/// approved is what runs. Stdin is nulled: background jobs are non-interactive by
/// definition.
pub fn spawn(name: &str, program: &str, args: &[String]) -> Option<JobHandle> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    // Bounded channel for backpressure: when the UI lags, `send` blocks the
    // reader, the child's pipe fills, and the child blocks on write. Lines are
    // never dropped — a flood throttles the producer instead of growing memory.
    let (tx, rx) = mpsc::sync_channel(CHANNEL_CAP);

    // stdout reader: one line → one Stdout event. The thread ends at pipe EOF,
    // which happens when the child exits. A blocked `send` (full channel)
    // simply waits here until the UI drains — that IS the backpressure.
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

    // The pane header shows the full invocation, args included, so it matches
    // the command the user confirmed.
    let command = if args.is_empty() {
        program.to_owned()
    } else {
        format!("{program} {}", args.join(" "))
    };

    Some(JobHandle {
        name: name.to_owned(),
        command,
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

    /// Spawn a just-written script, retrying transient ETXTBSY failures: a
    /// parallel test's `fork` can briefly inherit this script's write fd
    /// (open during `write_script`, closed in the child only at its `exec`),
    /// and exec-ing a file someone holds open for write fails. The window is
    /// microseconds but made this ~50% flaky under the default parallel test
    /// runner; retrying is the standard fix (cargo does the same).
    fn spawn_script(name: &str, path: &str) -> JobHandle {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(job) = spawn(name, path, &[]) {
                return job;
            }
            assert!(
                Instant::now() < deadline,
                "spawn {name} kept failing — not a transient ETXTBSY"
            );
            sleep(Duration::from_millis(2));
        }
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
        let job = spawn_script("multiline", path);
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
    fn flood_under_backpressure_loses_no_lines() {
        // The bounded channel must apply BACKPRESSURE, not drop. A producer that
        // emits far more than CHANNEL_CAP lines (here 4× the cap), drained by a
        // deliberately slow consumer, fills the channel and blocks the reader's
        // `send` — the child then blocks on its pipe. The contract: every line
        // still arrives, in order, none dropped. If `send` silently discarded on
        // a full channel (the bug this guards), the count would come up short.
        let n = CHANNEL_CAP * 4;
        let script = write_script(
            "flood",
            &format!("#!/bin/sh\nfor i in $(seq 1 {n}); do echo line-$i; done\n"),
        );
        let path = script.to_str().unwrap();
        let mut job = spawn_script("flood", path);

        // Stall before draining so the producer races ahead and the channel
        // genuinely fills to CHANNEL_CAP — parking the reader thread in a blocked
        // `send`. This is the state that distinguishes backpressure from drop: a
        // drop-on-full sender would shed lines here and the final count would
        // fall short. (n = 4× cap guarantees the producer cannot have finished.)
        sleep(Duration::from_millis(50));

        // Then drain to completion. Every line emitted while we stalled — and
        // while the producer was blocked — must still be delivered, in order.
        let mut out = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        let exit = loop {
            let drained = job.drain_into(&mut out);
            if let (Some(exit), true) = (job.poll_done(), drained) {
                break exit;
            }
            assert!(Instant::now() < deadline, "flood job did not finish in time");
            sleep(Duration::from_millis(1));
        };
        let _ = std::fs::remove_file(&script);

        assert_eq!(exit, JobExit::Code(0));
        assert_eq!(out.len(), n, "every line must survive backpressure (none dropped)");
        assert_eq!(out.first(), Some(&JobOutput::Stdout("line-1".to_owned())));
        assert_eq!(
            out.last(),
            Some(&JobOutput::Stdout(format!("line-{n}"))),
            "order must be preserved through a full channel"
        );
    }

    #[test]
    fn drain_into_respects_the_per_tick_budget() {
        // One `drain_into` call must move at most DRAIN_BUDGET lines, returning
        // `false` (more pending) when it hits the budget with the child still
        // running — so the draw loop never stalls behind a full channel. We push
        // more than the budget through a real job, then drain ONCE and check the
        // cap held.
        let n = DRAIN_BUDGET * 2;
        let script = write_script(
            "budget",
            &format!("#!/bin/sh\nfor i in $(seq 1 {n}); do echo line-$i; done\n"),
        );
        let path = script.to_str().unwrap();
        let job = spawn_script("budget", path);

        // Wait until at least DRAIN_BUDGET+1 lines are buffered so a single drain
        // is guaranteed to hit the budget (the channel caps at CHANNEL_CAP, which
        // is >= DRAIN_BUDGET, so this is reachable).
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            // Peek by draining into a throwaway, but we need the real buffer — so
            // instead just give the producer a moment; it emits n >> budget fast.
            sleep(Duration::from_millis(20));
            let mut probe = Vec::new();
            let _ = job.drain_into(&mut probe);
            assert!(
                probe.len() <= DRAIN_BUDGET,
                "a single drain moved {} lines, over the budget of {DRAIN_BUDGET}",
                probe.len()
            );
            if probe.len() == DRAIN_BUDGET {
                // Proved the cap engaged on a full-enough channel.
                break;
            }
            assert!(
                Instant::now() < deadline,
                "channel never buffered enough to exercise the budget"
            );
        }

        // Drain the rest so the child isn't left blocked on a full pipe at drop.
        let mut rest = Vec::new();
        let drain_deadline = Instant::now() + Duration::from_secs(10);
        while !job.drain_into(&mut rest) {
            sleep(Duration::from_millis(1));
            assert!(Instant::now() < drain_deadline, "failed to drain remainder");
        }
        let _ = std::fs::remove_file(&script);
    }

    #[test]
    fn drain_into_reports_disconnect_only_after_the_child_is_done() {
        // `true` exits immediately with no output. `drain_into` must eventually
        // report disconnect (senders dropped) — that is what lets the production
        // loop know the output is complete.
        let job = spawn("true", "true", &[]).expect("spawn true");
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
        let job = spawn("true", "true", &[]).expect("spawn true");
        let (out, exit) = run_via_drain_into(job);
        assert_eq!(exit, JobExit::Code(0));
        assert!(out.is_empty(), "`true` produces no output");
    }

    #[test]
    fn spawn_captures_stdout_lines() {
        // `printf` is not guaranteed as a standalone binary, but `echo` is on
        // PATH on Linux and prints one line then exits 0.
        let job = spawn("echo", "echo", &[]).expect("spawn echo");
        let (out, exit) = run_via_drain_into(job);
        assert_eq!(exit, JobExit::Code(0));
        // `echo` with no args prints a single empty line.
        assert_eq!(out, vec![JobOutput::Stdout(String::new())]);
    }

    #[test]
    fn spawn_passes_args_to_the_child_and_reflects_them_in_command() {
        // Args must reach the spawned child AND the displayed command, so a
        // background tool launched with a subcommand runs exactly what the
        // confirm-gate preview showed. `echo hello world` proves the args were
        // handed to the program (the output is the joined args), and the
        // `command` field must carry them too for the pane header.
        let args = vec!["hello".to_owned(), "world".to_owned()];
        let job = spawn("echo", "echo", &args).expect("spawn echo with args");
        assert_eq!(
            job.command, "echo hello world",
            "the displayed command must include the args"
        );
        let (out, exit) = run_via_drain_into(job);
        assert_eq!(exit, JobExit::Code(0));
        assert_eq!(
            out,
            vec![JobOutput::Stdout("hello world".to_owned())],
            "the child must actually receive the args"
        );
    }

    #[test]
    fn spawn_reports_nonzero_exit() {
        // `false` exits 1.
        let job = spawn("false", "false", &[]).expect("spawn false");
        let (_out, exit) = run_via_drain_into(job);
        assert_eq!(exit, JobExit::Code(1));
    }

    #[test]
    fn spawn_missing_binary_returns_none() {
        assert!(
            spawn("nope", "definitely-not-a-real-binary-xyz", &[]).is_none(),
            "a missing binary must not yield a handle"
        );
    }

    #[test]
    fn drop_kills_and_reaps_a_running_child() {
        // Regression for "quit doesn't cancel a running job": dropping the
        // handle (which is what quitting does — App drops, JobHandle drops)
        // must kill the child AND reap it. `yes` runs forever (an argless
        // coreutils binary — no temp script, which also avoids the fork/exec
        // ETXTBSY race two concurrent write-script tests would hit), so the
        // child is provably still running at drop. A reaped pid has no /proc
        // entry (a zombie still would), so the assert proves both kill and reap.
        let job = spawn("yes", "yes", &[]).expect("spawn yes");
        let pid = job.child.id();
        let proc_path = format!("/proc/{pid}");
        assert!(
            std::path::Path::new(&proc_path).exists(),
            "child must be alive before drop"
        );

        drop(job); // Drop = kill + wait; by the time it returns the pid is gone

        assert!(
            !std::path::Path::new(&proc_path).exists(),
            "child must be killed and reaped by Drop, not orphaned"
        );
    }

    #[test]
    fn cancel_is_idempotent_and_leaves_a_terminal_poll() {
        // `cancel` is best-effort: it kills a live child, and is a harmless no-op
        // on one that already exited. After cancelling, `poll_done` must always
        // reach a terminal result (the child is gone either way). We use a
        // short-lived process; calling cancel twice exercises the no-op path too.
        let mut job = spawn("true", "true", &[]).expect("spawn true");
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
