//! torajs conformance runner.
//!
//! Each conformance case is a single `.ts` file at `conformance/cases/`.
//! For every case we run:
//!   1. bun run <case>.ts     ← oracle (TS spec behavior)
//!   2. torajs run <case>.ts  ← AOT-with-cache (LLVM, cached at ~/.torajs/cache)
//!   3. torajs build <case>.ts && ./out  ← AOT (LLVM, explicit out path)
//!
//! All three outputs must match. A case can opt out of bun (when the
//! TS spec output diverges from torajs's subset semantics — e.g. JS's
//! reference-shape mutable closure capture vs torajs's value-shape) by
//! placing an `.expected` file alongside the `.ts`; presence of
//! `.expected` overrides bun's output.
//!
//! Parallelism: the `tr` binary is built ONCE up front (cargo tells us
//! the exact path via `--message-format=json`), then every case invokes
//! that binary directly across a worker pool. This removes ~N× per-case
//! `cargo run` overhead and the cargo build-lock contention that would
//! otherwise serialize concurrent cases. Scheduling only — the set of
//! cases, the 3 sub-steps, and the byte-equal check are unchanged, so
//! the pass/fail verdict is identical to the sequential runner.
//! Per-case temp paths are made worker/pid-unique to stay collision-safe
//! under concurrency. Results are replayed in original case order so the
//! output is byte-stable across runs (and across the sequential→parallel
//! switch). Default 8 workers; override with `--workers N`.
//!
//! Exit code: 0 if all pass, 1 if any fail.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

const DEFAULT_WORKERS: usize = 8;

#[derive(Debug, Clone)]
struct Case {
    name: String,
    src: PathBuf,
    expected_override: Option<PathBuf>,
}

#[derive(Debug)]
enum Outcome {
    Pass,
    Fail { reason: String },
    Skip { reason: String },
}

fn main() {
    let workers = parse_workers();

    let repo_root = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => die(&format!("cwd: {e}")),
    };
    let cases_dir = repo_root.join("conformance/cases");
    let port_dir = repo_root.join("conformance/test262-port");
    let mut cases: Vec<Case> = Vec::new();
    if cases_dir.is_dir() {
        cases.extend(collect_cases(&cases_dir));
    }
    if port_dir.is_dir() {
        cases.extend(collect_cases(&port_dir));
    }
    if cases.is_empty() {
        die("no .ts files under conformance/cases/ or conformance/test262-port/");
    }
    let manifest = repo_root.join("crates/torajs-cli/Cargo.toml");
    if !manifest.is_file() {
        die("torajs CLI not found — run from repo root");
    }

    // Build `tr` exactly once. cargo reports the produced binary's
    // absolute path in its JSON artifact stream — orthodox, no
    // target-dir guessing, no extra deps.
    let tr_bin = build_tr_once(&manifest, &repo_root);

    println!(
        "running {} conformance cases — {} workers, tr = {}\n",
        cases.len(),
        workers,
        tr_bin.display()
    );

    // Per-case result slots, filled by workers, replayed in order.
    let results: Vec<Mutex<Option<Outcome>>> = (0..cases.len()).map(|_| Mutex::new(None)).collect();
    let queue = Mutex::new(0usize);
    let progress = AtomicUsize::new(0);
    let total = cases.len();
    let start = Instant::now();

    std::thread::scope(|scope| {
        for slot in 0..workers {
            let queue = &queue;
            let cases = &cases;
            let results = &results;
            let tr_bin = &tr_bin;
            let progress = &progress;
            scope.spawn(move || {
                loop {
                    let idx = {
                        let mut g = queue.lock().unwrap();
                        let i = *g;
                        *g += 1;
                        i
                    };
                    if idx >= cases.len() {
                        break;
                    }
                    let outcome = run_case(&cases[idx], tr_bin, slot);
                    *results[idx].lock().unwrap() = Some(outcome);
                    let n = progress.fetch_add(1, Ordering::Relaxed) + 1;
                    if n.is_multiple_of(50) || n == total {
                        let pct = (n as f64 / total as f64) * 100.0;
                        let secs = start.elapsed().as_secs_f64();
                        let rate = if secs > 0.0 { n as f64 / secs } else { 0.0 };
                        print!("  [{n}/{total} {pct:.0}% — {rate:.1}/s]\r");
                        let _ = std::io::Write::flush(&mut std::io::stdout());
                    }
                }
            });
        }
    });

    // Replay in original case order → byte-stable, sequential-identical.
    println!();
    let mut pass = 0;
    let mut fail = Vec::new();
    let mut skip = 0;
    for (i, c) in cases.iter().enumerate() {
        let outcome = results[i]
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| Outcome::Fail {
                reason: "worker produced no result".to_string(),
            });
        match outcome {
            Outcome::Pass => {
                pass += 1;
                println!("  ok    {}", c.name);
            }
            Outcome::Fail { reason } => {
                println!("  FAIL  {}: {reason}", c.name);
                fail.push((c.name.clone(), reason));
            }
            Outcome::Skip { reason } => {
                skip += 1;
                println!("  skip  {}: {reason}", c.name);
            }
        }
    }
    println!("\n{} pass / {} fail / {} skip", pass, fail.len(), skip);
    if !fail.is_empty() {
        println!("\nfailures:");
        for (n, r) in &fail {
            println!("  {n}");
            for line in r.lines().take(6) {
                println!("    {line}");
            }
        }
        std::process::exit(1);
    }
}

