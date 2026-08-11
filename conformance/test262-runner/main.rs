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
//!      classify with a `no-oracle:` kind prefix. `flags: [async]`
//!      cases additionally judge on the `$DONE` completion marker
//!      (`Test262:AsyncTestComplete` on stdout) — exit 0 alone is
//!      not a pass, and bun printing the failure marker demotes it
//!      from oracle to the no-oracle path.
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
mod fixtures;
mod frontmatter;
mod harness_shake;
mod summary_json;
mod verdict;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use crate::args::parse_args;
use crate::summary_json::{detect_head_sha, now_rfc3339, write_summary_json};

const TEST262_ROOT: &str = "vendor/test262";
/// Typed harness path (relative to repo root). Replaces test262's
/// stock `harness/sta.js` + `harness/assert.js` — those are untyped
/// JS and would trip torajs's typecheck on the very first line. The
/// typed harness exposes top-level generic fns (`__t262_*`) that the
/// source-rewrite layer points every `assert.X(...)` call site at.
const TORAJS_HARNESS: &str = "conformance/test262-harness.ts";

use crate::verdict::Outcome;

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
///
/// `var` passes through UNCHANGED. A `var ` → `let ` rewrite lived here
/// from before tr parsed `var` at all, and outlived its need: the two
/// keywords do not mean the same thing (function scope + hoisting vs
/// block scope + TDZ), and a duplicate `var` — legal per §14.3.2 —
/// became a duplicate `let`, which is a real SyntaxError. Both the
/// oracle and tr read the rewritten source, so neither the passes nor
/// the rejections it produced were statements about the case as
/// written.
///
/// What this DOESN'T do: untyped fn-decl parameter annotation,
/// `null` / `undefined` literals, or features like Proxy. Those hit
/// torajs's subset boundary directly and the case stays classified
/// `incompatible` until a bigger transform layer or substrate
/// change addresses them. `==` / `!=` pass through UNCHANGED — the
/// substrate implements IsLooselyEqual (§7.2.14) for real (RFC
/// 20260713-loose-eq-substrate blade 4 retired the old `==` →
/// `===` byte rewrite, which had been erasing loose-equality
/// semantics across the whole corpus).
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
                let is_escape = c == b'\\' && i + 1 < bytes.len();
                copy_utf8_char(bytes, &mut i, &mut out);
                if is_escape {
                    copy_utf8_char(bytes, &mut i, &mut out);
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
                copy_utf8_char(bytes, &mut i, &mut out);
            }
            continue;
        }
        // `/* ... */` block comment.
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            out.push('/');
            out.push('*');
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                copy_utf8_char(bytes, &mut i, &mut out);
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
                // Must sit in this table (tried before the
                // `assert.throws(` class-arg-drop block below, whose
                // needle can't match this one anyway — after "throws"
                // it requires '('): unlike assert.throws, the class
                // arg is KEPT — `.constructor === Class` compares
                // bun-equal on tr, so the port keeps full fidelity.
                (b"assert.throwsAsync(", "__t262_throwsAsync("),
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
            (
                b"verifyPrimordialCallableProperty(",
                "__t262_verifyCallableProperty(",
            ),
            (b"verifyEqualTo(", "__t262_verifyEqualTo("),
            (b"isConfigurable(", "__t262_isConfigurable("),
            (b"isEnumerable(", "__t262_isEnumerable("),
            (b"isSameValue(", "__t262_isSameValue("),
            (b"isWritable(", "__t262_isWritable("),
            (b"isConstructor(", "__t262_isConstructor("),
            (b"assertRelativeDateMs(", "__t262_assertRelativeDateMs("),
            (b"asyncTest(", "__t262_asyncTest("),
            (b"fnGlobalObject(", "__t262_fnGlobalObject("),
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
        copy_utf8_char(bytes, &mut i, &mut out);
    }
    out
}

