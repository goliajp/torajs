//! One hyperfine invocation: warmup + runs of a command, read back
//! through hyperfine's JSON export.

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::bench::preview;
use crate::exec::{PER_EXEC_TIMEOUT, spawn_output_bounded};

#[derive(Debug, Clone, Copy)]
pub(crate) struct Stats {
    pub(crate) median_ms: f64,
    pub(crate) stddev_ms: f64,
    /// Largest peak RSS any measured run reached; `None` when hyperfine
    /// did not report memory.
    pub(crate) max_rss_kb: Option<u64>,
}

#[derive(Deserialize)]
struct HyperfineExport {
    results: Vec<HyperfineRun>,
}

#[derive(Deserialize)]
struct HyperfineRun {
    median: f64, // seconds
    stddev: f64, // seconds
    /// Peak RSS per measured run, from the child's rusage (hyperfine
    /// 1.17+; absent in older exports).
    #[serde(default)]
    memory_usage_byte: Vec<u64>,
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
    Ok(Stats {
        median_ms: r.median * 1000.0,
        stddev_ms: r.stddev * 1000.0,
        max_rss_kb: r.memory_usage_byte.iter().max().map(|b| b / 1024),
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
