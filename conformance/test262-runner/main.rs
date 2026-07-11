//! tc39/test262 runner — measure torajs's conformance over the
//! subset-compatible slice of test262.
//!
//! Pipeline per case:
//!   1. Read the case's source from `vendor/test262/test/.../case.js`.
//!   2. Prepend the standard test262 harness (sta.js + assert.js).
//!   3. Parse the frontmatter. `negative:` cases are judged against
//!      their declared phase (the frontmatter IS the oracle — bun is
//!      not consulted); `includes:` beyond assert/sta classify as
//!      `harness-includes` until the typed harness ports them.
//!   4. Positive cases: run `bun run` (oracle). bun passing → tr must
//!      match exit 0 + stdout. bun FAILING is NOT a skip (takagi
//!      2026-06-13): the assert harness self-validates, so tr is
//!      judged directly — exit 0 = `pass-no-oracle`, failures
//!      classify with a `no-oracle:` kind prefix.
//!   5. Categorize (see verdict.rs for the full table):
//!     - pass / pass-no-oracle / pass-negative
//!     - bug: unexpected divergence (real-bug bucket)
//!     - incompatible: documented subset-boundary rejects
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

mod args;
mod bugdump;
mod cache;
mod frontmatter;
mod verdict;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use crate::args::parse_args;

const TEST262_ROOT: &str = "vendor/test262";
/// Typed harness path (relative to repo root). Replaces test262's
/// stock `harness/sta.js` + `harness/assert.js` — those are untyped
/// JS and would trip torajs's typecheck on the very first line. The
/// typed harness exposes top-level generic fns (`__t262_*`) that the
/// source-rewrite layer points every `assert.X(...)` call site at.
const TORAJS_HARNESS: &str = "conformance/test262-harness.ts";

use crate::verdict::Outcome;

/// Minimal hand-rolled JSON object writer (test262-runner is zero-dep).
/// Escapes `"` and `\` in strings; everything else assumed ASCII-safe.
fn write_summary_json(
    out: &Path,
    ran_at: &str,
    head_sha: &str,
    elapsed_sec: f64,
    workers: usize,
    limit: Option<usize>,
    total_cases: usize,
    ran: usize,
    pass: usize,
    pass_no_oracle: usize,
    pass_negative: usize,
    bug: usize,
    incompatible: usize,
    bun_fail: usize,
    harness_error: usize,
    in_scope: usize,
    tr_accepted: usize,
    pass_rate_in_scope: f64,
    pass_rate_tr_accepted: f64,
) -> std::io::Result<()> {
    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }
    let limit_json = match limit {
        Some(n) => n.to_string(),
        None => "null".to_string(),
    };
    // `bunSkip` keeps its key for dashboard compatibility but now
    // counts bun failures that were still judged (no case is skipped).
    let body = format!(
        "{{\n  \"ranAt\": \"{ra}\",\n  \"headSha\": \"{hs}\",\n  \"elapsedSec\": {es:.2},\n  \"workers\": {w},\n  \"limit\": {lj},\n  \"totalCases\": {tc},\n  \"ran\": {ran},\n  \"pass\": {pass},\n  \"passNoOracle\": {pno},\n  \"passNegative\": {png},\n  \"passTotal\": {ptot},\n  \"bug\": {bug},\n  \"incompatible\": {inc},\n  \"bunSkip\": {bs},\n  \"harnessError\": {he},\n  \"inScope\": {is_},\n  \"trAccepted\": {ta},\n  \"passRateInScope\": {pris:.2},\n  \"passRateTrAccepted\": {prta:.2}\n}}\n",
        ra = esc(ran_at),
        hs = esc(head_sha),
        es = elapsed_sec,
        w = workers,
        lj = limit_json,
        tc = total_cases,
        ran = ran,
        pass = pass,
        pno = pass_no_oracle,
        png = pass_negative,
        ptot = pass + pass_no_oracle + pass_negative,
        bug = bug,
        inc = incompatible,
        bs = bun_fail,
        he = harness_error,
        is_ = in_scope,
        ta = tr_accepted,
        pris = pass_rate_in_scope,
        prta = pass_rate_tr_accepted,
    );
    let mut f = std::fs::File::create(out)?;
    f.write_all(body.as_bytes())?;
    Ok(())
}