/// Copy the UTF-8 character starting at `bytes[*i]` into `out`, advancing
/// `*i` past its full byte length (1-4). Pre-fix the transform iterated
/// byte-by-byte with `out.push(bytes[i] as char)` — that mapped every
/// non-ASCII UTF-8 continuation byte to a bogus Latin-1 code point (a
/// leading `D0 90` → `Ð\u{90}`), so every cyrillic / cjk / emoji literal
/// in a case body was destroyed. The rewrite patterns are all pure ASCII
/// (assert.X, var, __t262_*), so all we need is a UTF-8-safe fallback
/// copy for everything the pattern loop doesn't consume.
fn copy_utf8_char(bytes: &[u8], i: &mut usize, out: &mut String) {
    let start = *i;
    let b = bytes[start];
    let len = if b < 0x80 {
        1
    } else if b < 0xC0 {
        // Continuation / invalid lead — advance one byte and skip
        // (upstream `src.as_bytes()` came from a `&str`, so a bare
        // continuation byte can only appear inside a broken slice
        // window; falling back to 1 keeps the loop bounded).
        1
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    };
    let end = (start + len).min(bytes.len());
    if let Ok(s) = std::str::from_utf8(&bytes[start..end]) {
        out.push_str(s);
    } else {
        // Byte was invalid UTF-8 in isolation; emit U+FFFD so the
        // downstream lexer still gets valid input.
        out.push(char::REPLACEMENT_CHARACTER);
    }
    *i = end;
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
    harness: &harness_shake::HarnessIndex,
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
        // S-NEW 刀 3 — real `isConstructor`, once `__t262_isConstructor`
        // stopped being a stub that answered true to everything.
        "isConstructor.js",
        // 2026-07-30 — real `__t262_asyncTest` + `__t262_throwsAsync`
        // (constructor comparison kept, see the harness port note).
        "asyncHelpers.js",
        // 2026-08-11 — real global object: the stock body's
        // `Function("return this;")()` runs on tr and answers the G2
        // globalThis singleton, so the port returns it directly.
        "fnGlobalObject.js",
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

    let mut transformed = transform_source(&case_src);
    // asyncHelpers port — stock `asyncTest` probes the async flag via
    // `Object.hasOwn(globalThis, "$DONE")`; tr's harness reads the
    // same fact from a runner-set global instead. Inject the
    // assignment only for flagged cases that actually call asyncTest:
    // an unflagged caller keeps the flag false and gets the stock
    // Test262Error throw. The line lands in `transformed` BEFORE the
    // harness shake and the oracle-cache key, so both see it.
    if fm.is_async() && transformed.contains("__t262_asyncTest(") {
        transformed = format!("__t262_async_flag = true;\n{transformed}");
    }
    // RFC 20260724-test262-harness-preamble blade 1 — prepend only
    // the harness segments this case references (dep-closed), not
    // the full ~700-line harness: the tr front-end otherwise spends
    // ~90% of its per-case time re-compiling identical helpers.
    let harness_min = harness.minimal_for(&transformed);
    let full = format!("{harness_min}\n{transformed}");

    // RFC 20260810-sloppy-goal-arguments S1 — noStrict cases run
    // under the sloppy goal only (official test262 host convention:
    // the strict twin is what onlyStrict / the strict pass of an
    // unflagged case would be). `.cts` is bun's extension-mapped
    // CommonJS sloppy goal, and tr adopts the same mapping — one tmp
    // file gives both runtimes the same goal.
    let ext = if fm.is_no_strict() { "cts" } else { "ts" };

    // `--dump-src`: persist the assembled source for runner-isomorphic
    // reproduction (byte-identical to the tmp file executed below,
    // same extension so the goal survives the round trip).
    if let Some(dir) = dump_src {
        let rel = path
            .strip_prefix(TEST262_ROOT)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('/', "__");
        let _ = std::fs::write(dir.join(format!("{rel}.{ext}")), &full);
    }

    // Distinct tmp SUBDIRECTORY per worker slot to avoid races —
    // sibling `*_FIXTURE.js` files stage beside the assembled case
    // so its relative module imports resolve (fixtures module doc).
    // `.ts` (or `.cts` for the sloppy goal) so tr's read_source
    // treats it as a normal source file.
    let slot_dir =
        std::env::temp_dir().join(format!("torajs-test262-{}-{}", std::process::id(), slot));
    let fixture_salt = fixtures::stage(&slot_dir, path, &case_src);
    let tmp_path = slot_dir.join(format!("case.{ext}"));
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
    //
    // Keyed on the TRANSFORMED source, which is what bun is handed.
    // Keying on the case as it sits in the corpus left every entry
    // valid across a change to `transform_source`, so the next sweep
    // silently scored tr against the previous transform's oracle —
    // the shape of wrongness that dropping the `var` rewrite exposed
    // (298 cases came back carrying the rewritten program's verdict).
    // Fixture-staging cases salt the oracle key with the staged
    // fixture bytes: their cached verdicts predate resolvable
    // imports (fixtures module doc). Fixture-less cases keep their
    // existing entries.
    // The sloppy goal salts the oracle key too: the same transformed
    // bytes behave differently as `.cts` vs `.ts` (mapped arguments,
    // callee, ...), and an unsalted key would score sloppy runs
    // against strict-goal oracle entries cached before S1.
    let salted_case: Vec<u8>;
    let oracle_case: &[u8] = if fixture_salt.is_empty() && !fm.is_no_strict() {
        transformed.as_bytes()
    } else {
        let mut v = transformed.as_bytes().to_vec();
        if !fixture_salt.is_empty() {
            v.push(0xff);
            v.extend_from_slice(&fixture_salt);
        }
        if fm.is_no_strict() {
            v.extend_from_slice(b"\xfegoal:sloppy");
        }
        salted_case = v;
        &salted_case
    };
    let (bun_success, bun_stdout) =
        match cache::bun_oracle(oracle_case, harness_min.as_bytes(), &tmp_path) {
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

    // `flags: [async]` — the doneprintHandle.js protocol makes the
    // exit code meaningless (`$DONE(err)` prints a failure marker and
    // exits 0). bun only counts as a live oracle when it actually
    // completed; matching bun on a run where bun itself printed the
    // failure marker would score a shared failure as a pass.
    let is_async = fm.is_async();
    let bun_ok = bun_success && (!is_async || verdict::async_completed(&bun_stdout));

    if bun_ok {
        verdict::judge_oracle(&out, &bun_stdout)
    } else {
        // bun itself failed — not a skip (takagi 2026-06-13): test262
        // positive cases self-validate through the assert harness.
        verdict::judge_no_oracle(&out, is_async)
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
    let harness = harness_shake::HarnessIndex::build(&harness);

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
    // `--incompat-ndjson` — the full incompatible list, not the 30-per-kind
    // sample above. Only filled when the flag is on: the corpus rejects
    // ~39k cases, and carrying every (path, kind, msg) triple costs a few
    // MB that a plain sweep has no use for.
    let incompat_all: Mutex<Vec<(PathBuf, String, String)>> = Mutex::new(Vec::new());
    let bugs: Mutex<Vec<(PathBuf, String, String)>> = Mutex::new(Vec::new());
    let harness_err: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());
    // `--dump-verdicts` — one line per case, so two runs diff into the
    // list of cases that MOVED. An aggregate delta ("passTotal -19")
    // can't distinguish a regression from a by-design reclassification.
    let verdicts: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());

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
            let incompat_all = &incompat_all;
            let want_incompat_all = args.incompat_ndjson.is_some();
            let bugs = &bugs;
            let harness_err = &harness_err;
            let verdicts = &verdicts;
            let want_verdicts = args.dump_verdicts.is_some();
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
                    if want_verdicts {
                        let label = match &outcome {
                            Outcome::Pass => "pass".to_string(),
                            Outcome::PassNoOracle => "pass-no-oracle".to_string(),
                            Outcome::PassNegative => "pass-negative".to_string(),
                            Outcome::Incompatible { kind, .. } => format!("incompatible:{kind}"),
                            Outcome::Bug { kind, .. } => format!("bug:{kind}"),
                            Outcome::HarnessError { .. } => "harness-error".to_string(),
                        };
                        verdicts.lock().unwrap().push((p.clone(), label));
                    }
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
                            if want_incompat_all {
                                incompat_all.lock().unwrap().push((
                                    p.clone(),
                                    kind.clone(),
                                    msg.clone(),
                                ));
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
    let incompat_all = incompat_all.into_inner().unwrap();
    let incompat_total: usize = incompat.values().sum();
    let verdicts = verdicts.into_inner().unwrap();
    if let Some(path) = &args.dump_verdicts {
        let mut lines: Vec<String> = verdicts
            .iter()
            .map(|(p, label)| {
                let rel = p.strip_prefix(root).unwrap_or(p);
                format!("{}\t{label}", rel.display())
            })
            .collect();
        lines.sort();
        match std::fs::write(path, lines.join("\n") + "\n") {
            Ok(()) => println!("\nverdicts: {path} ({} cases)", lines.len()),
            Err(e) => eprintln!("error writing verdicts to {path}: {e}"),
        }
    }
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
        match bugdump::write_cases_ndjson(Path::new(out_path), root, &bugs) {
            Ok(()) => println!("\nbugs ndjson: {out_path} ({} cases)", bugs.len()),
            Err(e) => eprintln!("warn: --bugs-ndjson write to {out_path}: {e}"),
        }
    }

    if let Some(out_path) = args.incompat_ndjson.as_deref() {
        match bugdump::write_cases_ndjson(Path::new(out_path), root, &incompat_all) {
            Ok(()) => println!(
                "\nincompat ndjson: {out_path} ({} cases)",
                incompat_all.len()
            ),
            Err(e) => eprintln!("warn: --incompat-ndjson write to {out_path}: {e}"),
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

#[cfg(test)]
mod transform_tests {
    use super::transform_source;

    #[test]
    fn utf8_cyrillic_literal_preserved_in_string_body() {
        // Pre-fix `out.push(bytes[i] as char)` mapped `А` (U+0410,
        // UTF-8 = D0 90) to `Ð\u{90}` — the S7.8.4_A2.2 / A4.2_T5-7
        // cases compared such a literal against `String.fromCharCode(0x0410)`
        // and every assertion failed with a mangled `#Ð` label.
        let src = "let x = \"\u{0410}\u{0451}\u{4E2D}\";";
        let out = transform_source(src);
        assert!(
            out.contains("\u{0410}") && out.contains("\u{0451}") && out.contains("\u{4E2D}"),
            "expected UTF-8 code points to survive rewrite, got: {out:?}"
        );
    }

    #[test]
    fn utf8_emoji_in_comment_preserved() {
        let src = "// pile of poo: \u{1F4A9}\nlet x = 1;";
        let out = transform_source(src);
        assert!(out.contains("\u{1F4A9}"), "emoji lost: {out:?}");
    }

    #[test]
    fn ascii_rewrites_still_fire() {
        // Sanity — the rewrite pattern side stays functional after
        // switching the fallback copy to UTF-8-safe.
        let src = "assert.sameValue(a, b);\nvar x = 1;\n";
        let out = transform_source(src);
        assert!(
            out.contains("__t262_sameValue("),
            "assert.sameValue rewrite lost: {out:?}"
        );
        assert!(
            out.contains("var x = 1;"),
            "var declaration mangled: {out:?}"
        );
    }

    #[test]
    fn async_helpers_rewrites_fire_and_guard() {
        // `asyncTest(` is a bare-call rewrite; member access and
        // longer identifiers must not match. `assert.throwsAsync(`
        // keeps ALL args (the class arg is compared for real, unlike
        // `assert.throws` whose rewrite drops it).
        let src = "asyncTest(async function () {});\n\
                   obj.asyncTest(1);\n\
                   myasyncTest(2);\n\
                   await assert.throwsAsync(TypeError, () => p, 'msg');\n\
                   assert.throws(TypeError, function () {});\n";
        let out = transform_source(src);
        assert!(
            out.contains("__t262_asyncTest(async function"),
            "bare asyncTest rewrite lost: {out:?}"
        );
        assert!(
            out.contains("obj.asyncTest(1)") && out.contains("myasyncTest(2)"),
            "guarded asyncTest sites must stay verbatim: {out:?}"
        );
        assert!(
            out.contains("__t262_throwsAsync(TypeError, () => p, 'msg')"),
            "throwsAsync must keep the class arg: {out:?}"
        );
        assert!(
            out.contains("__t262_throws_runtime(function () {})"),
            "assert.throws class-arg drop must survive the new needle: {out:?}"
        );
    }

    #[test]
    fn var_is_carried_through_verbatim() {
        // The runner used to rewrite `var ` -> `let ` — a crutch from
        // before tr parsed `var` at all. It outlived its need and had
        // become a source of wrong verdicts in both directions: the two
        // keywords do not mean the same thing (function scope +
        // hoisting vs block scope + TDZ), and a duplicate `var` — legal
        // per §14.3.2 — turned into a duplicate `let`, which is a real
        // SyntaxError. Both the oracle and tr saw the rewritten source,
        // so neither the passes nor the rejections it produced were
        // statements about the case as written.
        let src = "var x = 1;\nvar x = 2;\ntokenCodes.var = 'var';\n";
        let out = transform_source(src);
        assert_eq!(out, src, "source must reach the engines unrewritten");
    }
}
