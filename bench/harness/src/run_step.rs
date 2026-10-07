//! The run step of one cell: an untimed run whose output is checked,
//! then the timed runs.
//!
//! A comparator whose process dies to a signal (bun has been seen to
//! SIGTRAP at startup before the script runs) gets the whole step
//! again, up to `MAX_RETRIES` times, and every retry is recorded in
//! the row. torajs's own runners are never retried: a tr crash is a
//! tr bug and fails the cell. Exit codes, timeouts and wrong output
//! are never retried either.

use std::os::unix::process::ExitStatusExt;

use anyhow::Result;

use crate::bench::preview;
use crate::case::{Case, stdout_matches};
use crate::exec::{PER_EXEC_TIMEOUT, spawn_output_bounded};
use crate::outcome::{RunOutcome, Status};
use crate::runner::split_cmd;
use crate::rusage::{RunStats, measure_runs};

/// Retries after the first attempt, so at most three attempts.
pub(crate) const MAX_RETRIES: u32 = 2;

/// A child that ended by a signal rather than by exiting.
#[derive(Debug)]
pub(crate) struct KilledBySignal {
    pub(crate) signal: i32,
    pub(crate) detail: String,
}

impl KilledBySignal {
    /// From a raw `wait4` status whose low seven bits are nonzero.
    pub(crate) fn from_wait(program: &str, status: i32) -> Self {
        let signal = status & 0x7f;
        Self {
            signal,
            detail: format!("`{program}` killed by signal {signal} (wait status {status:#x})"),
        }
    }
}

impl std::fmt::Display for KilledBySignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for KilledBySignal {}

/// One attempt of the run step that died to a signal and was retried.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Retry {
    /// The interleaved pass this happened in (1-based).
    pub pass: usize,
    /// Which attempt died: 1 is the original run, 2 the first retry.
    pub attempt: u32,
    pub signal: i32,
    /// UTC time the death was seen.
    pub at: String,
    pub error: String,
}

/// The runners that build or run tr itself.
pub(crate) fn is_own_runtime(runner: &str) -> bool {
    runner == "torajs" || runner.starts_with("torajs-")
}

/// The signal to retry on, when attempt number `attempt` (1-based) of
/// `runner` failed with `err`; `None` when the failure stands.
pub(crate) fn retry_signal(runner: &str, attempt: u32, err: &anyhow::Error) -> Option<i32> {
    if is_own_runtime(runner) || attempt > MAX_RETRIES {
        return None;
    }
    err.downcast_ref::<KilledBySignal>().map(|k| k.signal)
}

struct StepError {
    message: String,
    cause: Option<anyhow::Error>,
}

impl StepError {
    fn caused(prefix: &str, e: anyhow::Error) -> Self {
        Self {
            message: format!("{prefix}: {e:#}"),
            cause: Some(e),
        }
    }
}

/// Runs the step for `runner` until it succeeds or its failure stands,
/// filling `outcome`.
pub(crate) fn run_cell(
    case: &Case,
    runner: &str,
    run_cmd: &str,
    pass: usize,
    first_pass: bool,
    outcome: &mut RunOutcome,
) {
    let mut attempt = 1;
    loop {
        let Err(err) = attempt_once(case, run_cmd, first_pass, outcome) else {
            return;
        };
        let signal = err
            .cause
            .as_ref()
            .and_then(|c| retry_signal(runner, attempt, c));
        let Some(signal) = signal else {
            outcome.status = Status::Failed;
            outcome.error = Some(err.message);
            return;
        };
        eprintln!(
            "  ↻ {} × {runner}: attempt {attempt} died to signal {signal}, retrying ({attempt}/{MAX_RETRIES})",
            case.name
        );
        outcome.retries.push(Retry {
            pass,
            attempt,
            signal,
            at: crate::report::now_iso8601(),
            error: err.message,
        });
        attempt += 1;
    }
}

fn attempt_once(
    case: &Case,
    run_cmd: &str,
    first_pass: bool,
    outcome: &mut RunOutcome,
) -> std::result::Result<(), StepError> {
    let actual = exec_capture(run_cmd).map_err(|e| StepError::caused("run failed", e))?;
    let matched = stdout_matches(case, &actual);
    outcome.stdout_match = Some(matched);
    if !matched {
        return Err(StepError {
            message: format!(
                "stdout mismatch: got {:?}, want {:?} (tolerance={})",
                preview(&actual),
                preview(&case.expected_stdout),
                case.tolerance
            ),
            cause: None,
        });
    }
    if first_pass {
        outcome.peak_threads = crate::threads::peak_threads(run_cmd, PER_EXEC_TIMEOUT);
    }
    let stats = measure_runs(
        run_cmd,
        case.run_warmup,
        case.run_runs,
        &[],
        PER_EXEC_TIMEOUT,
    )
    .map_err(|e| StepError::caused("timed run", e))?;
    fill(outcome, stats);
    Ok(())
}

