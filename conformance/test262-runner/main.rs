//! tc39/test262 runner — measure torajs's conformance over the
//! subset-compatible slice of test262.
//!
//! Pipeline per case:
//!   1. Read the case's source from `vendor/test262/test/.../case.js`.
//!   2. Prepend the standard test262 harness (sta.js + assert.js).
//!   3. Run with `bun run` (oracle). If bun exits non-zero we treat
//!      the case as a negative / harness-dependent test and skip.
//!   4. Run with `tr run`. Compare exit code + stdout against bun.
//!   5. Categorize:
//!     - pass: tr matches bun
//!     - bug: tr exit non-zero with no obvious "subset boundary"
//!       error in stderr, OR stdout differs
//!     - incompatible: tr stderr starts with one of the documented
//!       subset-boundary messages (lex error / parse error / type
//!       error / not yet supported / import error)
//!     - bun-skip: oracle didn't pass; not interesting
//!
//! Concurrency: spawns N worker threads (default 8) that pull from a
//! shared queue. Each worker writes a temp file under
//! `$TMPDIR/torajs-test262-<pid>-<n>.ts`, runs bun + tr, cleans up.
//!
//! Args:
//!   --limit N       — only run the first N cases (useful for sampling).
//!   --filter STR    — only run cases whose path contains STR.
//!   --workers N     — concurrency (default 8).
//!   --report-bugs N — list the first N bug-classified failures with
//!                     their stderr first line (default 20).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

const TEST262_ROOT: &str = "vendor/test262";
/// Typed harness path (relative to repo root). Replaces test262's
/// stock `harness/sta.js` + `harness/assert.js` — those are untyped
/// JS and would trip torajs's typecheck on the very first line. The
/// typed harness exposes top-level generic fns (`__t262_*`) that the
/// source-rewrite layer points every `assert.X(...)` call site at.
const TORAJS_HARNESS: &str = "conformance/test262-harness.ts";
const DEFAULT_WORKERS: usize = 8;
const DEFAULT_REPORT_BUGS: usize = 20;

#[derive(Debug, Clone)]
enum Outcome {
    Pass,
    Bug { kind: String, msg: String },
    Incompatible { kind: String },
    BunSkip,
    HarnessError { msg: String },
}

struct Args {
    limit: Option<usize>,
    filter: Option<String>,
    workers: usize,
    report_bugs: usize,
}

fn parse_args() -> Args {
    let mut limit: Option<usize> = None;
    let mut filter: Option<String> = None;
    let mut workers = DEFAULT_WORKERS;
    let mut report_bugs = DEFAULT_REPORT_BUGS;
    let mut iter = std::env::args().skip(1);
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--limit" => limit = iter.next().and_then(|v| v.parse().ok()),
            "--filter" => filter = iter.next(),
            "--workers" => {
                if let Some(v) = iter.next().and_then(|v| v.parse().ok()) {
                    workers = v;
                }
            }
            "--report-bugs" => {
                if let Some(v) = iter.next().and_then(|v| v.parse().ok()) {
                    report_bugs = v;
                }
            }
            "-h" | "--help" => {
                eprintln!(
                    "torajs-test262 — run tc39/test262 against tr\n\nflags:\n  --limit N       only first N cases\n  --filter STR    cases whose path contains STR\n  --workers N     concurrency (default {DEFAULT_WORKERS})\n  --report-bugs N list first N bug failures (default {DEFAULT_REPORT_BUGS})"
                );
                std::process::exit(0);
            }
            other => {
                eprintln!("error: unknown arg `{other}`");
                std::process::exit(2);
            }
        }
    }
    Args { limit, filter, workers, report_bugs }
}

fn collect_cases(root: &Path, filter: Option<&str>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if p.extension().is_none_or(|x| x != "js") {
                continue;
            }
            // Test262 includes _FIXTURE.js helper sources that aren't
            // standalone test cases — they're loaded by includes:.
            let stem = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if stem.ends_with("_FIXTURE.js") {
                continue;
            }
            if let Some(f) = filter
                && !p.to_string_lossy().contains(f)
            {
                continue;
            }
            out.push(p);
        }
    }
    out.sort();
    out
}

fn read_harness() -> Result<String, String> {
    let path = Path::new(TORAJS_HARNESS);
    std::fs::read_to_string(path)
        .map(|s| {
            let mut out = s;
            out.push('\n');
            out
        })
        .map_err(|e| format!("read harness {}: {e}", path.display()))
}

