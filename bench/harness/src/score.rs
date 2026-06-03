//! `bench-harness score <result.json>` — per-category weighted
//! scoreboard.
//!
//! Reads `bench/categories.toml` for the case → category mapping and
//! per-category weight, then for each comparator runtime prints:
//!
//! 1. **Per-category geomean** of `torajs.run_ms / comparator.run_ms`
//!    (lower = tora wins; 1.0 = tied; > 1.0 = tora slower).
//! 2. **Weighted overall geomean** computed as `exp(Σ wᵢ·log(rᵢ) /
//!    Σ wᵢ)` where `rᵢ` is each category's geomean and `wᵢ` is its
//!    declared weight. Categories without any tora-vs-comparator
//!    pair are skipped from the weighted average (their weight does
//!    not penalize the overall score).
//! 3. **Coverage gaps**: cases in the result file but unmapped in
//!    categories.toml are flagged so coverage drift stays visible.
//!
//! The weighted scoreboard is the honest answer to "are we
//! `actually` beating bun?" — raw `26/0 wins` over a case set heavy
//! on pure-compute microbenches is misleading; weighted geomean over
//! a categorized set surfaces where the wins are concrete and where
//! they're missing.

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct CategoriesConfig {
    categories: BTreeMap<String, CategoryDef>,
}

#[derive(Debug, Deserialize)]
struct CategoryDef {
    weight: u32,
    cases: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ResultFile {
    rows: Vec<ResultRow>,
}

#[derive(Debug, Deserialize)]
struct ResultRow {
    case: String,
    runtime: String,
    status: String,
    run_ms: Option<f64>,
}

const COMPARATORS: &[&str] = &["bun-aot", "bun-jsc", "node-v8", "rust", "go"];

pub fn score(args: &[String]) -> Result<bool> {
    let result_path = args.first().context(
        "score: expected a result file path, e.g. \
                 `bench-harness score bench/results/<latest>.json`",
    )?;
    let bench_dir = find_bench_dir()?;
    let categories = load_categories(&bench_dir)?;
    let result = load_result(Path::new(result_path))?;

    // case → run_ms by runtime (only ok rows with a non-null run_ms).
    let mut by_case: HashMap<String, HashMap<String, f64>> = HashMap::new();
    for r in &result.rows {
        if r.status != "ok" {
            continue;
        }
        let Some(ms) = r.run_ms else { continue };
        by_case
            .entry(r.case.clone())
            .or_default()
            .insert(r.runtime.clone(), ms);
    }

    let mapped: BTreeSet<String> = categories
        .categories
        .values()
        .flat_map(|c| c.cases.iter().cloned())
        .collect();
    let mut unmapped: Vec<String> = by_case
        .keys()
        .filter(|c| !mapped.contains(*c))
        .cloned()
        .collect();
    unmapped.sort();
    if !unmapped.is_empty() {
        eprintln!(
            "⚠ {} case(s) in {result_path} missing from \
             bench/categories.toml — they will not contribute to \
             the weighted score:",
            unmapped.len()
        );
        for c in &unmapped {
            eprintln!("    {c}");
        }
        eprintln!();
    }

    println!(
        "score over {} (per-category geomean of torajs / comparator run_ms)",
        result_path
    );
    println!("  < 1.0 = torajs faster · 1.0 = tied · > 1.0 = torajs slower");
    println!();

    let header_cats: Vec<(&String, &CategoryDef)> = categories.categories.iter().collect();

    print!("{:<22}", "comparator");
    for (name, c) in &header_cats {
        print!(" {:>14}", format!("{}(w={})", name, c.weight));
    }
    println!(" {:>16}", "WEIGHTED");
    println!("{}", "-".repeat(22 + 15 * header_cats.len() + 17));

    for comp in COMPARATORS {
        print!("vs {:<19}", comp);
        let mut weighted_logsum: f64 = 0.0;
        let mut weighted_total: f64 = 0.0;
        for (_, cdef) in &header_cats {
            let mut ratios: Vec<f64> = Vec::new();
            for case in &cdef.cases {
                let Some(row) = by_case.get(case) else {
                    continue;
                };
                let (Some(t), Some(c)) = (row.get("torajs"), row.get(*comp)) else {
                    continue;
                };
                if *c > 0.0 {
                    ratios.push(*t / *c);
                }
            }
            if ratios.is_empty() {
                print!(" {:>14}", "—");
            } else {
                let gm = geomean(&ratios);
                print!(" {:>14}", format!("{:.3}", gm));
                weighted_logsum += (cdef.weight as f64) * gm.ln();
                weighted_total += cdef.weight as f64;
            }
        }
        if weighted_total > 0.0 {
            let weighted = (weighted_logsum / weighted_total).exp();
            print!(
                " {:>16}",
                format!("{:.3} ({:.2}× faster)", weighted, 1.0 / weighted)
            );
        } else {
            print!(" {:>16}", "—");
        }
        println!();
    }

    Ok(true)
}

fn geomean(xs: &[f64]) -> f64 {
    let n = xs.len() as f64;
    (xs.iter().map(|x| x.ln()).sum::<f64>() / n).exp()
}

fn find_bench_dir() -> Result<std::path::PathBuf> {
    // Walk up from cwd looking for `bench/categories.toml` — supports
    // running from the workspace root OR from inside `bench/`.
    let mut here = std::env::current_dir()?;
    loop {
        let candidate = here.join("bench").join("categories.toml");
        if candidate.exists() {
            return Ok(here.join("bench"));
        }
        let here_cat = here.join("categories.toml");
        if here_cat.exists() {
            return Ok(here);
        }
        if !here.pop() {
            return Err(anyhow!("could not locate bench/categories.toml"));
        }
    }
}

fn load_categories(bench_dir: &Path) -> Result<CategoriesConfig> {
    let path = bench_dir.join("categories.toml");
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

fn load_result(path: &Path) -> Result<ResultFile> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}