/// Read git HEAD short sha from `.git/HEAD` chain, falling back to
/// `git rev-parse` if the chain doesn't resolve to a packed/loose ref.
/// Best-effort — returns "unknown" on any error since the JSON consumer
/// must tolerate it (cold-start invocations may not be in a git repo).
fn detect_head_sha() -> String {
    if let Ok(out) = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        && out.status.success()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return s;
        }
    }
    "unknown".to_string()
}

/// RFC 3339 UTC timestamp without external deps. Uses libc gmtime.
fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Convert epoch seconds to civil date via Howard Hinnant's algorithm.
    let z = secs as i64;
    let days = z.div_euclid(86_400);
    let secs_of_day = z.rem_euclid(86_400);
    let hh = secs_of_day / 3600;
    let mm = (secs_of_day / 60) % 60;
    let ss = secs_of_day % 60;
    let z_days = days + 719_468;
    let era = if z_days >= 0 {
        z_days
    } else {
        z_days - 146_096
    } / 146_097;
    let doe = (z_days - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z",
        y = y,
        m = m,
        d = d,
        hh = hh,
        mm = mm,
        ss = ss,
    )
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
            let stem = p.file_name().and_then(|s| s.to_str()).unwrap_or_default();
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
                (b"assert.compareArray(", "__t262_compareArray_assert("),
                (b"assert.deepEqual(", "__t262_deepEqual("),
                (b"assert.compareIterator(", "__t262_compareIterator("),
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
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
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
        // 2026-05-18 — test262 helper rewrites. Each replaces a bare
        // call with the matching `__t262_*` shim defined in
        // test262-harness.ts. Order: longer needles first (no
        // ambiguity here since all are distinct identifiers).
        const T262_HELPER_REWRITES: &[(&[u8], &str)] = &[
            (b"verifyProperty(", "__t262_verifyProperty("),
            (b"compareArray(", "__t262_compareArray("),
            (b"verifyConfigurable(", "__t262_verifyConfigurable("),
            (b"verifyEnumerable(", "__t262_verifyEnumerable("),
            (b"verifyWritable(", "__t262_verifyWritable("),
            (b"verifyNotConfigurable(", "__t262_verifyNotConfigurable("),
            (b"verifyNotEnumerable(", "__t262_verifyNotEnumerable("),
            (b"verifyNotWritable(", "__t262_verifyNotWritable("),
            (b"verifyCallableProperty(", "__t262_verifyCallableProperty("),
            (b"verifyPrimordialProperty(", "__t262_verifyProperty("),
            (b"verifyPrimordialCallableProperty(", "__t262_verifyCallableProperty("),
            (b"verifyEqualTo(", "__t262_verifyEqualTo("),
            (b"isConfigurable(", "__t262_isConfigurable("),
            (b"isEnumerable(", "__t262_isEnumerable("),
            (b"isSameValue(", "__t262_isSameValue("),
            (b"isWritable(", "__t262_isWritable("),
            (b"isConstructor(", "__t262_isConstructor("),
            (b"assertRelativeDateMs(", "__t262_assertRelativeDateMs("),
        ];
        let mut hit_helper = false;
        for (needle, replacement) in T262_HELPER_REWRITES {
            if starts_with_at(bytes, i, needle)
                && !preceded_by_dot(bytes, i)
                && !preceded_by_word(bytes, i)
            {
                out.push_str(replacement);
                i += needle.len();
                hit_helper = true;
                break;
            }
        }
        if hit_helper {
            continue;
        }
        // `var ` → `let ` (word-boundary on the left + whitespace on the right).
        if starts_with_at(bytes, i, b"var ") && !preceded_by_word(bytes, i) {
            out.push_str("let ");
            i += b"var ".len();
            continue;
        }
        // `==` / `!=` → strict form. Skip already-strict (`===` /
        // `!==`) and the negation-then-eq combo `!==`. byte-walker
        // sees `=` first; if next is `=`, look one further:
        //   `===` (already strict) — pass through 3 bytes
        //   `==`  + non-`=` next   — rewrite to `===`
        //   `=`   + non-`=`         — assignment, pass through
        if b == b'=' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            if i + 2 < bytes.len() && bytes[i + 2] == b'=' {
                // `===` — copy verbatim
                out.push('=');
                out.push('=');
                out.push('=');
                i += 3;
                continue;
            }
            // `==` not followed by `=` — rewrite.
            out.push_str("===");
            i += 2;
            continue;
        }
        if b == b'!' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            if i + 2 < bytes.len() && bytes[i + 2] == b'=' {
                // `!==` — copy verbatim
                out.push('!');
                out.push('=');
                out.push('=');
                i += 3;
                continue;
            }
            // `!=` not followed by `=` — rewrite.
            out.push_str("!==");
            i += 2;
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