fn parse_workers() -> usize {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--workers" {
            if let Some(v) = args
                .next()
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|&v| v >= 1)
            {
                return v;
            }
            die("--workers expects a positive integer");
        }
    }
    DEFAULT_WORKERS
}

/// Build `tr` once and return the exact binary path cargo produced.
/// Uses `--message-format=json` so cargo itself reports the artifact
/// location — no target-dir inference. Falls back to
/// `<repo>/target/iter/tr` only if the JSON has no `tr` artifact.
fn build_tr_once(manifest: &Path, repo_root: &Path) -> PathBuf {
    let out = Command::new("cargo")
        .args([
            "build",
            // hardev devperf #1 — fast iteration profile (lto=off,
            // cgu=256, opt=1). opt-level/LTO are semantics-invariant
            // so an `iter` tr is stdout-byte-equal to a `release` tr
            // on every case (629/0/1 proves it); same coverage, same
            // verdict, ~6x faster tr rebuilds. bench/ship keep
            // --release (separate target/release/tr artifact).
            "--profile",
            "iter",
            "--quiet",
            "--manifest-path",
            manifest.to_str().unwrap(),
            "--message-format=json",
        ])
        .output()
        .unwrap_or_else(|e| die(&format!("spawn cargo build: {e}")));
    if !out.status.success() {
        die(&format!(
            "cargo build tr failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    // Each line is a JSON object; the compiler-artifact for the `tr`
    // binary carries `"executable":"<abs path .../tr>"`. Hand-extract
    // (no serde dep): keep the last such path.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut found: Option<PathBuf> = None;
    for line in stdout.lines() {
        if let Some(pos) = line.find("\"executable\":\"") {
            let rest = &line[pos + "\"executable\":\"".len()..];
            if let Some(end) = rest.find('"') {
                let path = &rest[..end];
                if path.ends_with("/tr") {
                    found = Some(PathBuf::from(path));
                }
            }
        }
    }
    let tr_bin = found.unwrap_or_else(|| repo_root.join("target/iter/tr"));
    if !tr_bin.is_file() {
        die(&format!(
            "tr binary not found after build: {}",
            tr_bin.display()
        ));
    }
    tr_bin
}

fn collect_cases(dir: &Path) -> Vec<Case> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => die(&format!("read_dir {}: {e}", dir.display())),
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|x| x.path())).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            // Phase K.2 — multi-file case. Convention: the directory is one
            // case, named after the directory; `<dir>/main.ts` is the entry
            // point that bun and torajs both run; sibling `.ts` files are
            // imported via relative `import` statements. Expected-output
            // override (when bun differs) lives at `<dir>/main.expected`,
            // mirroring the single-file convention.
            let main_ts = p.join("main.ts");
            if !main_ts.is_file() {
                continue;
            }
            let stem = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            let expected = main_ts.with_extension("expected");
            out.push(Case {
                name: stem,
                src: main_ts,
                expected_override: if expected.is_file() {
                    Some(expected)
                } else {
                    None
                },
            });
            continue;
        }
        if p.extension().is_none_or(|e| e != "ts") {
            continue;
        }
        let stem = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let expected = p.with_extension("expected");
        out.push(Case {
            name: stem,
            src: p,
            expected_override: if expected.is_file() {
                Some(expected)
            } else {
                None
            },
        });
    }
    out
}

