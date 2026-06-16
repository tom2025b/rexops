//! exec.rs — Sole module that spawns external processes (private).
//!
//! run_optional: graceful missing-binary -> Ok(None) for probes.
//! run_json:     missing binary -> BinaryNotFound Err for data calls.
//! All calls are timeout-bounded. No shell. Pure argv. Returns AdapterError only.

use std::io::{ErrorKind, Read};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::AdapterError;

/// Default hard timeout applied to every external invocation unless the
/// caller explicitly passes a shorter/longer value.
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// How often the exit poll checks `try_wait`. Coarse on purpose: adapter calls
/// are seconds-scale probes, and a finer poll buys nothing but wakeups.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Run binary with args. Missing binary (ENOENT on spawn) -> Ok(None).
/// Success (0) -> Some(trim(stdout)). Nonzero -> CommandFailed. Timeout -> Timeout.
pub(crate) fn run_optional(
    binary: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<Option<String>, AdapterError> {
    let start = Instant::now();

    // Build argv form — never goes through a shell, so no injection risk.
    let mut cmd = Command::new(binary);
    cmd.args(args);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    // Own process group (child becomes leader, pid == pgid) so a timeout-kill
    // can `killpg` the WHOLE tree — a grandchild a probe forks dies too, instead
    // of lingering until it finishes on its own.
    cmd.process_group(0);
    // Do not inherit env beyond what the caller has; explicit is better but
    // for adapter probes we usually want the user's PATH, so inherit is fine.

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    // Drain both pipes on their own threads while we poll for exit: a child
    // that writes more than the OS pipe buffer would otherwise block on a full
    // pipe forever and turn into a spurious timeout. Keeping `child` HERE (not
    // moved into a wait thread) is what makes kill-on-timeout possible at all.
    let stdout_reader = spawn_pipe_reader(child.stdout.take());
    let stderr_reader = spawn_pipe_reader(child.stderr.take());

    let deadline = start + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Exited — but the pipes only hit EOF once every writer is
                // gone. A grandchild that inherited them (a daemonizing tool)
                // keeps the pipes open after the child dies, so the readers
                // are waited on against the SAME deadline; past it we detach
                // them and report Timeout instead of hanging forever. (The
                // child is already dead and the grandchild's pid is unknown —
                // there is nothing left to kill.)
                while !(stdout_reader.is_finished() && stderr_reader.is_finished()) {
                    if Instant::now() >= deadline {
                        return Err(AdapterError::Timeout(start.elapsed()));
                    }
                    thread::sleep(POLL_INTERVAL);
                }
                let stdout = stdout_reader.join().unwrap_or_default();
                let stderr = stderr_reader.join().unwrap_or_default();
                return if status.success() {
                    Ok(Some(String::from_utf8_lossy(&stdout).trim().to_owned()))
                } else {
                    Err(AdapterError::CommandFailed {
                        command: binary.to_owned(),
                        exit_code: status.code(),
                        stderr: String::from_utf8_lossy(&stderr).trim().to_owned(),
                    })
                };
            }
            Ok(None) if Instant::now() >= deadline => {
                kill_and_reap(&mut child, stdout_reader, stderr_reader);
                return Err(AdapterError::Timeout(start.elapsed()));
            }
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(e) => {
                kill_and_reap(&mut child, stdout_reader, stderr_reader);
                return Err(e.into());
            }
        }
    }
}

/// Read a child's pipe to EOF on its own thread, returning the bytes. `None`
/// (pipe not captured) yields an empty buffer.
fn spawn_pipe_reader<R: Read + Send + 'static>(pipe: Option<R>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    })
}

/// Kill the child's whole process GROUP and reap the child, then DETACH the pipe
/// readers (do not join).
///
/// The child is spawned as its own group leader, so `killpg` reaches the direct
/// child AND any grandchildren it forked (a shell wrapper's `sleep`, a
/// daemonizing tool) — the whole tree dies, not just the child. That closes the
/// inherited-pipe ends, so the readers normally hit EOF promptly.
///
/// We still DETACH rather than join the readers: a grandchild can race to hold a
/// pipe open in the instant between fork and our kill, and joining could then
/// block past the deadline — defeating the very timeout that called us. Dropping
/// the handles returns immediately (the point of a timeout); a detached reader
/// on a still-open pipe is harmless, holding only its own buffer and exiting when
/// the pipe finally closes. Group-killing makes that window vanishingly small;
/// detaching keeps the timeout honest even if it occurs.
fn kill_and_reap(
    child: &mut std::process::Child,
    stdout_reader: thread::JoinHandle<Vec<u8>>,
    stderr_reader: thread::JoinHandle<Vec<u8>>,
) {
    kill_process_group(child.id());
    let _ = child.wait();
    // Detach, don't join: see the doc comment. Dropping the handles is what keeps
    // a late grandchild holding the pipe from hanging the timeout path.
    drop(stdout_reader);
    drop(stderr_reader);
}

