//! hardev bench B1 — machine regression verdict (`bench compare`).
//!
//! Replaces the ad-hoc, non-reproducible, not-in-repo
//! "agent hand-runs python to eyeball two result json files"
//! anti-pattern with a reproducible, in-repo, machine-judged verdict
//! plus a process exit code.
//!
//! Methodology (empirically established 2026-05-19 on torajs; see
//! `hardev/environment.md` §4b and `hardev/metrics.md`):
//!
//! - **artifact content (SHA-256) is the only trustworthy regression
//!   signal** — it is deterministic (same `tr` + same source ⇒
//!   byte-identical native binary). It is the **HARD GATE**: any
//!   per-case content change is a regression suspect and makes the
//!   command exit non-zero, unless explicitly justified with
//!   `--allow-artifact-delta case:runtime` (e.g. an intended runtime
//!   relink, or known float linker-padding nondeterminism). The gate
//!   compares `artifact_sha256` when both sides carry it and falls
//!   back to `artifact_bytes` (size) with a loud warning for pre-hash
//!   result files — size-equality is NOT code-equality (Mach-O 16 KiB
//!   segment alignment absorbs small text deltas; the size-only gate
//!   silently skipped timed runs across the Cluster E inliner firing
//!   on 8 cases).
//!
//! - **run_ms is never a hard gate.** On a loaded / cross-day mac it
//!   swings ±15 % systematically and up to +200 % single-point. A
//!   run_ms delta is only even *classified* as a possible regression
//!   when the SAME case's artifact content ALSO changed (machine code
//!   actually differs, so a perf delta is physically plausible). If
//!   the content is identical, a run_ms delta is noise **by
//!   construction** (identical machine code = identical performance)
//!   and is reported as informational only.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashSet};

#[derive(serde::Deserialize)]
struct ResultFile {
    rows: Vec<Row>,
}

// serde_json ignores unknown fields by default, so this minimal
// mirror of RunOutcome is enough (no need to add Deserialize there).
#[derive(serde::Deserialize)]
struct Row {
    case: String,
    runtime: String,
    status: String,
    run_ms: Option<f64>,
    artifact_bytes: Option<u64>,
    // absent in result files written before the content-hash fix;
    // serde fills None and the comparison falls back to size with a
    // loud warning (size-equality is NOT code-equality — Mach-O
    // 16 KiB segment alignment absorbs small text deltas)
    #[serde(default)]
    artifact_sha256: Option<String>,
}

fn load(path: &str) -> Result<BTreeMap<(String, String), Row>> {
    let txt = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let f: ResultFile = serde_json::from_str(&txt)
        .with_context(|| format!("parsing {path} as bench result json"))?;
    Ok(f.rows
        .into_iter()
        .map(|r| ((r.case.clone(), r.runtime.clone()), r))
        .collect())
}

/// hardev bench B2b — baseline artifact content hashes keyed by
/// (case, runtime), for the `--vs` artifact-precheck. Reuses the same
/// parser as `bench compare`; does not expose `Row`. Rows from
/// pre-hash result files yield `None` (precheck treats them as
/// unknown and forces the timed run).
pub fn load_artifacts(path: &str) -> Result<BTreeMap<(String, String), Option<String>>> {
    Ok(load(path)?
        .into_iter()
        .map(|(k, r)| (k, r.artifact_sha256))
        .collect())
}

