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
    pub rows: Vec<RunOutcome>,
}

impl Report {
    pub fn new(bench_dir: &Path) -> Result<Self> {
        Ok(Self {
            started_at: now_iso8601(),
            host: hostname(),
            git_sha: git_sha(bench_dir),
            rows: Vec::new(),
        })
    }

    pub fn push(&mut self, outcome: RunOutcome) {
        self.rows.push(outcome);
    }

    pub fn print_table(&self) {
        println!();
        println!(
            "{:<22} {:<10} {:>12} {:>12} {:>10} {:<8}",
            "case", "runtime", "compile_ms", "run_ms", "stddev", "status"
        );
        println!("{}", "-".repeat(76));
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
            let status = match r.status {
                Status::Ok => "ok",
                Status::Skipped => "skip",
                Status::Failed => "fail",
            };
            println!(
                "{:<22} {:<10} {:>12} {:>12} {:>10} {:<8}",
                r.case, r.runtime, cms, rms, std, status
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
