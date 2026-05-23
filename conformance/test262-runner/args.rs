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
    /// `--no-cache` flag: skip the bun oracle cache (cache::lookup
    /// returns None even on disk hit). Default false → cache enabled.
    /// Useful for benchmarking the runner itself or after a corpus /
    /// harness change that hasn't yet invalidated the hash key.
    pub no_cache: bool,
}

pub fn parse_args() -> Args {
    let mut limit: Option<usize> = None;
    let mut filter: Option<String> = None;
    let mut workers = DEFAULT_WORKERS;
    let mut report_bugs = DEFAULT_REPORT_BUGS;
    let mut json_out: Option<String> = None;
    let mut no_cache = false;
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
            "--no-cache" => no_cache = true,
            "-h" | "--help" => {
                eprintln!(
                    "torajs-test262 — run tc39/test262 against tr\n\n\
                     flags:\n  \
                     --limit N       only first N cases\n  \
                     --filter STR    cases whose path contains STR\n  \
                     --workers N     concurrency (default {DEFAULT_WORKERS})\n  \
                     --report-bugs N list first N bug failures (default {DEFAULT_REPORT_BUGS})\n  \
                     --json PATH     also write machine-readable summary to PATH\n  \
                     --no-cache      bypass the bun oracle cache for this run"
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
        no_cache,
    }
}