/// Source rewrite — minimum-viable layer to bridge test262's stock
/// JS to torajs's strict TS subset. Operates byte-by-byte over the
/// case source, skipping inside string literals and comments so the
/// rewrites never fire on string contents. Current rewrites:
///
///   - `assert.sameValue(`     → `__t262_sameValue(`
///   - `assert.notSameValue(`  → `__t262_notSameValue(`
///   - `assert.throws(<id>, `  → `__t262_throws_runtime(`  (drops the
///                              first ident arg — torajs has no way
///                              to compare class identity at runtime)
///   - bare `assert(`          → `__t262_assert(`
///   - leading-word `var `     → `let `
///
/// What this DOESN'T do: handle `==` → `===`, untyped fn-decl
/// parameter annotation, `null` / `undefined` literals, or features
/// like Symbol / Proxy / WeakMap. Those hit torajs's subset boundary
/// directly and the case stays classified `incompatible` until a
/// bigger transform layer or substrate change addresses them.
fn transform_source(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(bytes.len() + 64);
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        // String literal — copy verbatim until the matching quote.
        if b == b'"' || b == b'\'' || b == b'`' {
            let quote = b;
            out.push(quote as char);
            i += 1;
            while i < bytes.len() {
                let c = bytes[i];
                out.push(c as char);
                i += 1;
                if c == b'\\' && i < bytes.len() {
                    out.push(bytes[i] as char);
                    i += 1;
                    continue;
                }
                if c == quote {
                    break;
                }
            }
            continue;
        }
        // `//` line comment.
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(bytes[i] as char);
                i += 1;
            }
            continue;
        }
        // `/* ... */` block comment.
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            out.push('/');
            out.push('*');
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                out.push(bytes[i] as char);
                i += 1;
            }
            if i + 1 < bytes.len() {
                out.push('*');
                out.push('/');
                i += 2;
            }
            continue;
        }
        // `assert.<method>(` rewrites.
        if starts_with_at(bytes, i, b"assert.") {
            // Try the longest-match-first rewrites.
            const REWRITES: &[(&[u8], &str)] = &[
                (b"assert.sameValue(", "__t262_sameValue("),
                (b"assert.notSameValue(", "__t262_notSameValue("),
            ];
            let mut hit = false;
            for (needle, replacement) in REWRITES {
                if starts_with_at(bytes, i, needle) {
                    out.push_str(replacement);
                    i += needle.len();
                    hit = true;
                    break;
                }
            }
            if hit {
                continue;
            }
            // `assert.throws(<ident>, ` → drop the class arg.
            if starts_with_at(bytes, i, b"assert.throws(") {
                let after = i + b"assert.throws(".len();
                let mut j = after;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                let id_start = j;
                while j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_')
                {
                    j += 1;
                }
                if j > id_start {
                    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b',' {
                        j += 1;
                        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                            j += 1;
                        }
                        out.push_str("__t262_throws_runtime(");
                        i = j;
                        continue;
                    }
                }
                // Couldn't parse the class arg cleanly — fall through and
                // emit verbatim.
            }
        }
        // bare `assert(` (must NOT be a member like `obj.assert(`).
        if starts_with_at(bytes, i, b"assert(") && !preceded_by_dot(bytes, i) {
            out.push_str("__t262_assert(");
            i += b"assert(".len();
            continue;
        }
        // `var ` → `let ` (word-boundary on the left + whitespace on the right).
        if starts_with_at(bytes, i, b"var ") && !preceded_by_word(bytes, i) {
            out.push_str("let ");
            i += b"var ".len();
            continue;
        }
        out.push(b as char);
        i += 1;
    }
    out
}

fn starts_with_at(bytes: &[u8], i: usize, needle: &[u8]) -> bool {
    if i + needle.len() > bytes.len() {
        return false;
    }
    &bytes[i..i + needle.len()] == needle
}

fn preceded_by_dot(bytes: &[u8], i: usize) -> bool {
    if i == 0 {
        return false;
    }
    bytes[i - 1] == b'.'
}