fn fill(outcome: &mut RunOutcome, stats: RunStats) {
    outcome.run_ms = Some(stats.median_ms);
    outcome.run_stddev_ms = Some(stats.stddev_ms);
    outcome.run_min_ms = Some(stats.min_ms);
    outcome.run_samples_ms = stats.samples_ms;
    outcome.max_rss_kb = Some(stats.max_rss_kb);
    outcome.user_ms = stats.user_ms;
    outcome.system_ms = stats.system_ms;
    outcome.instructions = stats.instructions;
    outcome.cycles = stats.cycles;
    outcome.pinstructions = stats.pinstructions;
    outcome.pcycles = stats.pcycles;
    outcome.phys_footprint_kb = stats.phys_footprint_kb;
    outcome.pageins = stats.pageins;
    outcome.diskio_read_bytes = stats.diskio_read_bytes;
    outcome.diskio_written_bytes = stats.diskio_written_bytes;
    outcome.minor_faults = stats.minor_faults;
    outcome.major_faults = stats.major_faults;
    outcome.ctx_switches = stats.ctx_switches;
}

fn exec_capture(cmd: &str) -> Result<String> {
    let parts = split_cmd(cmd);
    anyhow::ensure!(!parts.is_empty(), "empty command");
    let args: Vec<&str> = parts[1..].iter().map(String::as_str).collect();
    let output = spawn_output_bounded(&parts[0], &args, &[], PER_EXEC_TIMEOUT)
        .map_err(|e| anyhow::anyhow!("spawning `{cmd}`: {e}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if let Some(signal) = output.status.signal() {
        return Err(KilledBySignal {
            signal,
            detail: format!("{} stderr={:?}", output.status, preview(&stderr)),
        }
        .into());
    }
    anyhow::ensure!(
        output.status.success(),
        "{} stderr={:?}",
        output.status,
        preview(&stderr)
    );
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sigtrap() -> anyhow::Error {
        KilledBySignal {
            signal: 5,
            detail: "killed".into(),
        }
        .into()
    }

    #[test]
    fn comparator_signal_is_retried_twice() {
        assert_eq!(retry_signal("bun-aot", 1, &sigtrap()), Some(5));
        assert_eq!(retry_signal("bun-aot-fast", 2, &sigtrap()), Some(5));
        assert_eq!(retry_signal("bun-aot", 3, &sigtrap()), None);
    }

    #[test]
    fn own_runtime_is_never_retried() {
        assert_eq!(retry_signal("torajs", 1, &sigtrap()), None);
        assert_eq!(retry_signal("torajs-run", 1, &sigtrap()), None);
        assert!(!is_own_runtime("rust"));
    }

    #[test]
    fn exit_code_and_timeout_are_not_retried() {
        let exit = anyhow::anyhow!("`x` exited with code 1");
        assert_eq!(retry_signal("bun-aot", 1, &exit), None);
        let err = measure_runs("false", 0, 1, &[], std::time::Duration::from_secs(5))
            .expect_err("false fails");
        assert_eq!(retry_signal("bun-aot", 1, &err), None);
        let late = measure_runs("sleep 2", 0, 1, &[], std::time::Duration::from_millis(300))
            .expect_err("times out");
        assert_eq!(retry_signal("bun-aot", 1, &late), None);
    }

    #[test]
    fn signal_death_is_typed_in_both_run_paths() {
        let cmd = "sh -c kill${IFS}-TRAP${IFS}$$";
        let err = exec_capture(cmd).expect_err("killed");
        assert_eq!(retry_signal("bun-aot", 1, &err), Some(5), "{err:#}");
        let err =
            measure_runs(cmd, 0, 1, &[], std::time::Duration::from_secs(5)).expect_err("killed");
        assert_eq!(retry_signal("bun-aot", 1, &err), Some(5), "{err:#}");
    }

    #[test]
    fn retry_serializes_its_fields() {
        let r = Retry {
            pass: 1,
            attempt: 2,
            signal: 5,
            at: "2026-10-07T00:00:00Z".into(),
            error: "run failed".into(),
        };
        let v = serde_json::to_value(&r).expect("json");
        assert_eq!(v["attempt"], 2);
        assert_eq!(v["signal"], 5);
        assert_eq!(v["pass"], 1);
    }
}
