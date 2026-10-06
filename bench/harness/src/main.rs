mod bench;
mod case;
mod compare;
mod exec;
mod hyperfine;
mod outcome;
mod passes;
mod report;
mod runner;
mod rusage;
mod score;
mod version_check;

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
    println!("    run [case ...]                run all cases (or the named ones)");
    println!(
        "                                  default runtimes: torajs · torajs-run · rust · rust-fast ·"
    );
    println!(
        "                                  c · c-fast · go · go-fast · bun-aot · bun-aot-fast ·"
    );
    println!("                                  bun-jsc · node-v8 · deno-aot · deno-v8");
    println!("        --all                     also include python (several times slower alone)");
    println!("        --runtime r1,r2           explicit comma-separated runtime list (overrides");
    println!("                                  both --all and the default set)");
    println!("        --head-sha SHA            stamp results with this source id (the runner's");
    println!("                                  own git HEAD names its CHECKOUT, a different");
    println!("                                  thing whenever the tree arrived by rsync)");
    println!(
        "        --runs N                  N full interleaved passes; row = median run_ms + MAD"
    );
    println!("        --seed N                  replay a run's per-pass shuffle order");
    println!(
        "        --self                    per-commit fast path: torajs runtimes only (~3-4×;"
    );
    println!("                                  phase-close must still run the default matrix)");
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
    println!(
        "    score <result.json>           per-category weighted geomean of torajs / comparator"
    );
    println!(
        "                                  run_ms, sourced from bench/categories.toml (catches"
    );
    println!("                                  category-coverage drift; flags unmapped cases)");
    println!();
    println!("    --help, -h                    print this help");
}

