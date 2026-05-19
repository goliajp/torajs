use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::bench::{RunOutcome, Status};

#[derive(Debug, Serialize)]
pub struct Report {
    pub started_at: String,
    pub host: String,
    pub git_sha: Option<String>,
    /// hardev bench B1b — how many full passes were aggregated into
    /// each row. 1 = single pass (`run_stddev_ms` is the within-pass
    /// hyperfine stddev). >1 = `--runs N`: `run_ms` is the **median**
    /// across the N passes and `run_stddev_ms` is the **MAD** (median
    /// absolute deviation, robust to mac single-point spikes). A
    /// reader / `bench compare` keys off this to interpret the spread.
    pub runs: usize,
    pub rows: Vec<RunOutcome>,
}

impl Report {
    pub fn new(bench_dir: &Path) -> Result<Self> {
        Ok(Self {
            started_at: now_iso8601(),
            host: hostname(),
            git_sha: git_sha(bench_dir),
            runs: 1,
            rows: Vec::new(),
        })
    }

    pub fn push(&mut self, outcome: RunOutcome) {
        self.rows.push(outcome);
    }

    pub fn print_table(&self) {
        println!();
        println!(
            "{:<22} {:<14} {:>12} {:>12} {:>10} {:>11} {:<8}",
            "case", "runtime", "compile_ms", "run_ms", "stddev", "size", "status"
        );
        println!("{}", "-".repeat(92));
        for r in &self.rows {
            let cms = r
                .compile_ms
                .map(|v| format!("{v:.2}"))
                .unwrap_or_else(|| "—".into());
            let rms = r
                .run_ms
                .map(|v| format!("{v:.2}"))
                .unwrap_or_else(|| "—".into());
            let std = r
                .run_stddev_ms
                .map(|v| format!("{v:.2}"))
                .unwrap_or_else(|| "—".into());
            let size = r
                .artifact_bytes
                .map(format_bytes)
                .unwrap_or_else(|| "—".into());
            let status = match r.status {
                Status::Ok => "ok",
                Status::Skipped => "skip",
                Status::Failed => "fail",
            };
            println!(
                "{:<22} {:<14} {:>12} {:>12} {:>10} {:>11} {:<8}",
                r.case, r.runtime, cms, rms, std, size, status
            );
        }
        // print errors / skip reasons below the table
        let with_msg: Vec<_> = self.rows.iter().filter(|r| r.error.is_some()).collect();
        if !with_msg.is_empty() {
            println!();
            for r in with_msg {
                if let Some(msg) = &r.error {
                    println!("  {} × {}: {}", r.case, r.runtime, msg);
                }
            }
        }
        println!();
    }

    pub fn write_json(&self, results_dir: &Path) -> Result<PathBuf> {
        std::fs::create_dir_all(results_dir)
            .with_context(|| format!("creating {}", results_dir.display()))?;
        let date = self.started_at.split('T').next().unwrap_or("undated");
        let host_short = self.host.split('.').next().unwrap_or(&self.host);
        let sha_short = self
            .git_sha
            .as_deref()
            .map(|s| &s[..s.len().min(7)])
            .unwrap_or("nogit");
        let filename = format!("{date}-{host_short}-{sha_short}.json");
        let path = results_dir.join(&filename);
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    }
}

fn format_bytes(b: u64) -> String {
    if b >= 1_000_000 {
        format!("{:.2} MB", b as f64 / 1_000_000.0)
    } else if b >= 1_000 {
        format!("{:.1} KB", b as f64 / 1_000.0)
    } else {
        format!("{b} B")
    }
}

/// hardev bench B1b — median of a slice (avg of the two middle values
/// for even length). `None` for empty input.
fn median(xs: &[f64]) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    })
}

/// Median absolute deviation — robust spread, far less spike-sensitive
/// than stddev (a single mac thermal/load spike barely moves it).
fn mad(xs: &[f64], med: f64) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    let dev: Vec<f64> = xs.iter().map(|x| (x - med).abs()).collect();
    median(&dev)
}

/// hardev bench B1b — fold N passes of the SAME (case, runtime) into
/// one row: `run_ms` = median across passes, `run_stddev_ms` = MAD
/// (robust spread), `compile_ms` = median, `artifact_bytes` = the
/// shared value if all identical else the median (the small ±N-byte
/// drift seen run-to-run is benign linker nondeterminism, already
/// handled conservatively by `bench compare`). `status` is the worst
/// observed (Failed > Skipped > Ok) so a single failing pass is never
/// hidden by aggregation. Caller guarantees `outcomes` is non-empty
/// and homogeneous in (case, runtime).
pub fn aggregate(outcomes: Vec<RunOutcome>) -> RunOutcome {
    let mut base = outcomes[0].clone();

    base.status = if outcomes.iter().any(|o| o.status == Status::Failed) {
        Status::Failed
    } else if outcomes.iter().any(|o| o.status == Status::Skipped) {
        Status::Skipped
    } else {
        Status::Ok
    };

    let run: Vec<f64> = outcomes.iter().filter_map(|o| o.run_ms).collect();
    base.run_ms = median(&run);
    base.run_stddev_ms = base.run_ms.and_then(|m| mad(&run, m));

    let comp: Vec<f64> = outcomes.iter().filter_map(|o| o.compile_ms).collect();
    base.compile_ms = median(&comp);

    let arts: Vec<u64> = outcomes.iter().filter_map(|o| o.artifact_bytes).collect();
    base.artifact_bytes = if arts.is_empty() {
        None
    } else if arts.iter().all(|&a| a == arts[0]) {
        Some(arts[0])
    } else {
        let af: Vec<f64> = arts.iter().map(|&a| a as f64).collect();
        median(&af).map(|m| m.round() as u64)
    };

    // surface the first error if the aggregate failed; clear it otherwise
    base.error = if base.status == Status::Failed {
        outcomes.iter().find_map(|o| o.error.clone())
    } else {
        None
    };
    base
}

fn now_iso8601() -> String {
    Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".into())
}

fn hostname() -> String {
    Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".into())
}

fn git_sha(bench_dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(bench_dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
