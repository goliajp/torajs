mod bench;
mod case;
mod compare;
mod report;
mod runner;

use anyhow::{Context, Result};
use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn bench_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("harness has parent dir")
        .to_path_buf()
}

fn print_usage() {
    println!("bench-harness — torajs cross-runtime benchmark harness");
    println!();
    println!("USAGE:");
    println!("    bench-harness <COMMAND> [args]");
    println!();
    println!("COMMANDS:");
    println!("    list                          list cases and runners (with detected versions)");
    println!(
        "    run [case]                    run all cases (or one case) on all available runtimes"
    );
    println!("        --runtime r1,r2           filter to a comma-separated runtime list");
    println!(
        "        --runs N                  N full interleaved passes; row = median run_ms + MAD"
    );
    println!(
        "        --self                    per-commit fast path: torajs runtimes only (~3-4×;"
    );
    println!(
        "                                  phase-close must still run the full 8-runner matrix)"
    );
    println!(
        "        --vs <baseline.json>      artifact-precheck: skip timed runs if every torajs"
    );
    println!(
        "                                  artifact is byte-identical to baseline (else full run)"
    );
    println!("        --no-save                 don't write results/<file>.json");
    println!();
    println!(
        "    compare <base> <cur>          machine regression verdict (artifact_bytes hard gate +"
    );
    println!(
        "                                  noise-aware run_ms); exit 1 on unjustified regression"
    );
    println!(
        "        --allow-artifact-delta c:r  justify intended per-case artifact_bytes change(s)"
    );
    println!();
    println!("    --help, -h                    print this help");
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str);
    let bench_dir = bench_root();

    match cmd {
        Some("list") => match list(&bench_dir) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => fatal(e),
        },
        Some("run") => match run_cmd(&bench_dir, &args[1..]) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::from(1),
            Err(e) => fatal(e),
        },
        Some("compare") => match compare::compare(&args[1..]) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::from(1),
            Err(e) => fatal(e),
        },
        None | Some("--help") | Some("-h") => {
            print_usage();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("error: unknown command `{other}`");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn fatal(e: anyhow::Error) -> ExitCode {
    eprintln!("error: {e:#}");
    ExitCode::from(1)
}

fn list(bench_dir: &Path) -> Result<()> {
    let workspace = bench_dir.parent().context("bench_dir has no parent")?;
    let runners = runner::load_all(&bench_dir.join("runners"))?;
    let cases = case::discover_all(&bench_dir.join("cases"))?;

    println!("runners ({}):", runners.len());
    for r in &runners {
        match r.detect_version(workspace) {
            Some(v) => println!("  {:<10} {}", r.name, v.trim()),
            None => println!("  {:<10} (not installed)", r.name),
        }
    }
    println!();
    println!("cases ({}):", cases.len());
    for c in &cases {
        println!("  {}", c.name);
    }
    Ok(())
}

fn run_cmd(bench_dir: &Path, args: &[String]) -> Result<bool> {
    let mut case_filter: Option<String> = None;
    let mut runtime_filter: Option<Vec<String>> = None;
    let mut no_save = false;
    let mut runs: usize = 1;
    let mut self_only = false;
    let mut vs_baseline: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--no-save" => {
                no_save = true;
                i += 1;
            }
            "--self" => {
                self_only = true;
                i += 1;
            }
            "--vs" => {
                let v = args
                    .get(i + 1)
                    .context("--vs requires a baseline json path")?;
                vs_baseline = Some(v.clone());
                i += 2;
            }
            "--runs" => {
                let v = args
                    .get(i + 1)
                    .context("--runs requires a positive integer")?;
                runs = v
                    .parse::<usize>()
                    .ok()
                    .filter(|&n| n >= 1)
                    .context("--runs requires a positive integer")?;
                i += 2;
            }
            "--runtime" => {
                let v = args.get(i + 1).context("--runtime requires a value")?;
                runtime_filter = Some(v.split(',').map(String::from).collect());
                i += 2;
            }
            s if s.starts_with("--") => anyhow::bail!("unknown flag: {s}"),
            s => {
                if case_filter.is_some() {
                    anyhow::bail!("only one case name is supported per run");
                }
                case_filter = Some(s.to_string());
                i += 1;
            }
        }
    }

    let runners_all = runner::load_all(&bench_dir.join("runners"))?;
    let cases_all = case::discover_all(&bench_dir.join("cases"))?;

    let cases: Vec<_> = match &case_filter {
        Some(name) => cases_all.into_iter().filter(|c| &c.name == name).collect(),
        None => cases_all,
    };
    if cases.is_empty() {
        anyhow::bail!(
            "no cases match{}",
            case_filter.map(|n| format!(" `{n}`")).unwrap_or_default()
        );
    }

    // hardev bench B2 — `--self` is the per-commit fast path: only the
    // torajs runtimes (drop bun/node/go/rust/python — those are the
    // SOTA cross-runtime comparison, a phase-close concern, not a
    // per-commit regression gate). ~3–4× faster per-commit. Does NOT
    // reduce coverage: the regression target is torajs vs its own
    // baseline; phase-close still runs the full 8-runner matrix
    // (first hard rule). An explicit `--runtime` always wins.
    if self_only && runtime_filter.is_none() {
        runtime_filter = Some(vec!["torajs".to_string(), "torajs-run".to_string()]);
        eprintln!(
            "→ hardev B2: --self per-commit scope (torajs only; \
             phase-close must run the full 8-runner matrix)"
        );
    }

    let runners: Vec<_> = match &runtime_filter {
        Some(filt) => runners_all
            .into_iter()
            .filter(|r| filt.contains(&r.name))
            .collect(),
        None => runners_all,
    };
    if runners.is_empty() {
        anyhow::bail!("no runners match the filter");
    }

    let workspace = bench_dir.parent().context("bench_dir has no parent")?;

    // hardev bench B0 — bench MUST measure the real fat-LTO ship
    // binary. Since hardev devperf #1, the conformance runner builds
    // `target/iter/tr` (fast profile), so a bench run can no longer
    // assume `cargo run -p torajs-conformance` left a fresh
    // `target/release/tr` behind. The runner descriptors hardcode
    // `target/release/tr`; running against a stale/missing one would
    // silently measure the wrong thing (violates the first hard
    // rule). Force a release build up front — idempotent (cargo
    // no-ops in ~0.05 s when fresh, rebuilds when stale), so bench
    // always times the current ship binary.
    ensure_release_tr(workspace)?;

    let work_dir = env::temp_dir().join(format!("torajs-bench-{}", std::process::id()));
    std::fs::create_dir_all(&work_dir).context("creating work_dir")?;

    // hardev bench B2b — artifact-precheck. If every selected case's
    // torajs artifact_bytes is byte-identical to the baseline, the
    // machine code is unchanged ⇒ no perf regression is physically
    // possible ⇒ skip the (minutes-long) timed runs entirely
    // (seconds). The instant ANY artifact differs / is unknown we
    // fall through to the full timed measurement, so coverage is
    // never reduced (first hard rule). Safe by construction.
    if let Some(base_path) = &vs_baseline {
        let Some(tr_runner) = runners.iter().find(|r| r.name == "torajs") else {
            anyhow::bail!(
                "--vs needs the artifact-producing `torajs` runner in scope \
                 (drop a conflicting --runtime, or add --self)"
            );
        };
        let base = compare::load_artifacts(base_path)?;
        let mut identical = 0usize;
        let mut changed: Vec<(String, u64, u64)> = Vec::new();
        let mut unknown: Vec<String> = Vec::new();
        for c in &cases {
            let cur = bench::artifact_only(c, tr_runner, &work_dir, workspace)?;
            match (
                cur.artifact_bytes,
                base.get(&(c.name.clone(), "torajs".to_string()))
                    .copied()
                    .flatten(),
            ) {
                (Some(cb), Some(bb)) if cb == bb => identical += 1,
                (Some(cb), Some(bb)) => changed.push((c.name.clone(), bb, cb)),
                _ => unknown.push(c.name.clone()),
            }
        }
        if changed.is_empty() && unknown.is_empty() && identical > 0 {
            println!(
                "hardev B2b artifact-precheck: all {identical} torajs artifact(s) \
                 byte-identical to {base_path}\n  → machine code unchanged → \
                 0 perf regression by construction → timed runs SKIPPED."
            );
            let _ = std::fs::remove_dir_all(&work_dir);
            return Ok(true);
        }
        eprintln!(
            "→ hardev B2b precheck: {identical} identical, {} changed, {} unknown \
             → artifact(s) differ/unknown, falling back to FULL timed measurement \
             (coverage preserved)",
            changed.len(),
            unknown.len()
        );
        for (case, bb, cb) in &changed {
            eprintln!(
                "    changed: {case}  {bb} → {cb} ({:+})",
                *cb as i64 - *bb as i64
            );
        }
        for case in &unknown {
            eprintln!("    unknown: {case} (not in baseline or compile skipped/failed)");
        }
    }

    let mut report = report::Report::new(bench_dir)?;
    report.runs = runs;
    let mut all_ok = true;

    // hardev bench B1b — N full interleaved passes (full case×runner
    // matrix per pass, repeated `runs` times) so the median samples
    // machine-state variance ACROSS time (matches the historical
    // "3 full-suite runs" intent), not N back-to-back runs of one
    // cell. One aggregated row per cell → one statistically-sound
    // json, no same-name overwrite, no log-parsing.
    let nr = runners.len();
    let mut acc: Vec<Vec<bench::RunOutcome>> = (0..cases.len() * nr).map(|_| Vec::new()).collect();
    for pass in 1..=runs {
        for (ci, c) in cases.iter().enumerate() {
            for (ri, r) in runners.iter().enumerate() {
                if runs > 1 {
                    eprintln!("→ {} × {} (pass {pass}/{runs})", c.name, r.name);
                } else {
                    eprintln!("→ {} × {}", c.name, r.name);
                }
                acc[ci * nr + ri].push(bench::run_one(c, r, &work_dir, workspace)?);
            }
        }
    }
    for (ci, _c) in cases.iter().enumerate() {
        for ri in 0..nr {
            let passes = std::mem::take(&mut acc[ci * nr + ri]);
            let outcome = if passes.len() > 1 {
                report::aggregate(passes)
            } else {
                passes.into_iter().next().expect("at least one pass")
            };
            if !outcome.is_ok() {
                all_ok = false;
            }
            report.push(outcome);
        }
    }

    report.print_table();
    if !no_save {
        let path = report.write_json(&bench_dir.join("results"))?;
        eprintln!("results: {}", path.display());
    }

    let _ = std::fs::remove_dir_all(&work_dir);
    Ok(all_ok)
}

/// hardev bench B0 — guarantee `target/release/tr` is the *current*
/// fat-LTO ship binary before any timing run. Idempotent: cargo
/// no-ops (~0.05 s) when fresh, rebuilds when stale. Fail-fast on a
/// build error — a loud build failure beats silently benchmarking a
/// stale or missing binary (first hard rule: bench must measure the
/// real ship artifact, full coverage/correctness).
fn ensure_release_tr(workspace: &Path) -> Result<()> {
    eprintln!(
        "→ hardev B0: cargo build --release -p torajs-cli (ensure target/release/tr is current)"
    );
    let status = std::process::Command::new("cargo")
        .args(["build", "--release", "-p", "torajs-cli"])
        .current_dir(workspace)
        .status()
        .context("spawning `cargo build --release -p torajs-cli`")?;
    if !status.success() {
        anyhow::bail!(
            "release build of torajs-cli failed — refusing to benchmark a stale/missing target/release/tr"
        );
    }
    let tr = workspace.join("target/release/tr");
    if !tr.is_file() {
        anyhow::bail!(
            "release build reported success but {} is missing",
            tr.display()
        );
    }
    Ok(())
}