fn preceded_by_word(bytes: &[u8], i: usize) -> bool {
    if i == 0 {
        return false;
    }
    let c = bytes[i - 1];
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

fn run_case(path: &Path, harness: &str, tr_bin: &Path, slot: usize) -> Outcome {
    let case_src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return Outcome::HarnessError {
                msg: format!("read {}: {e}", path.display()),
            };
        }
    };
    let transformed = transform_source(&case_src);
    let full = format!("{harness}\n{transformed}");

    // Distinct tmp file per worker slot to avoid races. Use `.ts` so
    // tr's read_source treats it as a normal source file (extension
    // isn't actually checked but the convention matches the rest of
    // the pipeline).
    let tmp_path = std::env::temp_dir().join(format!(
        "torajs-test262-{}-{}.ts",
        std::process::id(),
        slot
    ));
    if let Err(e) = std::fs::write(&tmp_path, &full) {
        return Outcome::HarnessError {
            msg: format!("write tmp: {e}"),
        };
    }

    let bun = Command::new("bun")
        .args(["run", &tmp_path.to_string_lossy()])
        .output();
    let bun_out = match bun {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            return Outcome::HarnessError {
                msg: format!("bun spawn: {e}"),
            };
        }
    };
    if !bun_out.status.success() {
        let _ = std::fs::remove_file(&tmp_path);
        return Outcome::BunSkip;
    }

    let tr = Command::new(tr_bin)
        .args(["run", &tmp_path.to_string_lossy()])
        .env("TORAJS_NO_CACHE", "1")
        .output();
    let _ = std::fs::remove_file(&tmp_path);
    let tr_out = match tr {
        Ok(o) => o,
        Err(e) => {
            return Outcome::HarnessError {
                msg: format!("tr spawn: {e}"),
            };
        }
    };

    if tr_out.status.success() && tr_out.stdout == bun_out.stdout {
        return Outcome::Pass;
    }

    let stderr = String::from_utf8_lossy(&tr_out.stderr).into_owned();
    let first_line = stderr.lines().next().unwrap_or("(no stderr)").to_string();

    // Subset-boundary signals: tr deliberately rejects out-of-subset
    // syntax / surface. classify these as `incompatible` so they
    // don't pollute the "bug" bucket.
    let incompat_kind = if first_line.starts_with("lex error:") {
        Some("lex error")
    } else if first_line.starts_with("parse error:") {
        Some("parse error")
    } else if first_line.starts_with("type error:") {
        Some("type error")
    } else if first_line.starts_with("not yet supported:") {
        Some("not yet supported")
    } else if first_line.starts_with("import error:") {
        Some("import error")
    } else if first_line.starts_with("compile error:") {
        Some("compile error")
    } else {
        None
    };

    if let Some(kind) = incompat_kind {
        return Outcome::Incompatible {
            kind: kind.to_string(),
        };
    }

    // Non-subset-boundary failure: either tr crashed (sigsegv,
    // abort, panic without "panic" prefix) or it exited with a
    // different stdout — that's a real bug or unexpected divergence.
    let kind = if !tr_out.status.success() {
        if let Some(code) = tr_out.status.code() {
            format!("exit {code}")
        } else {
            "killed".to_string()
        }
    } else {
        "stdout-mismatch".to_string()
    };
    Outcome::Bug {
        kind,
        msg: first_line,
    }
}