fn run_case(
    path: &Path,
    harness: &str,
    tr_bin: &Path,
    slot: usize,
    dump_src: Option<&Path>,
) -> Outcome {
    let case_src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return Outcome::HarnessError {
                msg: format!("read {}: {e}", path.display()),
            };
        }
    };
    let fm = frontmatter::parse(&case_src);

    // Harness includes beyond assert/sta classify by whether the typed
    // harness has ported them. A case whose every include is ported
    // runs normally (the `__t262_*` rewrites in transform_source bind
    // the call sites); any unported include keeps the case in the
    // harness-includes bucket — an attributable reject, NOT a silent
    // skip (the case would fail on the missing helper for bun and tr
    // alike).
    const PORTED_INCLUDES: &[&str] = &[
        "compareArray.js",
        "dateConstants.js",
        "propertyHelper.js",
        "decimalToHexString.js",
        "nans.js",
        "promiseHelper.js",
        "regExpUtils.js",
        "tcoHelper.js",
    ];
    let unported: Vec<&str> = fm
        .includes
        .iter()
        .map(String::as_str)
        .filter(|inc| !PORTED_INCLUDES.contains(inc))
        .collect();
    if !unported.is_empty() {
        return Outcome::Incompatible {
            kind: "harness-includes".to_string(),
            msg: format!("needs {}", unported.join(", ")),
        };
    }

    let transformed = transform_source(&case_src);
    let full = format!("{harness}\n{transformed}");

    // `--dump-src`: persist the assembled source for runner-isomorphic
    // reproduction (byte-identical to the tmp file executed below).
    if let Some(dir) = dump_src {
        let rel = path
            .strip_prefix(TEST262_ROOT)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('/', "__");
        let _ = std::fs::write(dir.join(format!("{rel}.ts")), &full);
    }

    // Distinct tmp file per worker slot to avoid races. Use `.ts` so
    // tr's read_source treats it as a normal source file (extension
    // isn't actually checked but the convention matches the rest of
    // the pipeline).
    let tmp_path =
        std::env::temp_dir().join(format!("torajs-test262-{}-{}.ts", std::process::id(), slot));
    if let Err(e) = std::fs::write(&tmp_path, &full) {
        return Outcome::HarnessError {
            msg: format!("write tmp: {e}"),
        };
    }

    // Negative case — the frontmatter is the oracle; bun isn't
    // consulted at all (judging it against bun's own failure modes
    // would re-introduce the bun-skip blind spot).
    if let Some(phase) = fm.negative_phase.as_deref() {
        let expected_type = fm.negative_type.as_deref().unwrap_or("?").to_string();
        let out = match verdict::run_tr(tr_bin, &tmp_path) {
            Ok(o) => o,
            Err(outcome) => {
                let _ = std::fs::remove_file(&tmp_path);
                return outcome;
            }
        };
        let _ = std::fs::remove_file(&tmp_path);
        return verdict::judge_negative(phase, &expected_type, &out);
    }

    // Positive case — bun oracle (cache lookup + spawn with 15s
    // timeout). Cache hit → 0 spawn cost. Miss → spawn bun, populate
    // cache.
    let (bun_success, bun_stdout) =
        match cache::bun_oracle(case_src.as_bytes(), harness.as_bytes(), &tmp_path) {
            Ok(v) => v,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_path);
                return Outcome::HarnessError { msg: e };
            }
        };

    let out = match verdict::run_tr(tr_bin, &tmp_path) {
        Ok(o) => o,
        Err(outcome) => {
            let _ = std::fs::remove_file(&tmp_path);
            return outcome;
        }
    };
    let _ = std::fs::remove_file(&tmp_path);

    if bun_success {
        verdict::judge_oracle(&out, &bun_stdout)
    } else {
        // bun itself failed — not a skip (takagi 2026-06-13): test262
        // positive cases self-validate through the assert harness.
        verdict::judge_no_oracle(&out)
    }
}