/// Default `bench-harness run` runtime set — the **AOT-mode-only**
/// comparison that directly informs L1 vision #1 ("perf 远超 bun"):
/// torajs (real-native AOT) vs bun-aot (bundle-with-JSC "AOT") vs
/// rust and c (native ceiling; every case has both). `torajs-run`
/// rides along as the dev-loop view (steady-state cache-hit; matches
/// `tr run` ergonomics).
///
/// The JS runtimes appear in both of their modes — standalone
/// executable (bun-aot, deno-aot) and JIT from source (bun-jsc,
/// node-v8, deno-v8) — so a reader can compare against whichever one
/// they would otherwise have used. python stays behind `--all`: it is
/// half of the full wall-clock on its own and not a peer.
///
/// Every compiled comparator is run in two tiers, its default build
/// and its fastest (`<name>-fast`: the same toolchain with every
/// codegen option a user could turn on), so a ratio against it can be
/// read against either and never rests on an untuned comparator.
const DEFAULT_RUNTIMES: &[&str] = &[
    "torajs",
    "torajs-run",
    "rust",
    "rust-fast",
    "c",
    "c-fast",
    "go",
    "go-fast",
    "bun-aot",
    "bun-aot-fast",
    "bun-jsc",
    "node-v8",
    "deno-aot",
    "deno-v8",
];

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
        Some("score") => match score::score(&args[1..]) {
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
    let mut case_filter: Vec<String> = Vec::new();
    let mut runtime_filter: Option<Vec<String>> = None;
    let mut no_save = false;
    let mut runs: usize = 1;
    let mut head_sha: Option<String> = None;
    let mut self_only = false;
    let mut all_runtimes = false;
    let mut vs_baseline: Option<String> = None;
    let mut seed: Option<u64> = None;

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
            "--all" => {
                all_runtimes = true;
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
            // The value that names the SOURCE, for a caller that
            // knows it. The runner's own `git rev-parse HEAD` answers
            // a different question whenever the tree did not arrive by
            // pull — see `report::git_sha`.
            "--head-sha" => {
                let v = args.get(i + 1).context("--head-sha requires a value")?;
                head_sha = Some(v.clone());
                i += 2;
                continue;
            }
            "--runtime" => {
                let v = args.get(i + 1).context("--runtime requires a value")?;
                runtime_filter = Some(v.split(',').map(String::from).collect());
                i += 2;
            }
            "--seed" => {
                let v = args.get(i + 1).context("--seed requires an integer")?;
                seed = Some(v.parse::<u64>().context("--seed requires an integer")?);
                i += 2;
            }
            s if s.starts_with("--") => anyhow::bail!("unknown flag: {s}"),
            s => {
                case_filter.push(s.to_string());
                i += 1;
            }
        }
    }

    let runners_all = runner::load_all(&bench_dir.join("runners"))?;
    let cases_all = case::discover_all(&bench_dir.join("cases"))?;
    // the calibration program is gcd1m's c source, so it takes gcd1m's args
    let calibration_args: Vec<String> = cases_all
        .iter()
        .find(|c| c.name == "gcd1m")
        .map(|c| c.args.clone())
        .unwrap_or_default();

    let cases: Vec<_> = if case_filter.is_empty() {
        cases_all
    } else {
        cases_all
            .into_iter()
            .filter(|c| case_filter.contains(&c.name))
            .collect()
    };
    if cases.is_empty() {
        anyhow::bail!("no cases match `{}`", case_filter.join(" "));
    }

    // hardev bench B2 — `--self` is the per-commit fast path: only the
    // torajs runtimes (drop bun/node/go/rust/python — those are the
    // SOTA cross-runtime comparison, a phase-close concern, not a
    // per-commit regression gate). ~3–4× faster per-commit. Does NOT
    // reduce coverage: the regression target is torajs vs its own
    // baseline; phase-close still runs the default matrix
    // (first hard rule). An explicit `--runtime` always wins.
    if self_only && runtime_filter.is_none() {
        runtime_filter = Some(vec!["torajs".to_string(), "torajs-run".to_string()]);
        eprintln!(
            "→ hardev B2: --self per-commit scope (torajs only; \
             phase-close must run the default matrix)"
        );
    }

    // `--all` adds python to the default set. Explicit `--runtime`
    // always wins over both; `--self` already pinned the filter above
    // so it skips this branch.
    if runtime_filter.is_none() {
        if all_runtimes {
            eprintln!("→ --all: every runner (default set plus python)");
        } else {
            runtime_filter = Some(DEFAULT_RUNTIMES.iter().map(|s| s.to_string()).collect());
            eprintln!(
                "→ default runtimes: {} (pass --all to add python)",
                DEFAULT_RUNTIMES.join(" · ")
            );
        }
    }

    let c_runner = runners_all.iter().find(|r| r.name == "c").cloned();
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

    // B-BENCH-VER — fail fast if any externally-provided runtime drifted
    // below its latest-stable floor. Policy: bench/latest-stable.toml.
    // In-tree runners (torajs / torajs-run) skip automatically — their
    // version IS git HEAD, not a downloaded toolchain. Bench numbers that
    // compare against an old node/bun/go/rust are self-deception; verify
    // before any timing.
    let floors = version_check::load_floors(&bench_dir.join("latest-stable.toml"))?;
    version_check::verify_versions(&runners, &floors, workspace)?;

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
    // torajs artifact content hash (SHA-256) is identical to the
    // baseline, the machine code is unchanged ⇒ no perf regression is
    // physically possible ⇒ skip the (minutes-long) timed runs
    // entirely (seconds). The instant ANY artifact differs / is
    // unknown we fall through to the full timed measurement, so
    // coverage is never reduced (first hard rule).
    //
    // The comparison MUST be on content hash, never on size alone:
    // Mach-O 16 KiB segment alignment absorbs small text-section
    // deltas, so equal size does NOT imply equal code (a size-only
    // precheck silently skipped timed runs across the Cluster E
    // inliner firing on 8 cases). Baselines written before
    // artifact_sha256 existed have no hash → those cases count as
    // unknown and force the timed run (re-record the baseline to
    // re-enable the fast path).
    if let Some(base_path) = &vs_baseline {
        let Some(tr_runner) = runners.iter().find(|r| r.name == "torajs") else {
            anyhow::bail!(
                "--vs needs the artifact-producing `torajs` runner in scope \
                 (drop a conflicting --runtime, or add --self)"
            );
        };
        let base = compare::load_artifacts(base_path)?;
        let mut identical = 0usize;
        let mut changed: Vec<String> = Vec::new();
        let mut unknown: Vec<String> = Vec::new();
        for c in &cases {
            let cur = bench::artifact_only(c, tr_runner, &work_dir, workspace)?;
            let base_hash = base
                .get(&(c.name.clone(), "torajs".to_string()))
                .cloned()
                .flatten();
            match (cur.artifact_sha256, base_hash) {
                (Some(ch), Some(bh)) if ch == bh => identical += 1,
                (Some(_), Some(_)) => changed.push(c.name.clone()),
                _ => unknown.push(c.name.clone()),
            }
        }
        if changed.is_empty() && unknown.is_empty() && identical > 0 {
            println!(
                "hardev B2b artifact-precheck: all {identical} torajs artifact(s) \
                 content-hash-identical to {base_path}\n  → machine code unchanged → \
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
        for case in &changed {
            eprintln!("    changed: {case} (content hash differs)");
        }
        for case in &unknown {
            eprintln!(
                "    unknown: {case} (no hash in baseline / not in baseline / \
                 compile skipped or failed)"
            );
        }
    }

    let mut report = report::Report::new(bench_dir, head_sha.as_deref())?;
    report.runs = runs;
    let seed = seed.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1)
    });
    report.pass_order_seed = seed;
    eprintln!("→ pass order seed {seed}");
    let calibration_bin = match &c_runner {
        Some(c) => passes::build_calibration(
            c,
            &bench_dir.join("cases/gcd1m/main.c"),
            &calibration_args,
            &work_dir,
        )?,
        None => None,
    };
    if calibration_bin.is_none() {
        eprintln!(
            "→ no calibration binary (c runner or its compiler missing); passes run unbracketed"
        );
    }
    let (rows, calibration) = passes::Passes {
        cases: &cases,
        runners: &runners,
        runs,
        work_dir: &work_dir,
        workspace,
        seed,
        calibration_bin,
    }
    .run()?;
    report.calibration = calibration;
    let mut all_ok = true;
    for outcome in rows {
        if !outcome.is_ok() {
            all_ok = false;
        }
        report.push(outcome);
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
    // polish A4 — invoke scripts/release-build.sh if present so the
    // benchmarked tr is the polish-A4 build (nightly + build-std +
    // panic=immediate-abort, -90% user binary size). Falls back to
    // vanilla `cargo build --release` when the script is absent
    // (older checkouts / non-mac targets / no nightly toolchain).
    let release_script = workspace.join("scripts/release-build.sh");
    let status = if release_script.is_file() {
        eprintln!(
            "→ hardev B0: scripts/release-build.sh (polish-A4 release tr — accurate user-binary size)"
        );
        std::process::Command::new(&release_script)
            .arg("-p")
            .arg("torajs-cli")
            .current_dir(workspace)
            .status()
            .context("spawning scripts/release-build.sh")?
    } else {
        eprintln!(
            "→ hardev B0: cargo build --release -p torajs-cli (vanilla release; no polish-A4 win)"
        );
        std::process::Command::new("cargo")
            .args(["build", "--release", "-p", "torajs-cli"])
            .current_dir(workspace)
            .status()
            .context("spawning `cargo build --release -p torajs-cli`")?
    };
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
