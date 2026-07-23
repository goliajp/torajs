//! Bounded child-process execution for the conformance runner.
//!
//! Each `bun run`, `tr run`, `tr build`, or AOT-binary exec runs
//! with a wall-clock timeout and, on timeout, is SIGKILLed **as a
//! whole process group**. Killing only the direct child (`Child::kill`)
//! left `tr run`'s AOT-spawned `torajs-run-new-*` grandchild orphaned
//! to init when the fixture hung — the same failure mode that took
//! down mini via the test262 runner (see `test262-runner/verdict.rs`
//! module doc). Setting the child as its own process-group leader
//! (`process_group(0)`) and signalling `kill(-pgid, SIGKILL)`
//! reaps tr + its grandchild atomically.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Hard ceiling per child process invocation. Any single `bun run`,
/// `tr run`, `tr build`, or AOT-binary exec must finish within this
/// budget; otherwise we SIGKILL the whole process group and mark the
/// case Failed with a timeout reason. Real fixtures complete in tens
/// to a few hundred ms; 60 s is conservative even for cold-cache LLVM
/// AOT builds. Without this gate any single hung fixture (e.g. the
/// known throw-010-bigint-rangeerror macOS arm64 malloc-lock case)
/// would block the entire conformance run.
pub(crate) const PER_EXEC_TIMEOUT: Duration = Duration::from_secs(60);

/// `kill(2)` — sent with a negated pid to signal a whole process
/// group. Mirrors the pattern in `test262-runner/verdict.rs` so the
/// conformance gate can't leak grandchildren the way its untimed
/// ancestor did. Inline declaration keeps the runner's zero-dep build.
const SIGKILL: i32 = 9;
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

/// Default: run `cmd args` with the module-level `PER_EXEC_TIMEOUT`.
pub(crate) fn exec(cmd: &str, args: &[&str]) -> Result<(String, String), String> {
    exec_with_timeout(cmd, args, PER_EXEC_TIMEOUT)
}

/// Run `cmd args` with the given wall-clock deadline. Returns
/// `(stdout, stderr)` on exit 0 within the deadline; `Err(msg)` on
/// timeout (whole process group SIGKILLed), non-zero exit, or spawn
/// failure.
pub(crate) fn exec_with_timeout(
    cmd: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<(String, String), String> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // tr run AOT-compiles and spawns `torajs-run-new-*` as a
        // grandchild; making tr the group leader lets a SIGKILL on
        // timeout reap the grandchild too, rather than orphan it.
        .process_group(0)
        .spawn()
        .map_err(|e| format!("spawn {cmd}: {e}"))?;

    // Group leader's pid is the pgid; negate it to target the group.
    let pgid = child.id() as i32;

    let stdout_pipe = child.stdout.take().expect("stdout piped");
    let stderr_pipe = child.stderr.take().expect("stderr piped");
    let stdout_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::BufReader::new(stdout_pipe).read_to_string(&mut s);
        s
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::BufReader::new(stderr_pipe).read_to_string(&mut s);
        s
    });

    let start = Instant::now();
    // Exponential backoff polling: most fixtures finish in <50 ms so a
    // flat 20 ms sleep wastes ~15 ms per case on short ops. Start at
    // 0.5 ms (catches fast cases nearly idle-free), double up to a
    // 20 ms cap.
    let mut backoff_us: u64 = 500;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if start.elapsed() >= timeout {
                    // SIGKILL the *whole group* so the AOT-spawned
                    // native grandchild dies with tr rather than
                    // orphaning to init.
                    unsafe { kill(-pgid, SIGKILL) };
                    let _ = child.wait();
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    return Err(format!(
                        "{cmd} timeout after {}s (SIGKILL group)",
                        timeout.as_secs_f64()
                    ));
                }
                std::thread::sleep(Duration::from_micros(backoff_us));
                backoff_us = (backoff_us * 2).min(20_000);
            }
            Err(e) => return Err(format!("wait {cmd}: {e}")),
        }
    };

    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();

    if !status.success() {
        return Err(format!("{cmd} exited {}: {}", status, stderr.trim()));
    }
    Ok((stdout, stderr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_returns_stdout_for_quick_command() {
        let (out, _) = exec("echo", &["hello"]).expect("echo should succeed");
        assert_eq!(out, "hello\n");
    }

    #[test]
    fn exec_with_timeout_kills_hung_child_and_returns_timeout_error() {
        let start = Instant::now();
        let result = exec_with_timeout("sleep", &["30"], Duration::from_millis(200));
        let elapsed = start.elapsed();
        let err = result.expect_err("expected timeout error, got success");
        assert!(
            err.contains("timeout") && err.contains("SIGKILL"),
            "error should mention timeout + SIGKILL, got: {err}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "timeout path took {:?} — should be < 2 s (deadline 200 ms + cleanup)",
            elapsed
        );
    }

    #[test]
    fn exec_with_timeout_returns_non_zero_exit_normally() {
        let err = exec_with_timeout("false", &[], Duration::from_secs(5))
            .expect_err("false should yield non-zero exit error");
        assert!(
            err.contains("exited") && !err.contains("timeout"),
            "non-zero exit must not be mistaken for timeout: {err}"
        );
    }

    /// Timeout must reap the *grandchild* through process-group
    /// signalling. Without `process_group(0)` + `kill(-pgid)`, a
    /// hung `sh -c 'sleep 30'` grandchild kept running after the
    /// parent shell was SIGKILLed. This test spawns `sh` that
    /// forks a background sleep and waits — kill via pgid must
    /// return quickly *and* leave no live sleep descendant.
    #[test]
    fn timeout_reaps_grandchild_via_process_group() {
        let start = Instant::now();
        // `sh -c "sleep 30 & wait"` — sh forks sleep, then waits.
        // Killing only sh (Child::kill) would leave sleep an
        // orphan; group SIGKILL takes both.
        let _ = exec_with_timeout("sh", &["-c", "sleep 30 & wait"], Duration::from_millis(200));
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(3),
            "group SIGKILL should reap sh + sleep in <3s, got {elapsed:?}"
        );
    }
}
