//! Corpus runner — feed every `conformance/cases/*.ts` fixture's SSA
//! Functions through `torajs_codegen::compile_function` and tally
//! the per-fixture outcome (OK / pipeline-failed / codegen-panicked).
//!
//! The point is *gap discovery*: torajs-codegen is still isolated
//! (ssa_inkwell is the production path), so every compile failure
//! is a concrete InstKind shape / SSA pattern the new backend has
//! to handle before the S6 cut-over can flip the dispatch.
//!
//! ## Why fork-per-fixture
//!
//! The workspace sets `panic = "abort"` in both dev and release
//! profiles, so `catch_unwind` is a no-op — a single SSA Function
//! that panics will SIGABRT the whole sweep. To still batch over
//! all 475 fixtures, the example self-forks: the parent process
//! sweeps `conformance/cases/*.ts` and spawns a child per fixture;
//! each child invokes itself with a `--single <path>` arg, runs
//! the pipeline + codegen for that one file, and either exits 0
//! (all functions compiled) or aborts with a panic message on
//! stderr that the parent captures.
//!
//! Each fixture's outcome is bucketed by the first line of the
//! child's stderr (the panic message), trimmed of file:line noise.
//!
//! Usage from repo root:
//!
//! ```bash
//!   cargo run --release -p torajs-codegen --example corpus_check
//!   cargo run --release -p torajs-codegen --example corpus_check -- \
//!       --single conformance/cases/anon-obj-001-literal-layout.ts
//! ```
//!
//! Output is a markdown summary on stdout — paste it into commit
//! messages / plan-state notes when reasoning about which gap to
//! close next.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use torajs_codegen::compile_function;
use torajs_core::{check, lexer, parser, ssa_lower};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--single") => {
            let path = args.get(1).expect("--single requires a path argument");
            run_single(Path::new(path));
        }
        Some("--help" | "-h") => print_help(),
        _ => run_sweep(),
    }
}

fn print_help() {
    println!(
        "usage:\n  \
         corpus_check                              # sweep conformance/cases/*.ts\n  \
         corpus_check --single <path>             # compile one fixture, panic-as-abort\n"
    );
}

/// In-process single-fixture run. Returns normally on success; if
/// the pipeline or codegen panics, the panic-runtime aborts the
/// process with a message on stderr — the parent sweep captures it.
fn run_single(path: &Path) {
    let src = fs::read_to_string(path).unwrap_or_else(|e| panic!("PIPELINE read: {e}"));
    let tokens = lexer::tokenize(&src).unwrap_or_else(|e| panic!("PIPELINE lex: {e}"));
    let ast = parser::parse(&tokens).unwrap_or_else(|e| panic!("PIPELINE parse: {e}"));
    let (gcs, expr_types, arity_pad) = check::check_with_arity(&ast).unwrap_or_else(|e| {
        // Keep only the first line so the parent's classifier
        // doesn't blow up over multi-line diagnostic blocks.
        let head = e.lines().next().unwrap_or(&e).to_string();
        panic!("PIPELINE check: {head}")
    });
    let module = ssa_lower::lower_with_arity(&ast, &gcs, &expr_types, &arity_pad);

    // Compile every fn; the first one that panics aborts us — that
    // panic message is what the parent buckets.
    let mut n: u32 = 0;
    let mut total_bytes: usize = 0;
    for f in &module.funcs {
        n += 1;
        let cf = compile_function(f);
        total_bytes += cf.bytes.len();
    }
    // Plain newline so the parent can parse the success line.
    println!("OK {n} fns, {total_bytes} bytes");
}

