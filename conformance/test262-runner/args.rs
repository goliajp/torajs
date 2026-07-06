//! CLI argument parsing for the test262 runner.
//!
//! Extracted from main.rs to keep that file under the file-size.md
//! HARD RULE ≤ 500 LOC / file (known debt: still over until further
//! refactor; this extraction stops the bleed).

pub const DEFAULT_WORKERS: usize = 8;
pub const DEFAULT_REPORT_BUGS: usize = 20;

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
    let mut workers = DEFAULT_WORKERS;
    let mut report_bugs = DEFAULT_REPORT_BUGS;
    let mut json_out: Option<String> = None;
    let mut bugs_ndjson: Option<String> = None;
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
            "--no-cache" => no_cache = true,
            "--dump-src" => dump_src = iter.next(),
            "-h" | "--help" => {
                eprintln!(
                    "torajs-test262 — run tc39/test262 against tr\n\n\
                     flags:\n  \
                     --limit N       only first N cases\n  \
                     --filter STR    cases whose path contains STR\n  \
                     --workers N     concurrency (default {DEFAULT_WORKERS})\n  \
                     --report-bugs N list first N bug failures (default {DEFAULT_REPORT_BUGS})\n  \
                     --json PATH     also write machine-readable summary to PATH\n  \
                     --bugs-ndjson PATH  dump every bug case (path/kind/msg) as ndjson for clustering\n  \
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
        no_cache,
        dump_src,
    }
}