/// SIGKILL the process GROUP led by `pgid` (best-effort). Probes are spawned into
/// their own group (`process_group(0)`, child pid == group id), so one `killpg`
/// tears down the direct child and any grandchildren together. Errors (the group
/// already exited) are ignored.
fn kill_process_group(pgid: u32) {
    // A real pid/pgid always fits in i32, so this cast is exact in practice; the
    // u32→pid_t(i32) conversion is the standard FFI shape. (MSRV-portable: avoids
    // `u32::cast_signed`, stabilized only in 1.87.)
    #[allow(clippy::cast_possible_wrap)]
    let pgid = pgid as libc::pid_t;
    // SAFETY: a plain libc call with an integer pgid and signal; touches no
    // memory and any failure (ESRCH/EPERM) is surfaced only via the ignored
    // return value.
    unsafe {
        libc::killpg(pgid, libc::SIGKILL);
    }
}

/// run_optional + require present + serde_json::from_str. Missing -> BinaryNotFound.
pub(crate) fn run_json<T>(binary: &str, args: &[&str], timeout: Duration) -> Result<T, AdapterError>
where
    T: serde::de::DeserializeOwned,
{
    let stdout =
        run_optional(binary, args, timeout)?.ok_or_else(|| AdapterError::BinaryNotFound {
            binary: binary.to_owned(),
        })?;

    let value: T = serde_json::from_str(&stdout)?;
    Ok(value)
}

