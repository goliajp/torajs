use anyhow::Result;
use std::path::Path;

use crate::case::Case;
use crate::exec::{PER_EXEC_TIMEOUT, spawn_output_bounded};
use crate::hyperfine::hyperfine_one;
pub use crate::outcome::{RunOutcome, Status};
use crate::runner::{Runner, TemplateContext, split_cmd};
use crate::rusage::measure_runs;

/// The run command with the case's arguments appended.
fn with_args(cmd: &str, args: &[String]) -> String {
    if args.is_empty() {
        return cmd.to_string();
    }
    format!("{cmd} {}", args.join(" "))
}

/// Artifact file name padded to one length: the path is in the
/// child's argv, and argv length moves the initial stack, which moves
/// the measurement (Mytkowicz 2009). Every cell gets the same offset.
const OUT_NAME_LEN: usize = 56;

pub(crate) fn out_name(case: &str, runner: &str) -> String {
    let mut name = format!("{case}-{runner}");
    assert!(name.len() <= OUT_NAME_LEN, "artifact name too long: {name}");
    while name.len() < OUT_NAME_LEN {
        name.push('_');
    }
    name
}

/// Hex-encoded SHA-256 of a file's contents, via the in-house
/// `torajs_link::sha256` (the codesign digest — no external dep).
fn file_sha256_hex(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let digest = torajs_link::sha256::sha256(&bytes);
    let mut hex = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
    }
    Some(hex)
}


