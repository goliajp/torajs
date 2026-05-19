mod bench;
mod case;
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
    println!("        --no-save                 don't write results/<file>.json");
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

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--no-save" => {
                no_save = true;
                i += 1;
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

    let mut report = report::Report::new(bench_dir)?;
    let mut all_ok = true;

    for c in &cases {
        for r in &runners {
            eprintln!("→ {} × {}", c.name, r.name);
            let outcome = bench::run_one(c, r, &work_dir, workspace)?;
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