/// `bench compare <baseline.json> <current.json>
///                [--allow-artifact-delta case:runtime[,case:runtime…]]`
///
/// Ok(true) = clean, Ok(false) = unjustified artifact regression
/// (caller maps to a non-zero exit code).
pub fn compare(args: &[String]) -> Result<bool> {
    let mut positional: Vec<String> = Vec::new();
    let mut allowed: HashSet<String> = HashSet::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--allow-artifact-delta" => {
                let v = args
                    .get(i + 1)
                    .context("--allow-artifact-delta requires case:runtime[,…]")?;
                for x in v.split(',').filter(|s| !s.is_empty()) {
                    allowed.insert(x.to_string());
                }
                i += 2;
            }
            s if s.starts_with("--") => anyhow::bail!("unknown flag: {s}"),
            _ => {
                positional.push(args[i].clone());
                i += 1;
            }
        }
    }
    if positional.len() != 2 {
        anyhow::bail!(
            "usage: bench-harness compare <baseline.json> <current.json> \
             [--allow-artifact-delta case:runtime,…]"
        );
    }
    let base = load(&positional[0])?;
    let cur = load(&positional[1])?;

    let mut artifact_same = 0usize;
    let mut size_only_fallback = 0usize;
    // (case, runtime, base_bytes, cur_bytes, justified)
    let mut artifact_delta: Vec<(String, String, u64, u64, bool)> = Vec::new();
    // (case, runtime, base_ms, cur_ms) — only where artifact ALSO changed
    let mut run_real: Vec<(String, String, f64, f64)> = Vec::new();
    let mut run_noise = 0usize;

    for (key, c) in &cur {
        if c.status != "ok" {
            continue;
        }
        let Some(b) = base.get(key) else { continue };
        if b.status != "ok" {
            continue;
        }
        let (case, rt) = key;
        let (Some(ba), Some(ca)) = (b.artifact_bytes, c.artifact_bytes) else {
            continue;
        };
        // Content hash is the real "machine code unchanged" signal;
        // size-equality is necessary but NOT sufficient (Mach-O
        // 16 KiB segment alignment absorbs small text deltas). Fall
        // back to size only when either file predates the hash field,
        // and count those rows so the verdict can disclose the
        // weaker comparison.
        let identical = match (&b.artifact_sha256, &c.artifact_sha256) {
            (Some(bh), Some(ch)) => bh == ch,
            _ => {
                size_only_fallback += 1;
                ba == ca
            }
        };
        if identical {
            artifact_same += 1;
            if b.run_ms.is_some() && c.run_ms.is_some() {
                // identical machine code ⇒ any run delta is noise.
                run_noise += 1;
            }
        } else {
            let tag = format!("{case}:{rt}");
            let justified = allowed.contains(&tag);
            artifact_delta.push((case.clone(), rt.clone(), ba, ca, justified));
            if let (Some(br), Some(cr)) = (b.run_ms, c.run_ms) {
                if br > 0.0 && ((cr - br) / br * 100.0).abs() > 20.0 {
                    run_real.push((case.clone(), rt.clone(), br, cr));
                }
            }
        }
    }

    println!("hardev bench compare");
    println!("  baseline: {}", positional[0]);
    println!("  current:  {}", positional[1]);
    println!();
    println!("artifact content — HARD GATE (the only deterministic signal):");
    println!("  identical: {artifact_same}");
    if size_only_fallback > 0 {
        println!(
            "  ⚠ {size_only_fallback} row(s) compared by SIZE ONLY (no artifact_sha256 \
             in one side — pre-hash result file). Size-equality does not prove \
             code-equality; re-record the baseline to restore the hash gate."
        );
    }
    if artifact_delta.is_empty() {
        println!("  delta:     0  ✓");
    } else {
        println!("  delta:     {}:", artifact_delta.len());
        for (case, rt, ba, ca, j) in &artifact_delta {
            let mark = if *j { "justified" } else { "‼ UNJUSTIFIED" };
            if ba == ca {
                println!("    {case} × {rt}: size unchanged ({ba}), content hash differs [{mark}]");
            } else {
                println!(
                    "    {case} × {rt}: {ba} → {ca} ({:+}) [{mark}]",
                    *ca as i64 - *ba as i64
                );
            }
        }
    }
    println!();
    println!("run_ms — mac-noisy, NOT a gate; classified only where artifact also changed:");
    println!("  artifact-identical cases (run delta = noise by construction): {run_noise}");
    if run_real.is_empty() {
        println!("  artifact-changed AND |Δrun| > 20%: 0");
    } else {
        println!("  artifact-changed AND |Δrun| > 20% (investigate the codegen change):");
        for (case, rt, br, cr) in &run_real {
            println!(
                "    {case} × {rt}: {br:.2} → {cr:.2} ms ({:+.1}%)",
                (cr - br) / br * 100.0
            );
        }
    }
    println!();

    let unjustified = artifact_delta.iter().filter(|x| !x.4).count();
    if unjustified == 0 {
        println!("VERDICT: PASS — 0 unjustified artifact content regressions");
        Ok(true)
    } else {
        println!(
            "VERDICT: FAIL — {unjustified} unjustified artifact content change(s). \
             Investigate, or justify intended ones with \
             --allow-artifact-delta case:runtime[,…]"
        );
        Ok(false)
    }
}
