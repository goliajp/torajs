use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

pub use crate::aggregate::aggregate;
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
    /// Seed of the per-pass shuffle of (case, runner) order, so a run
    /// can be replayed in the same order.
    pub pass_order_seed: u64,
    /// The fixed C `gcd1m` binary timed once at the start and once at
    /// the end of every pass. A pass whose two readings differ by more
    /// than 3 % ran on a machine that changed under it.
    pub calibration: Vec<Calibration>,
    pub rows: Vec<RunOutcome>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Calibration {
    pub pass: usize,
    pub start_ms: f64,
    pub end_ms: f64,
    pub start_instructions: Option<u64>,
    pub end_instructions: Option<u64>,
    pub unstable: bool,
}

impl Report {
    pub fn new(bench_dir: &Path, head_sha: Option<&str>) -> Result<Self> {
        Ok(Self {
            started_at: now_iso8601(),
            host: hostname(),
            git_sha: git_sha(bench_dir, head_sha),
            runs: 1,
            pass_order_seed: 0,
            calibration: Vec::new(),
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
        // Truncating to 7 is for readability, but the `-dirty` marker
        // is the part a reader most needs — cutting it off leaves a
        // filename that names a commit the file is not a measurement
        // of, which is the whole defect this marker exists to stop.
        let sha_short = self
            .git_sha
            .as_deref()
            .map(|s| match s.strip_suffix("-dirty") {
                Some(base) => format!("{}-dirty", &base[..base.len().min(7)]),
                None => s[..s.len().min(7)].to_string(),
            })
            .unwrap_or_else(|| "nogit".to_string());
        let base = format!("{date}-{host_short}-{sha_short}");
        // First run gets `<base>.json`; re-runs at the same SHA append
        // `-r2`, `-r3`, ... so 3-run median workflows keep all results
        // instead of clobbering the prior file.
        let mut path = results_dir.join(format!("{base}.json"));
        if path.exists() {
            for n in 2..=999 {
                let cand = results_dir.join(format!("{base}-r{n}.json"));
                if !cand.exists() {
                    path = cand;
                    break;
                }
            }
        }
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

/// What produced the binaries this run timed.
///
/// `git rev-parse HEAD` answers "what did this directory check out",
/// which is a different question, and on the bench runner the two
/// diverge: it receives source by rsync rather than by pull, so its
/// HEAD names a commit from several rotations back while the tree
/// under it is current. A result file stamped that way claims to be a
/// measurement of code it was not a measurement of, and nothing
/// downstream can tell. The same defect was fixed in the test262
/// runner; this is the bench half of it.
///
/// So: an explicit `--head-sha` from a caller that knows the real
/// value wins, and otherwise a dirty tree is stamped `<sha>-dirty` —
/// HEAD does not name this source, and saying so is better than a
/// plausible commit id.
fn git_sha(bench_dir: &Path, override_sha: Option<&str>) -> Option<String> {
    if let Some(s) = override_sha {
        return Some(s.to_string());
    }
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(bench_dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(bench_dir)
        .output()
        .ok()
        .is_some_and(|o| o.status.success() && !o.stdout.is_empty());
    Some(if dirty { format!("{sha}-dirty") } else { sha })
}
