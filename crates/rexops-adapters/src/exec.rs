//! exec.rs — Sole module that spawns external processes (private).
//!
//! run_optional: graceful missing-binary -> Ok(None) for probes.
//! run_json:     missing binary -> BinaryNotFound Err for data calls.
//! All calls are timeout-bounded. No shell. Pure argv. Returns AdapterError only.

use std::io::ErrorKind;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::error::AdapterError;

/// Default hard timeout applied to every external invocation unless the
/// caller explicitly passes a shorter/longer value.
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

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
    // Do not inherit env beyond what the caller has; explicit is better but
    // for adapter probes we usually want the user's PATH, so inherit is fine.

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            Ok(Some(stdout))
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            Err(AdapterError::CommandFailed {
                command: binary.to_owned(),
                exit_code: output.status.code(),
                stderr,
            })
        }
        Ok(Err(e)) => Err(e.into()),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(AdapterError::Timeout(start.elapsed())),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(AdapterError::CommandFailed {
            command: binary.to_owned(),
            exit_code: None,
            stderr: "worker thread lost".into(),
        }),
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
pub(crate) fn probe_version(binary: &str) -> Result<Option<String>, AdapterError> {
    let out = run_optional(binary, &["--version"], DEFAULT_TIMEOUT)?;
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
        let _ = probe_version("echo").expect("echo probe");
    }

    #[test]
    fn timeout_returns_timeout_err() {
        // sleep will reliably block longer than the tiny timeout.
        let res = run_optional("sh", &["-c", "sleep 2"], Duration::from_millis(50));
        assert!(matches!(res, Err(AdapterError::Timeout(_))));
    }
}