fn main() {
    let args = parse_args();
    let root = Path::new(TEST262_ROOT);
    if !root.is_dir() {
        eprintln!(
            "error: {} not found. Run `git clone --depth 1 https://github.com/tc39/test262 vendor/test262` from the repo root.",
            root.display()
        );
        std::process::exit(2);
    }

    let harness = match read_harness() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };

    // tr binary path. The bench harness builds it via cargo; we
    // assume the workspace's `target/release/tr` is current — caller
    // should `cargo build --release -p tr` before running.
    let tr_bin = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("target/release/tr");
    if !tr_bin.is_file() {
        eprintln!(
            "error: {} not found. Build first: `cargo build --release -p tr`.",
            tr_bin.display()
        );
        std::process::exit(2);
    }

    let test_dir = root.join("test");
    let cases = collect_cases(&test_dir, args.filter.as_deref());
    let total = cases.len();
    let to_run = match args.limit {
        Some(n) => n.min(total),
        None => total,
    };
    let cases: Vec<PathBuf> = cases.into_iter().take(to_run).collect();

    println!(
        "torajs-test262 — {to_run} cases (of {total} total under test/), {} workers",
        args.workers
    );

    let queue = Mutex::new(0usize);
    let pass = AtomicUsize::new(0);
    let bun_skip = AtomicUsize::new(0);
    let incompat: Mutex<std::collections::HashMap<String, usize>> =
        Mutex::new(std::collections::HashMap::new());
    let bugs: Mutex<Vec<(PathBuf, String, String)>> = Mutex::new(Vec::new());
    let harness_err: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());

    let progress = AtomicUsize::new(0);
    let start = Instant::now();

    std::thread::scope(|scope| {
        for slot in 0..args.workers {
            let queue = &queue;
            let cases = &cases;
            let harness = &harness;
            let tr_bin = &tr_bin;
            let pass = &pass;
            let bun_skip = &bun_skip;
            let incompat = &incompat;
            let bugs = &bugs;
            let harness_err = &harness_err;
            let progress = &progress;
            scope.spawn(move || loop {
                let idx = {
                    let mut g = queue.lock().unwrap();
                    let i = *g;
                    *g += 1;
                    i
                };
                if idx >= cases.len() {
                    break;
                }
                let p = &cases[idx];
                let outcome = run_case(p, harness, tr_bin, slot);
                match outcome {
                    Outcome::Pass => {
                        pass.fetch_add(1, Ordering::Relaxed);
                    }
                    Outcome::BunSkip => {
                        bun_skip.fetch_add(1, Ordering::Relaxed);
                    }
                    Outcome::Incompatible { kind } => {
                        let mut m = incompat.lock().unwrap();
                        *m.entry(kind).or_insert(0) += 1;
                    }
                    Outcome::Bug { kind, msg } => {
                        let mut v = bugs.lock().unwrap();
                        v.push((p.clone(), kind, msg));
                    }
                    Outcome::HarnessError { msg } => {
                        let mut v = harness_err.lock().unwrap();
                        v.push((p.clone(), msg));
                    }
                }
                let n = progress.fetch_add(1, Ordering::Relaxed) + 1;
                if n.is_multiple_of(200) {
                    let pct = (n as f64 / cases.len() as f64) * 100.0;
                    let elapsed = start.elapsed().as_secs_f64();
                    let rate = n as f64 / elapsed;
                    print!("  [{n}/{total} {pct:.1}% — {rate:.0}/s]\r", total = cases.len());
                    let _ = std::io::stdout().flush();
                }
            });
        }
    });

    let pass = pass.load(Ordering::Relaxed);
    let bun_skip = bun_skip.load(Ordering::Relaxed);
    let incompat = incompat.into_inner().unwrap();
    let incompat_total: usize = incompat.values().sum();
    let bugs = bugs.into_inner().unwrap();
    let harness_err = harness_err.into_inner().unwrap();
    let elapsed = start.elapsed().as_secs_f64();

    // "In-scope" = cases bun executed successfully and torajs at least
    // attempted (so excluding bun-skip and harness-error). Within that,
    // `incompatible` are torajs's documented subset-boundary rejects;
    // `bug` are unexpected divergences (subset slice we *should* pass
    // but don't yet); `pass` are three-way-agreed.
    let in_scope = pass + bugs.len() + incompat_total;
    let tr_accepted = pass + bugs.len();
    let pass_rate_in_scope = if in_scope > 0 {
        (pass as f64 / in_scope as f64) * 100.0
    } else {
        0.0
    };
    let pass_rate_tr_accepted = if tr_accepted > 0 {
        (pass as f64 / tr_accepted as f64) * 100.0
    } else {
        0.0
    };

    println!("\n\n=== test262 baseline ===");
    println!("ran           : {} cases ({elapsed:.1}s)", cases.len());
    println!("pass          : {pass}");
    println!("bug           : {}", bugs.len());
    println!("incompatible  : {incompat_total}  (subset-boundary rejects)");
    println!("bun-skip      : {bun_skip}  (oracle non-zero — negative tests / harness)");
    println!("harness-error : {}  (runner-side issue)", harness_err.len());
    println!();
    println!(
        "pass rate over in-scope (pass / (pass + bug + incompatible)): {pass_rate_in_scope:.2}%  ({pass}/{in_scope})"
    );
    println!(
        "pass rate over tr-accepted (pass / (pass + bug)):             {pass_rate_tr_accepted:.2}%  ({pass}/{tr_accepted})"
    );

    let mut incompat_sorted: Vec<(String, usize)> = incompat.into_iter().collect();
    incompat_sorted.sort_by_key(|b| std::cmp::Reverse(b.1));
    if !incompat_sorted.is_empty() {
        println!("\nincompatibility breakdown:");
        for (k, v) in &incompat_sorted {
            println!("  {v:>6}  {k}");
        }
    }

    if !bugs.is_empty() {
        let limit = bugs.len().min(args.report_bugs);
        println!(
            "\nfirst {limit} bug-classified failures (of {} total):",
            bugs.len()
        );
        for (p, kind, msg) in bugs.iter().take(limit) {
            let rel = p.strip_prefix(root).unwrap_or(p);
            println!("  [{kind}] {}: {msg}", rel.display());
        }
    }

    if !harness_err.is_empty() {
        println!("\nharness errors (first 5):");
        for (p, msg) in harness_err.iter().take(5) {
            println!("  {}: {msg}", p.display());
        }
    }
}
