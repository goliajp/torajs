//! torajs conformance runner.
//!
//! Each conformance case is a single `.ts` file at `conformance/cases/`.
//! For every case we run:
//!   1. bun run <case>.ts     ← oracle (TS spec behavior)
//!   2. torajs run <case>.ts  ← JIT (Cranelift)
//!   3. torajs build <case>.ts && ./out  ← AOT (LLVM)
//!
//! All three outputs must match. A case can opt out of bun (when the
//! TS spec output diverges from torajs's subset semantics — e.g. JS's
//! reference-shape mutable closure capture vs torajs's value-shape) by
//! placing an `.expected` file alongside the `.ts`; presence of
//! `.expected` overrides bun's output.
//!
//! Exit code: 0 if all pass, 1 if any fail.

use std::path::{Path, PathBuf};
use std::process::Command;

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
    let manifest = repo_root.join("labs/0001-walking-skeleton/Cargo.toml");
    if !manifest.is_file() {
        die("torajs CLI not found — run from repo root");
    }

    println!("running {} conformance cases\n", cases.len());

    let mut pass = 0;
    let mut fail = Vec::new();
    let mut skip = 0;
    for c in &cases {
        match run_case(c, &manifest) {
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
    println!(
        "\n{} pass / {} fail / {} skip",
        pass,
        fail.len(),
        skip
    );
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

fn collect_cases(dir: &Path) -> Vec<Case> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => die(&format!("read_dir {}: {e}", dir.display())),
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "ts"))
        .collect();
    paths.sort();
    for p in paths {
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

fn run_case(c: &Case, manifest: &Path) -> Outcome {
    // Step 1: oracle output. Either an explicit .expected file or
    // whatever bun produces.
    let oracle = match &c.expected_override {
        Some(p) => match std::fs::read_to_string(p) {
            Ok(s) => s,
            Err(e) => return Outcome::Fail { reason: format!("read {}: {e}", p.display()) },
        },
        None => match exec("bun", &["run", c.src.to_str().unwrap()], None) {
            Ok((s, _)) => s,
            Err(e) => return Outcome::Skip { reason: format!("bun: {e}") },
        },
    };

    // Step 2: torajs JIT
    let jit = match exec(
        "cargo",
        &[
            "run",
            "--release",
            "--quiet",
            "--manifest-path",
            manifest.to_str().unwrap(),
            "--",
            "run",
            c.src.to_str().unwrap(),
        ],
        Some(c),
    ) {
        Ok((s, _)) => s,
        Err(e) => return Outcome::Fail { reason: format!("jit: {e}") },
    };
    if jit != oracle {
        return Outcome::Fail {
            reason: format!("jit ≠ oracle:\n  oracle: {oracle:?}\n  jit:    {jit:?}"),
        };
    }

    // Step 3: torajs AOT
    let aot_bin = std::env::temp_dir().join(format!("torajs-conf-{}", c.name));
    if let Err(e) = exec(
        "cargo",
        &[
            "run",
            "--release",
            "--quiet",
            "--manifest-path",
            manifest.to_str().unwrap(),
            "--",
            "build",
            c.src.to_str().unwrap(),
            "-o",
            aot_bin.to_str().unwrap(),
        ],
        Some(c),
    ) {
        return Outcome::Fail { reason: format!("aot build: {e}") };
    }
    let aot = match exec(aot_bin.to_str().unwrap(), &[], Some(c)) {
        Ok((s, _)) => s,
        Err(e) => return Outcome::Fail { reason: format!("aot run: {e}") },
    };
    let _ = std::fs::remove_file(&aot_bin);
    if aot != oracle {
        return Outcome::Fail {
            reason: format!("aot ≠ oracle:\n  oracle: {oracle:?}\n  aot:    {aot:?}"),
        };
    }
    Outcome::Pass
}

fn exec(cmd: &str, args: &[&str], _ctx: Option<&Case>) -> Result<(String, String), String> {
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