/// Run <binary> --version, extract first semver-ish token (strips leading v).
/// `timeout` bounds the spawn (callers thread their configured value through).
pub(crate) fn probe_version(
    binary: &str,
    timeout: Duration,
) -> Result<Option<String>, AdapterError> {
    let out = run_optional(binary, &["--version"], timeout)?;
    let Some(line) = out else {
        // Binary was not present — the caller (Adapter impl) will map to health.
        return Ok(None);
    };

    let first = line.split_whitespace().next().unwrap_or("");
    if first.is_empty() {
        return Ok(None);
    }
    let ver = first
        .strip_prefix('v')
        .or_else(|| first.strip_prefix('V'))
        .unwrap_or(first);
    if ver.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return Ok(Some(ver.to_owned()));
    }
    let second = line
        .split_whitespace()
        .nth(1)
        .unwrap_or("")
        .trim_start_matches(['v', 'V']);
    if second.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        Ok(Some(second.to_owned()))
    } else {
        Ok(Some(first.to_owned()))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn run_optional_missing_binary_is_ok_none() {
        let res = run_optional(
            "rexops-test-no-such-binary-xyz987",
            &["--version"],
            Duration::from_secs(1),
        );
        assert!(res.is_ok());
        assert!(res.unwrap().is_none());
    }

    #[test]
    fn run_optional_echo_works() {
        let res = run_optional("echo", &["hello-adapter"], Duration::from_secs(2));
        assert!(res.is_ok());
        let out = res.unwrap().unwrap();
        assert_eq!(out, "hello-adapter");
    }

    #[test]
    fn run_optional_nonzero_exit_is_command_failed() {
        // sh -c 'echo err >&2; exit 3'
        let res = run_optional(
            "sh",
            &["-c", "echo 'error on stderr' >&2; exit 7"],
            Duration::from_secs(2),
        );
        match res {
            Err(AdapterError::CommandFailed {
                exit_code, stderr, ..
            }) => {
                assert_eq!(exit_code, Some(7));
                assert!(stderr.contains("error on stderr"));
            }
            other => panic!("expected CommandFailed, got {other:?}"),
        }
    }

    #[test]
    fn run_json_parses_valid() {
        // Use a command that prints JSON on stdout.
        let res: Result<serde_json::Value, _> = run_json(
            "sh",
            &["-c", r#"echo '{"ok": true, "n": 42}'"#],
            Duration::from_secs(2),
        );
        assert!(res.is_ok());
        let v = res.unwrap();
        assert_eq!(v["n"], 42);
    }

    #[test]
    fn run_json_missing_is_binary_not_found() {
        let res: Result<serde_json::Value, _> = run_json(
            "rexops-test-absent-zzz",
            &["--json"],
            Duration::from_secs(1),
        );
        assert!(matches!(res, Err(AdapterError::BinaryNotFound { .. })));
    }

    #[test]
    fn run_json_bad_json_is_parse_err() {
        let res: Result<serde_json::Value, _> = run_json("echo", &["{bad"], Duration::from_secs(2));
        assert!(matches!(res, Err(AdapterError::JsonParse(_))));
    }

    #[test]
    fn probe_version_does_not_panic() {
        let _ = probe_version("echo", DEFAULT_TIMEOUT).expect("echo probe");
    }

    #[test]
    fn timeout_returns_timeout_err() {
        // sleep will reliably block longer than the tiny timeout.
        let res = run_optional("sh", &["-c", "sleep 2"], Duration::from_millis(50));
        assert!(matches!(res, Err(AdapterError::Timeout(_))));
    }

    #[test]
    fn grandchild_holding_the_pipe_does_not_hang_past_the_deadline() {
        // `sh` exits immediately, but the backgrounded sleep inherits the
        // stdout pipe and holds it open for 5s — the readers can't EOF. The
        // call must give up at the deadline (detaching the readers), not hang
        // until the grandchild lets go.
        let begin = Instant::now();
        let res = run_optional(
            "sh",
            &["-c", "sleep 5 & exit 0"],
            Duration::from_millis(300),
        );
        assert!(matches!(res, Err(AdapterError::Timeout(_))));
        assert!(
            begin.elapsed() < Duration::from_secs(2),
            "must return at the deadline, not wait out the grandchild"
        );
    }

    #[test]
    fn timeout_kills_the_grandchild_too_via_the_process_group() {
        // The job/exec child runs in its own process group, so a timeout-kill
        // `killpg`s the WHOLE tree — a backgrounded grandchild that inherited the
        // pipe is killed too, not left to finish on its own. A unique marker
        // (this pid) makes pgrep find exactly this test's grandchild.
        let marker = format!("314159.{}", std::process::id());
        let begin = Instant::now();
        let res = run_optional(
            "sh",
            &["-c", &format!("sleep {marker} & sleep {marker}")],
            Duration::from_millis(200),
        );
        assert!(matches!(res, Err(AdapterError::Timeout(_))));
        // After the timeout-kill, no sleep with our marker may remain — the group
        // kill reached the grandchild. (Poll briefly: reap is asynchronous.)
        let pat = format!("sleep {marker}");
        let gone_by = Instant::now() + Duration::from_secs(3);
        loop {
            let alive = Command::new("pgrep")
                .args(["-f", &pat])
                .output()
                .is_ok_and(|o| o.status.success());
            if !alive {
                break;
            }
            assert!(
                Instant::now() < gone_by,
                "grandchild survived the timeout-kill — process group was not killed"
            );
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            begin.elapsed() < Duration::from_secs(3),
            "timeout path must stay prompt"
        );
    }

    #[test]
    fn timeout_with_grandchild_holding_the_pipe_still_returns_promptly() {
        // Regression: on a TIMEOUT-kill, a grandchild that inherited the stdout
        // pipe (here `sh -c 'sleep 5 & ...'` backgrounds a sleep) used to hang
        // kill_and_reap's reader join for the grandchild's whole lifetime —
        // defeating the timeout. The call must return at the deadline by
        // DETACHING the readers, not waiting them out. (The group-kill now also
        // reaps the grandchild; this test still guards the no-hang property.)
        let begin = Instant::now();
        let res = run_optional(
            "sh",
            &["-c", "sleep 5 & sleep 5"],
            Duration::from_millis(200),
        );
        assert!(matches!(res, Err(AdapterError::Timeout(_))));
        assert!(
            begin.elapsed() < Duration::from_secs(2),
            "timeout-kill must not block on a grandchild holding the pipe (took {:?})",
            begin.elapsed()
        );
    }

    #[test]
    fn timeout_kills_the_child() {
        // A unique sleep duration acts as a process-table marker. After the
        // timeout fires, run_optional must have killed AND reaped the child —
        // so pgrep for the marker finds nothing. (Spawn `sleep` directly, no
        // shell wrapper: kill() reaches the direct child only.)
        let marker = "31.4159265358979";
        let res = run_optional("sleep", &[marker], Duration::from_millis(50));
        assert!(matches!(res, Err(AdapterError::Timeout(_))));

        let pgrep = Command::new("pgrep")
            .args(["-f", &format!("sleep {marker}")])
            .output()
            .expect("pgrep runs");
        assert!(
            !pgrep.status.success(),
            "child must be killed on timeout, but found: {}",
            String::from_utf8_lossy(&pgrep.stdout)
        );
    }
}