/// Parent sweep: enumerate fixtures, spawn one child per fixture,
/// classify the child's outcome.
fn run_sweep() {
    let exe = env::current_exe().expect("current_exe");
    let cases_dir = PathBuf::from("conformance/cases");
    let mut paths: Vec<PathBuf> = fs::read_dir(&cases_dir)
        .unwrap_or_else(|e| panic!("conformance/cases unreadable ({e}) — run from repo root"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|s| s == "ts").unwrap_or(false))
        .collect();
    paths.sort();

    let mut codegen_ok: usize = 0;
    let mut pipeline_failed: usize = 0;
    let mut codegen_panicked: usize = 0;
    let mut gap_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut gap_first_hit: BTreeMap<String, String> = BTreeMap::new();
    let mut pipeline_stage_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut total_fns: u64 = 0;
    let mut total_bytes: u64 = 0;

    // Light progress bar to stderr; ~5min for full 475 if each
    // child is ~600ms. Easy to spot regressions visually.
    let mut last_progress = 0;

    for (i, path) in paths.iter().enumerate() {
        let out = Command::new(&exe)
            .arg("--single")
            .arg(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();
        let out = match out {
            Ok(o) => o,
            Err(e) => {
                eprintln!("spawn failed for {}: {e}", path.display());
                continue;
            }
        };
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if out.status.success() {
            codegen_ok += 1;
            if let Some(line) = stdout.lines().next() {
                // Parse "OK N fns, M bytes".
                let mut it = line.split_ascii_whitespace();
                if it.next() == Some("OK") {
                    if let Some(n) = it.next() {
                        total_fns += n.parse::<u64>().unwrap_or(0);
                    }
                    // skip "fns,"
                    it.next();
                    if let Some(b) = it.next() {
                        total_bytes += b.parse::<u64>().unwrap_or(0);
                    }
                }
            }
        } else {
            let msg = extract_panic_message(&stderr);
            let is_upstream = msg.starts_with("PIPELINE ");
            let key = classify(&msg);
            if is_upstream {
                pipeline_failed += 1;
                let stage = key
                    .strip_prefix("PIPELINE ")
                    .and_then(|s| s.split(':').next())
                    .unwrap_or("?")
                    .trim()
                    .to_string();
                *pipeline_stage_counts.entry(stage).or_insert(0) += 1;
            } else {
                codegen_panicked += 1;
                *gap_counts.entry(key.clone()).or_insert(0) += 1;
                gap_first_hit.entry(key).or_insert_with(|| {
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()
                });
            }
        }

        if i - last_progress >= 25 || i + 1 == paths.len() {
            let _ = writeln!(
                std::io::stderr(),
                "[{:>4}/{}] ok={} upstream={} panic={}",
                i + 1,
                paths.len(),
                codegen_ok,
                pipeline_failed,
                codegen_panicked
            );
            last_progress = i;
        }
    }

    println!("# torajs-codegen corpus check");
    println!();
    println!("- fixtures scanned: {}", paths.len());
    println!("- codegen OK: {codegen_ok}");
    println!("- upstream-pipeline failed (parse / check / lex): {pipeline_failed}");
    println!("- codegen panicked: {codegen_panicked}");
    println!("- total SSA functions handed to compile_function (on OK fixtures): {total_fns}");
    println!("- total bytes emitted (on OK fixtures): {total_bytes}");
    println!();

    if !gap_counts.is_empty() {
        println!("## Codegen gap categories (sorted by count desc)");
        println!();
        println!("| count | category | first hit |");
        println!("|------:|----------|-----------|");
        let mut entries: Vec<(&String, &usize)> = gap_counts.iter().collect();
        entries.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (cat, n) in entries {
            println!(
                "| {n} | {cat} | {} |",
                gap_first_hit.get(cat).map(String::as_str).unwrap_or("")
            );
        }
        println!();
    }

    if !pipeline_stage_counts.is_empty() {
        println!("## Upstream (pre-codegen) failure tally");
        println!();
        println!("| count | stage |");
        println!("|------:|-------|");
        for (stage, n) in &pipeline_stage_counts {
            println!("| {n} | {stage} |");
        }
    }
}

/// Pull the first "thread 'X' panicked at ..." block's message line
/// out of a child's stderr. Falls back to the first non-empty line.
fn extract_panic_message(stderr: &str) -> String {
    let mut lines = stderr.lines().peekable();
    while let Some(line) = lines.next() {
        if line.starts_with("thread '") && line.contains("panicked at") {
            // The actual message lives on the next non-empty line.
            for next in lines.by_ref() {
                let t = next.trim();
                if !t.is_empty() {
                    return t.to_string();
                }
            }
        }
    }
    stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("<empty stderr>")
        .to_string()
}

/// Collapse a panic message to a stable category.
fn classify(msg: &str) -> String {
    let first = msg.lines().next().unwrap_or(msg).trim();
    // Strip the optional file:line tail some toolchains append:
    //   "not yet implemented: foo, src/bar.rs:42:5"
    if let Some(i) = first.find(", src/") {
        first[..i].trim().to_string()
    } else {
        first.to_string()
    }
}