pub fn run_one(
    case: &Case,
    runner: &Runner,
    work_dir: &Path,
    workspace: &Path,
    cached_compile_ms: Option<f64>,
) -> Result<RunOutcome> {
    let src_path = crate::runner::resolve_src(&case.dir, &runner.src_filename);
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

    let out_path = work_dir.join(out_name(&case.name, &runner.name));
    let ctx = TemplateContext {
        src: src_path.to_string_lossy().to_string(),
        out: out_path.to_string_lossy().to_string(),
        case: case.name.clone(),
        workspace: workspace.to_string_lossy().to_string(),
    };

    let mut outcome = RunOutcome::new(&case.name, &runner.name);
    outcome.runtime_version = Some(version);

    if let Some(compile_template) = &runner.compile {
        let compile_cmd = ctx.substitute(compile_template);
        let compile_env: Vec<(String, String)> = Vec::new();

        // Compile must run at least once per pass to produce the
        // artifact that the run-side timing executes. The compile_ms
        // METRIC, however, is deterministic across passes (source +
        // compiler unchanged within a single bench-harness invocation),
        // so on passes 2+ we skip hyperfine's repeated compile
        // measurements and just produce the artifact via one
        // `exec_capture_status` invocation, reusing the first-pass
        // compile_ms. Saves (warmup + runs) compile invocations per
        // (case, runner) per pass — the dominant savings on
        // `--runs N` runs (hardev bench B4).
        match exec_capture_status(&compile_cmd, &compile_env) {
            Ok(()) => {}
            Err(CompileError::NotYetImplemented(stderr)) => {
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
        if let Some(cached) = cached_compile_ms {
            outcome.compile_ms = Some(cached);
        } else {
            match hyperfine_one(
                &compile_cmd,
                case.compile_warmup,
                case.compile_runs,
                &compile_env,
            ) {
                Ok(stats) => outcome.compile_ms = Some(stats.median_ms),
                Err(e) => {
                    outcome.status = Status::Failed;
                    outcome.error = Some(format!("hyperfine compile: {e:#}"));
                    return Ok(outcome);
                }
            }
        }
        if cached_compile_ms.is_none()
            && let Some(switch) = &runner.phases_env
        {
            outcome.compile_phases_ms = crate::phases::compile_phases(&compile_cmd, switch);
        }
        // Compile produced an artifact at {out}; capture its byte size.
        if let Ok(meta) = std::fs::metadata(&out_path) {
            outcome.artifact_bytes = Some(meta.len());
            outcome.artifact_sha256 = file_sha256_hex(&out_path);
        }
        // CRITICAL — clean up `bun build --compile`'s scratch cache
        // files (`.HEX-NNNN.bun-build`). They land in cwd regardless
        // of --outfile, no env var redirects them. Across a full
        // `bench-harness run` they pile up by the thousand (one
        // saturated test session left 11,724 files / 688 GB on disk).
        // Any temp artifact a tool produces is cleaned up at the
        // same boundary that generated it. Best-effort glob — failures don't fail the
        // bench step.
        let _ = std::fs::read_dir(".").map(|entries| {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let s = name.to_string_lossy();
                if s.starts_with('.') && s.ends_with(".bun-build") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        });
    }

    let run_cmd = with_args(&ctx.substitute(&runner.run), &case.args);

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

    match measure_runs(
        &run_cmd,
        case.run_warmup,
        case.run_runs,
        &[],
        PER_EXEC_TIMEOUT,
    ) {
        Ok(stats) => {
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
        Err(e) => {
            outcome.status = Status::Failed;
            outcome.error = Some(format!("timed run: {e:#}"));
        }
    }

    Ok(outcome)
}

/// hardev bench B2b — artifact-only pass: compile ONCE (no hyperfine,
/// no timed run) and capture the produced binary's byte size. Used by
/// the `--vs` artifact-precheck: if every case's artifact_bytes is
/// byte-identical to the baseline, the machine code is unchanged, so
/// there can be no perf regression *by construction* and the
/// expensive timed run is safely skipped (seconds, not minutes).
/// Falling back to the full timed `run_one` whenever an artifact
/// differs keeps coverage intact (first hard rule). Only meaningful
/// for runners that produce an artifact (the `torajs` AOT runner);
/// returns `artifact_bytes: None` otherwise.
pub fn artifact_only(
    case: &Case,
    runner: &Runner,
    work_dir: &Path,
    workspace: &Path,
) -> Result<RunOutcome> {
    let src_path = crate::runner::resolve_src(&case.dir, &runner.src_filename);
    let mut outcome = RunOutcome::new(&case.name, &runner.name);
    if !src_path.exists() {
        outcome.status = Status::Skipped;
        outcome.error = Some(format!("no source file: {}", src_path.display()));
        return Ok(outcome);
    }
    let Some(compile_template) = &runner.compile else {
        outcome.status = Status::Skipped;
        outcome.error = Some("runner has no compile step (no artifact)".into());
        return Ok(outcome);
    };
    let out_path = work_dir.join(out_name(&case.name, &runner.name));
    let ctx = TemplateContext {
        src: src_path.to_string_lossy().to_string(),
        out: out_path.to_string_lossy().to_string(),
        case: case.name.clone(),
        workspace: workspace.to_string_lossy().to_string(),
    };
    let compile_cmd = ctx.substitute(compile_template);
    let compile_env: Vec<(String, String)> = Vec::new();
    match exec_capture_status(&compile_cmd, &compile_env) {
        Ok(()) => {}
        Err(CompileError::NotYetImplemented(stderr)) => {
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
    if let Ok(meta) = std::fs::metadata(&out_path) {
        outcome.artifact_bytes = Some(meta.len());
        outcome.artifact_sha256 = file_sha256_hex(&out_path);
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

fn exec_capture_status(
    cmd: &str,
    env: &[(String, String)],
) -> std::result::Result<(), CompileError> {
    let parts = split_cmd(cmd);
    if parts.is_empty() {
        return Err(CompileError::Real("empty command".into()));
    }
    let args: Vec<&str> = parts[1..].iter().map(String::as_str).collect();
    let output = match spawn_output_bounded(&parts[0], &args, env, PER_EXEC_TIMEOUT) {
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
    let args: Vec<&str> = parts[1..].iter().map(String::as_str).collect();
    let output = spawn_output_bounded(&parts[0], &args, &[], PER_EXEC_TIMEOUT)
        .map_err(|e| anyhow::anyhow!("spawning `{cmd}`: {e}"))?;
    anyhow::ensure!(
        output.status.success(),
        "{} stderr={:?}",
        output.status,
        preview(&String::from_utf8_lossy(&output.stderr))
    );
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub(crate) fn preview(s: &str) -> String {
    if s.len() > 80 {
        format!("{}…", &s[..80])
    } else {
        s.to_string()
    }
}
