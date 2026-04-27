use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

use crate::case::Case;
use crate::runner::{Runner, TemplateContext, split_cmd};

#[derive(Debug, Clone, serde::Serialize)]
pub struct RunOutcome {
    pub case: String,
    pub runtime: String,
    pub runtime_version: Option<String>,
    pub status: Status,
    pub compile_ms: Option<f64>,
    pub run_ms: Option<f64>,
    pub run_stddev_ms: Option<f64>,
    pub stdout_match: Option<bool>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Skipped,
    Failed,
}

impl RunOutcome {
    fn skip(case: &str, runtime: &str, reason: String) -> Self {
        Self {
            case: case.into(),
            runtime: runtime.into(),
            runtime_version: None,
            status: Status::Skipped,
            compile_ms: None,
            run_ms: None,
            run_stddev_ms: None,
            stdout_match: None,
            error: Some(reason),
        }
    }

    pub fn is_ok(&self) -> bool {
        self.status != Status::Failed
    }
}

pub fn run_one(
    case: &Case,
    runner: &Runner,
    work_dir: &Path,
    workspace: &Path,
) -> Result<RunOutcome> {
    let src_path = case.dir.join(&runner.src_filename);
    if !src_path.exists() {
        return Ok(RunOutcome::skip(
            &case.name,
            &runner.name,
            format!("no source file: {}", src_path.display()),
        ));
    }

    let version = match runner.detect_version(workspace) {
        Some(v) => v,
        None => {
            return Ok(RunOutcome::skip(
                &case.name,
                &runner.name,
                format!("{} not detected", runner.name),
            ));
        }
    };

    let out_path = work_dir.join(format!("{}-{}", case.name, runner.name));
    let ctx = TemplateContext {
        src: src_path.to_string_lossy().to_string(),
        out: out_path.to_string_lossy().to_string(),
        case: case.name.clone(),
        workspace: workspace.to_string_lossy().to_string(),
    };

    let mut outcome = RunOutcome {
        case: case.name.clone(),
        runtime: runner.name.clone(),
        runtime_version: Some(version),
        status: Status::Ok,
        compile_ms: None,
        run_ms: None,
        run_stddev_ms: None,
        stdout_match: None,
        error: None,
    };

    if let Some(compile_template) = &runner.compile {
        let compile_cmd = ctx.substitute(compile_template);
        match exec_capture_status(&compile_cmd) {
            Ok(()) => {}
            Err(CompileError::NotYetImplemented(stderr)) => {
                // Runner exists but doesn't support this case yet (e.g. tr's
                // AOT pass during P3.x ramp). Treat as skip, not fail.
                outcome.status = Status::Skipped;
                outcome.error = Some(stderr);
                return Ok(outcome);
            }
            Err(CompileError::Real(msg)) => {
                outcome.status = Status::Failed;
                outcome.error = Some(format!("compile failed: {msg}"));
                return Ok(outcome);
            }
        }
        match hyperfine_one(&compile_cmd, case.compile_warmup, case.compile_runs) {
            Ok(stats) => outcome.compile_ms = Some(stats.median_ms),
            Err(e) => {
                outcome.status = Status::Failed;
                outcome.error = Some(format!("hyperfine compile: {e:#}"));
                return Ok(outcome);
            }
        }
    }

    let run_cmd = ctx.substitute(&runner.run);

    let actual_stdout = match exec_capture(&run_cmd) {
        Ok(s) => s,
        Err(e) => {
            outcome.status = Status::Failed;
            outcome.error = Some(format!("run failed: {e:#}"));
            return Ok(outcome);
        }
    };
    outcome.stdout_match = Some(crate::case::stdout_matches(case, &actual_stdout));
    if !outcome.stdout_match.unwrap() {
        outcome.status = Status::Failed;
        outcome.error = Some(format!(
            "stdout mismatch: got {:?}, want {:?} (tolerance={})",
            preview(&actual_stdout),
            preview(&case.expected_stdout),
            case.tolerance
        ));
        return Ok(outcome);
    }

    match hyperfine_one(&run_cmd, case.run_warmup, case.run_runs) {
        Ok(stats) => {
            outcome.run_ms = Some(stats.median_ms);
            outcome.run_stddev_ms = Some(stats.stddev_ms);
        }
        Err(e) => {
            outcome.status = Status::Failed;
            outcome.error = Some(format!("hyperfine run: {e:#}"));
        }
    }

    Ok(outcome)
}

/// Compile-step error categorized by exit code.
///
/// Exit code 3 is reserved as "not yet implemented" — the runner exists and
/// recognized the program as well-formed, but its compile pipeline doesn't
/// support this shape yet (cf. `tr build` during P3.x ramp). We treat these
/// as skip, not fail, so the scoreboard doesn't show torajs in red while the
/// AOT path is being grown feature by feature.
enum CompileError {
    NotYetImplemented(String),
    Real(String),
}

fn exec_capture_status(cmd: &str) -> std::result::Result<(), CompileError> {
    let parts = split_cmd(cmd);
    if parts.is_empty() {
        return Err(CompileError::Real("empty command".into()));
    }
    let output = match Command::new(&parts[0]).args(&parts[1..]).output() {
        Ok(o) => o,
        Err(e) => return Err(CompileError::Real(format!("spawning `{cmd}`: {e}"))),
    };
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.code() == Some(3) {
        Err(CompileError::NotYetImplemented(stderr))
    } else {
        Err(CompileError::Real(format!(
            "{} stderr={:?}",
            output.status,
            preview(&stderr)
        )))
    }
}

fn exec_capture(cmd: &str) -> Result<String> {
    let parts = split_cmd(cmd);
    anyhow::ensure!(!parts.is_empty(), "empty command");
    let output = Command::new(&parts[0])
        .args(&parts[1..])
        .output()
        .with_context(|| format!("spawning `{cmd}`"))?;
    anyhow::ensure!(
        output.status.success(),
        "{} stderr={:?}",
        output.status,
        preview(&String::from_utf8_lossy(&output.stderr))
    );
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[derive(Debug, Clone, Copy)]
struct Stats {
    median_ms: f64,
    stddev_ms: f64,
}

#[derive(Deserialize)]
struct HyperfineExport {
    results: Vec<HyperfineRun>,
}

#[derive(Deserialize)]
struct HyperfineRun {
    median: f64, // seconds
    stddev: f64, // seconds
}

fn hyperfine_one(cmd: &str, warmup: u32, runs: u32) -> Result<Stats> {
    let tmp = std::env::temp_dir().join(format!(
        "hyperfine-{}-{}.json",
        std::process::id(),
        rand_suffix()
    ));
    let status = Command::new("hyperfine")
        .arg("--warmup")
        .arg(warmup.to_string())
        .arg("--runs")
        .arg(runs.to_string())
        .arg("--export-json")
        .arg(&tmp)
        .arg("--style")
        .arg("none")
        .arg("--shell=none")
        .arg("--")
        .arg(cmd)
        .status()
        .context("spawning hyperfine")?;
    anyhow::ensure!(status.success(), "hyperfine exited {status}");
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
    })
}

fn preview(s: &str) -> String {
    if s.len() > 80 {
        format!("{}…", &s[..80])
    } else {
        s.to_string()
    }
}

fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}
