//! One hyperfine invocation for the compile step: warmup + runs of a
//! command, read back through hyperfine's JSON export.

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::bench::preview;
use crate::exec::{PER_EXEC_TIMEOUT, spawn_output_bounded};

/// Only the compile step still goes through hyperfine; the timed run
/// is measured in `rusage.rs`, which keeps the counters hyperfine drops.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stats {
    pub(crate) median_ms: f64,
}

#[derive(Deserialize)]
struct HyperfineExport {
    results: Vec<HyperfineRun>,
}

/// hyperfine 1.x puts `median` (seconds) on the result; 2.x moves it
/// into `summary.time_wall_clock`.
#[derive(Deserialize)]
struct HyperfineRun {
    median: Option<f64>,
    summary: Option<Summary>,
}

#[derive(Deserialize)]
struct Summary {
    time_wall_clock: WallClock,
}

#[derive(Deserialize)]
struct WallClock {
    median: f64, // seconds
}

impl HyperfineRun {
    fn median_s(&self) -> Option<f64> {
        self.median
            .or_else(|| self.summary.as_ref().map(|s| s.time_wall_clock.median))
    }
}

pub(crate) fn hyperfine_one(
    cmd: &str,
    warmup: u32,
    runs: u32,
    env: &[(String, String)],
) -> Result<Stats> {
    let tmp = std::env::temp_dir().join(format!(
        "hyperfine-{}-{}.json",
        std::process::id(),
        rand_suffix()
    ));
    // hyperfine `--shell=none` execs tr directly, so tr's `torajs-run-new-*`
    // grandchild lives inside hyperfine's process group. Route the spawn
    // through `spawn_output_bounded` so a hung fixture SIGKILLs the whole
    // group instead of wedging the bench run + orphaning grandchild to init
    // (matches 5eb3ce87 / e02a3807 / 7ba70a7e pgid audit family).
    let tmp_str = tmp.to_string_lossy().into_owned();
    let warmup_str = warmup.to_string();
    let runs_str = runs.to_string();
    let args: [&str; 11] = [
        "--warmup",
        &warmup_str,
        "--runs",
        &runs_str,
        "--export-json",
        &tmp_str,
        "--style",
        "none",
        "--shell=none",
        "--",
        cmd,
    ];
    let output = spawn_output_bounded("hyperfine", &args, env, PER_EXEC_TIMEOUT)
        .map_err(|e| anyhow::anyhow!("spawning hyperfine: {e}"))?;
    anyhow::ensure!(
        output.status.success(),
        "hyperfine exited {} stderr={:?}",
        output.status,
        preview(&String::from_utf8_lossy(&output.stderr))
    );
    let text =
        std::fs::read_to_string(&tmp).with_context(|| format!("reading {}", tmp.display()))?;
    let _ = std::fs::remove_file(&tmp);
    let parsed: HyperfineExport = serde_json::from_str(&text)?;
    let r = parsed
        .results
        .first()
        .context("hyperfine returned no results")?;
    let median = r
        .median_s()
        .context("hyperfine result carries no wall-clock median")?;
    Ok(Stats {
        median_ms: median * 1000.0,
    })
}

fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

#[cfg(test)]
mod tests {
    use super::HyperfineExport;

    fn median(json: &str) -> Option<f64> {
        let parsed: HyperfineExport = serde_json::from_str(json).unwrap();
        parsed.results[0].median_s()
    }

    #[test]
    fn reads_the_1x_median() {
        assert_eq!(
            median(r#"{"results":[{"median":0.25,"mean":0.3}]}"#),
            Some(0.25)
        );
    }

    #[test]
    fn reads_the_2x_wall_clock_median() {
        let json = r#"{"schema_version":"1","results":[{"command":"true",
            "summary":{"time_wall_clock":{"unit":"second","median":0.004,"mean":0.005},
            "instructions":{"median":6.0}}}]}"#;
        assert_eq!(median(json), Some(0.004));
    }

    #[test]
    fn a_result_without_either_is_none() {
        assert_eq!(median(r#"{"results":[{"command":"true"}]}"#), None);
    }
}
