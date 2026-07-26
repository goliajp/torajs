//! CLI argument parsing for the test262 runner.
//!
//! Extracted from main.rs to keep that file under the file-size.md
//! HARD RULE ≤ 500 LOC / file (known debt: still over until further
//! refactor; this extraction stops the bleed).

/// Fallback used when `std::thread::available_parallelism()` fails
/// (extremely rare — sandbox or truly minimal envs). Runtime callers
/// should prefer [`default_workers`].
pub const DEFAULT_WORKERS: usize = 8;
pub const DEFAULT_REPORT_BUGS: usize = 20;

/// Default worker count: auto-detects logical core count via
/// `std::thread::available_parallelism()` and clamps to 10 to avoid
/// M-series E-core efficiency drag(mini M4 Pro 10P+4E cores empirical
/// @ rotation 199 chunk 4:workers=10 -22.3% sweep wall vs workers=8;
/// 12/14 未测,10 = P-core count 是 saturation 上限,更多 workers 会拉
/// E-core 单线程效率低 30-40% + 每 case tr+bun 双级 spawn = workers×2
/// process 争 mmap/fs 恶化)。其它 host clamp to logical core count 保守
/// (M2 Pro 8-core → 8 / M1 4-core → 4)。CLI `--workers N` override 优先。
pub fn default_workers() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().min(10))
        .unwrap_or(DEFAULT_WORKERS)
}

pub struct Args {
    pub limit: Option<usize>,
    pub filter: Option<String>,
    pub workers: usize,
    pub report_bugs: usize,
    pub json_out: Option<String>,
    /// `--bugs-ndjson PATH` flag: dump every bug-classified case (the
    /// full set, not just the `--report-bugs` head) to PATH as one JSON
    /// object per line (`{"path","kind","msg"}`). Feeds root-cause
    /// clustering of the bug corpus — `kind` carries the exit code
    /// (138/139 = silent crash, 1 = loud fail, stdout-mismatch), `msg`
    /// the stderr first line (the substrate-gap signal).
    pub bugs_ndjson: Option<String>,
    /// `--incompat-ndjson PATH`: same shape as `--bugs-ndjson`, for the
    /// `incompatible` bucket instead. The stdout breakdown only carries
    /// the six `incompat_kind` prefixes and the sample list is capped at
    /// 30 per kind, so neither can answer "how much of the incompatible
    /// mass sits behind one substrate gap". This dumps every case so a
    /// downstream pass can cluster on `msg`.
    pub incompat_ndjson: Option<String>,
    /// `--dump-verdicts PATH`: write one `<rel-path>\t<outcome>` line per
    /// case. Two runs diffed against each other are what turns a sweep's
    /// aggregate delta ("passTotal -19") into an attributable list of
    /// cases that moved, which is the only way to tell a real regression
    /// from a by-design reclassification.
    pub dump_verdicts: Option<String>,
    /// `--no-cache` flag: skip the bun oracle cache (cache::lookup
    /// returns None even on disk hit). Default false → cache enabled.
    /// Useful for benchmarking the runner itself or after a corpus /
    /// harness change that hasn't yet invalidated the hash key.
    pub no_cache: bool,
    /// `--dump-src DIR` flag: also write each assembled source (typed
    /// harness + transformed case, byte-identical to the tmp file the
    /// worker executes) into DIR, named by the case path with `/`
    /// flattened to `__`. Feeds runner-isomorphic crash reproduction —
    /// rerunning the dumped file under `tr run` reproduces exactly
    /// what the worker ran. Use with `--filter` to keep DIR small.
    pub dump_src: Option<String>,
}

pub fn parse_args() -> Args {
    let mut limit: Option<usize> = None;
    let mut filter: Option<String> = None;
    let mut workers = default_workers();
    let mut report_bugs = DEFAULT_REPORT_BUGS;
    let mut json_out: Option<String> = None;
    let mut bugs_ndjson: Option<String> = None;
    let mut incompat_ndjson: Option<String> = None;
    let mut dump_verdicts: Option<String> = None;
    let mut no_cache = false;
    let mut dump_src: Option<String> = None;
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
            "--json" => json_out = iter.next(),
            "--bugs-ndjson" => bugs_ndjson = iter.next(),
            "--incompat-ndjson" => incompat_ndjson = iter.next(),
            "--dump-verdicts" => dump_verdicts = iter.next(),
            "--no-cache" => no_cache = true,
            "--dump-src" => dump_src = iter.next(),
            "-h" | "--help" => {
                let default_w = default_workers();
                eprintln!(
                    "torajs-test262 — run tc39/test262 against tr\n\n\
                     flags:\n  \
                     --limit N       only first N cases\n  \
                     --filter STR    cases whose path contains STR\n  \
                     --workers N     concurrency (default {default_w}, auto-detected via available_parallelism clamped to 10)\n  \
                     --report-bugs N list first N bug failures (default {DEFAULT_REPORT_BUGS})\n  \
                     --json PATH     also write machine-readable summary to PATH\n  \
                     --bugs-ndjson PATH  dump every bug case (path/kind/msg) as ndjson for clustering\n  \
                     --incompat-ndjson PATH  same, for the incompatible bucket (stdout only shows kind counts + 30 samples/kind)\n  \
                     --dump-verdicts PATH  write `<case>\\t<outcome>` per case; diff two runs to attribute a sweep delta\n  \
                     --no-cache      bypass the bun oracle cache for this run\n  \
                     --dump-src DIR  also write each assembled source (harness + transformed case) into DIR"
                );
                std::process::exit(0);
            }
            other => {
                eprintln!("error: unknown arg `{other}`");
                std::process::exit(2);
            }
        }
    }
    Args {
        limit,
        filter,
        workers,
        report_bugs,
        json_out,
        bugs_ndjson,
        incompat_ndjson,
        dump_verdicts,
        no_cache,
        dump_src,
    }
}