fn run_case(c: &Case, tr_bin: &Path, slot: usize) -> Outcome {
    // Step 1: oracle output. Either an explicit .expected file or
    // whatever bun produces.
    let oracle = match &c.expected_override {
        Some(p) => match std::fs::read_to_string(p) {
            Ok(s) => s,
            Err(e) => {
                return Outcome::Fail {
                    reason: format!("read {}: {e}", p.display()),
                };
            }
        },
        None => match exec("bun", &["run", c.src.to_str().unwrap()]) {
            Ok((s, _)) => s,
            Err(e) => {
                return Outcome::Skip {
                    reason: format!("bun: {e}"),
                };
            }
        },
    };

    // Step 2: torajs JIT (prebuilt tr binary, invoked directly)
    let jit = match exec(tr_bin.to_str().unwrap(), &["run", c.src.to_str().unwrap()]) {
        Ok((s, _)) => s,
        Err(e) => {
            return Outcome::Fail {
                reason: format!("jit: {e}"),
            };
        }
    };
    if jit != oracle {
        return Outcome::Fail {
            reason: format!("jit ≠ oracle:\n  oracle: {oracle:?}\n  jit:    {jit:?}"),
        };
    }

    // Step 3: torajs AOT. Output path is worker/pid-unique so
    // concurrent cases never collide on the binary or its .dSYM.
    let pid = std::process::id();
    let aot_bin = std::env::temp_dir().join(format!("torajs-conf-{}-s{slot}-p{pid}", c.name));
    let aot_dsym = aot_bin.with_extension("dSYM");
    if let Err(e) = exec(
        tr_bin.to_str().unwrap(),
        &[
            "build",
            c.src.to_str().unwrap(),
            "-o",
            aot_bin.to_str().unwrap(),
        ],
    ) {
        let _ = std::fs::remove_file(&aot_bin);
        let _ = std::fs::remove_dir_all(&aot_dsym);
        return Outcome::Fail {
            reason: format!("aot build: {e}"),
        };
    }
    let aot = match exec(aot_bin.to_str().unwrap(), &[]) {
        Ok((s, _)) => s,
        Err(e) => {
            let _ = std::fs::remove_file(&aot_bin);
            let _ = std::fs::remove_dir_all(&aot_dsym);
            return Outcome::Fail {
                reason: format!("aot run: {e}"),
            };
        }
    };
    let _ = std::fs::remove_file(&aot_bin);
    let _ = std::fs::remove_dir_all(&aot_dsym);
    if aot != oracle {
        return Outcome::Fail {
            reason: format!("aot ≠ oracle:\n  oracle: {oracle:?}\n  aot:    {aot:?}"),
        };
    }
    Outcome::Pass
}

fn exec(cmd: &str, args: &[&str]) -> Result<(String, String), String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("spawn {cmd}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{cmd} exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    Ok((stdout, stderr))
}

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(2);
}
