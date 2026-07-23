//! Bounded child-process execution for the bench harness.
//!
//! Every spawn goes through `spawn_output_bounded` with a wall-clock
//! timeout and a process-group SIGKILL on timeout. `tr run` /
//! `tr build` AOT-compiles + spawns `torajs-run-new-*`; killing only
//! the direct child would leave that grandchild orphaned to init,
//! pegging a core (the failure mode that took down mini via the
//! test262 runner on 2026-07-23 — see `test262-runner/verdict.rs`
//! and `conformance/runner/exec.rs`). `.process_group(0)` makes tr
//! the group leader; `kill(-pgid, SIGKILL)` on timeout reaps tr and
//! its grandchild atomically.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Bench-harness default: a compile step must finish within this
/// budget. LLVM cold-cache AOT builds top out around 30 s per case;
/// 300 s absorbs hyperfine warmup + retries with room to spare and
/// still catches a truly stuck fixture in one round instead of
/// letting it wedge the whole bench run overnight.
pub(crate) const PER_EXEC_TIMEOUT: Duration = Duration::from_secs(300);

/// `kill(2)` with a negated pid signals the whole process group.
/// Inline libc extern — the bench harness is a dev tool and this
/// keeps its dep list unchanged (no need to pull in `libc` just for
/// one syscall).
const SIGKILL: i32 = 9;
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

/// Spawn `cmd args` with `env` overlaid, wait up to `timeout` for
/// completion, and SIGKILL the whole process group on timeout.
/// Returns the collected `Output` on any exit (success OR non-zero)
/// so callers keep their existing exit-code dispatch — tr's `exit 3`
/// = `NotYetImplemented`. `Err(msg)` only covers spawn failure and
/// timeout.
pub(crate) fn spawn_output_bounded(
    cmd: &str,
    args: &[&str],
    env: &[(String, String)],
    timeout: Duration,
) -> Result<Output, String> {
    let mut command = Command::new(cmd);
    command.args(args);
    for (k, v) in env {
        command.env(k, v);
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("spawn `{cmd}`: {e}"))?;

    // Group leader's pid == pgid; negate to target the group.
    let pgid = child.id() as i32;

    let stdout_pipe = child.stdout.take().expect("stdout piped");
    let stderr_pipe = child.stderr.take().expect("stderr piped");
    let stdout_thread = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = std::io::BufReader::new(stdout_pipe).read_to_end(&mut v);
        v
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = std::io::BufReader::new(stderr_pipe).read_to_end(&mut v);
        v
    });

    let start = Instant::now();
    let mut backoff_us: u64 = 500;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if start.elapsed() >= timeout {
                    unsafe { kill(-pgid, SIGKILL) };
                    let _ = child.wait();
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    return Err(format!(
                        "`{cmd}` timeout after {}s (SIGKILL group)",
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_micros(backoff_us));
                backoff_us = (backoff_us * 2).min(20_000);
            }
            Err(e) => return Err(format!("wait `{cmd}`: {e}")),
        }
    };

    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_output_on_success() {
        let out =
            spawn_output_bounded("echo", &["hello"], &[], Duration::from_secs(5)).expect("echo");
        assert_eq!(out.stdout, b"hello\n");
        assert!(out.status.success());
    }

    #[test]
    fn returns_output_on_nonzero_exit() {
        let out = spawn_output_bounded("false", &[], &[], Duration::from_secs(5))
            .expect("false spawn ok");
        assert!(!out.status.success());
    }

    #[test]
    fn timeout_returns_err() {
        let start = Instant::now();
        let err = spawn_output_bounded("sleep", &["30"], &[], Duration::from_millis(200))
            .expect_err("expected timeout");
        assert!(err.contains("timeout") && err.contains("SIGKILL"), "{err}");
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    /// Without process_group(0) + kill(-pgid), sh's grandchild
    /// sleep would outlive the parent SIGKILL. Group signalling
    /// takes both, so this test returns quickly.
    #[test]
    fn timeout_reaps_grandchild_via_process_group() {
        let start = Instant::now();
        let _ = spawn_output_bounded(
            "sh",
            &["-c", "sleep 30 & wait"],
            &[],
            Duration::from_millis(200),
        );
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "group SIGKILL should reap sh+sleep in <3s, got {:?}",
            start.elapsed()
        );
    }
}