fn main() {
    let args = parse_args();

    cache::init_and_report(args.no_cache);

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
    // Cap ~/.torajs/cache at 5 GiB (matches conformance gate
    // policy). Without this, 53k test262 cases × ~280 KB each could
    // grow the cache to ~15 GB per full run, ballooning across
    // weeks.
    cache::prune_tr_cache(&tr_bin, 5120);

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

    let dump_src = args.dump_src.as_deref().map(Path::new);
    if let Some(dir) = dump_src {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("error: create --dump-src dir {}: {e}", dir.display());
            std::process::exit(2);
        }
    }

    let queue = Mutex::new(0usize);
    let pass = AtomicUsize::new(0);
    let pass_no_oracle = AtomicUsize::new(0);
    let pass_negative = AtomicUsize::new(0);
    let bun_fail = AtomicUsize::new(0);
    let incompat: Mutex<std::collections::HashMap<String, usize>> =
        Mutex::new(std::collections::HashMap::new());
    let incompat_samples: Mutex<std::collections::HashMap<String, Vec<(PathBuf, String)>>> =
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
            let pass_no_oracle = &pass_no_oracle;
            let pass_negative = &pass_negative;
            let bun_fail = &bun_fail;
            let incompat = &incompat;
            let incompat_samples = &incompat_samples;
            let bugs = &bugs;
            let harness_err = &harness_err;
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
                    let p = &cases[idx];
                    let outcome = run_case(p, harness, tr_bin, slot, dump_src);
                    match outcome {
                        Outcome::Pass => {
                            pass.fetch_add(1, Ordering::Relaxed);
                        }
                        Outcome::PassNoOracle => {
                            pass_no_oracle.fetch_add(1, Ordering::Relaxed);
                            bun_fail.fetch_add(1, Ordering::Relaxed);
                        }
                        Outcome::PassNegative => {
                            pass_negative.fetch_add(1, Ordering::Relaxed);
                        }
                        Outcome::Incompatible { kind, msg } => {
                            if kind.starts_with("no-oracle:") {
                                bun_fail.fetch_add(1, Ordering::Relaxed);
                            }
                            let mut m = incompat.lock().unwrap();
                            *m.entry(kind.clone()).or_insert(0) += 1;
                            drop(m);
                            let mut s = incompat_samples.lock().unwrap();
                            let v = s.entry(kind).or_default();
                            if v.len() < 30 {
                                v.push((p.clone(), msg));
                            }
                        }
                        Outcome::Bug { kind, msg } => {
                            if kind.starts_with("no-oracle:") {
                                bun_fail.fetch_add(1, Ordering::Relaxed);
                            }
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
                        print!(
                            "  [{n}/{total} {pct:.1}% — {rate:.0}/s]\r",
                            total = cases.len()
                        );
                        let _ = std::io::stdout().flush();
                    }
                }
            });
        }
    });

    let pass = pass.load(Ordering::Relaxed);
    let pass_no_oracle = pass_no_oracle.load(Ordering::Relaxed);
    let pass_negative = pass_negative.load(Ordering::Relaxed);
    let bun_fail = bun_fail.load(Ordering::Relaxed);
    let incompat = incompat.into_inner().unwrap();
    let incompat_samples = incompat_samples.into_inner().unwrap();
    let incompat_total: usize = incompat.values().sum();
    let bugs = bugs.into_inner().unwrap();
    let harness_err = harness_err.into_inner().unwrap();
    let elapsed = start.elapsed().as_secs_f64();

    // Every case is judged (no bun-skip blind spot since 2026-06-13):
    // "in-scope" = everything except runner-side harness errors.
    // `incompatible` are torajs's documented subset-boundary rejects
    // (each with an attributable kind); `bug` are unexpected
    // divergences (the slice we *should* pass but don't yet).
    let pass_total = pass + pass_no_oracle + pass_negative;
    let in_scope = pass_total + bugs.len() + incompat_total;
    let tr_accepted = pass_total + bugs.len();
    let pass_rate_in_scope = if in_scope > 0 {
        (pass_total as f64 / in_scope as f64) * 100.0
    } else {
        0.0
    };
    let pass_rate_tr_accepted = if tr_accepted > 0 {
        (pass_total as f64 / tr_accepted as f64) * 100.0
    } else {
        0.0
    };

    println!("\n\n=== test262 baseline ===");
    println!("ran           : {} cases ({elapsed:.1}s)", cases.len());
    println!("pass          : {pass}  (bun-oracle matched)");
    println!("pass-no-oracle: {pass_no_oracle}  (bun failed; assert harness self-validated)");
    println!("pass-negative : {pass_negative}  (expected error, matching phase)");
    println!("bug           : {}", bugs.len());
    println!("incompatible  : {incompat_total}  (subset-boundary rejects, attributable kinds)");
    println!("bun-fail      : {bun_fail}  (diagnostic: oracle failed but case still judged)");
    println!("harness-error : {}  (runner-side issue)", harness_err.len());
    println!();
    println!(
        "pass rate over in-scope (passes / (passes + bug + incompatible)): {pass_rate_in_scope:.2}%  ({pass_total}/{in_scope})"
    );
    println!(
        "pass rate over tr-accepted (passes / (passes + bug)):             {pass_rate_tr_accepted:.2}%  ({pass_total}/{tr_accepted})"
    );

    let mut incompat_sorted: Vec<(String, usize)> = incompat.into_iter().collect();
    incompat_sorted.sort_by_key(|b| std::cmp::Reverse(b.1));
    if !incompat_sorted.is_empty() {
        println!("\nincompatibility breakdown:");
        for (k, v) in &incompat_sorted {
            println!("  {v:>6}  {k}");
        }
    }

    if std::env::var("TORAJS_T262_DUMP_INCOMPAT").is_ok() {
        for (k, _) in &incompat_sorted {
            let Some(samples) = incompat_samples.get(k) else {
                continue;
            };
            println!("\n--- sample {k} ({} cases) ---", samples.len());
            for (p, msg) in samples.iter().take(20) {
                let rel = p.strip_prefix(root).unwrap_or(p);
                println!("  {}: {msg}", rel.display());
            }
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

    if let Some(out_path) = args.bugs_ndjson.as_deref() {
        match bugdump::write_bugs_ndjson(Path::new(out_path), root, &bugs) {
            Ok(()) => println!("\nbugs ndjson: {out_path} ({} cases)", bugs.len()),
            Err(e) => eprintln!("warn: --bugs-ndjson write to {out_path}: {e}"),
        }
    }

    if !harness_err.is_empty() {
        println!("\nharness errors (first 5):");
        for (p, msg) in harness_err.iter().take(5) {
            println!("  {}: {msg}", p.display());
        }
    }

    // --json: machine-readable summary for downstream consumers (hardev
    // dashboard's snapshot.mjs reads this, falls back to historical
    // values if absent). Written last so the JSON reflects the same
    // values shown in the stdout report above.
    if let Some(out_path) = args.json_out.as_deref() {
        let ran_at = now_rfc3339();
        let head_sha = detect_head_sha();
        if let Err(e) = write_summary_json(
            Path::new(out_path),
            &ran_at,
            &head_sha,
            elapsed,
            args.workers,
            args.limit,
            total,
            cases.len(),
            pass,
            pass_no_oracle,
            pass_negative,
            bugs.len(),
            incompat_total,
            bun_fail,
            harness_err.len(),
            in_scope,
            tr_accepted,
            pass_rate_in_scope,
            pass_rate_tr_accepted,
        ) {
            eprintln!("warn: --json write to {out_path}: {e}");
        } else {
            println!("\njson summary: {out_path}");
        }
    }
}
