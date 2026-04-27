use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_RUN_WARMUP: u32 = 3;
const DEFAULT_RUN_RUNS: u32 = 10;
const DEFAULT_COMPILE_WARMUP: u32 = 1;
const DEFAULT_COMPILE_RUNS: u32 = 5;

#[derive(Debug, Clone)]
pub struct Case {
    pub name: String,
    pub dir: PathBuf,
    pub expected_stdout: String,
    pub run_warmup: u32,
    pub run_runs: u32,
    pub compile_warmup: u32,
    pub compile_runs: u32,
    /// 0 = byte-exact stdout match. Anything else activates tolerance mode:
    /// the last non-empty line of stdout is parsed as an integer and compared
    /// against the matching line of `expected.txt`; the run passes if the
    /// difference is within `tolerance`. Earlier lines, if any, still need
    /// to match byte-for-byte. This exists for FP-heavy cases like
    /// mandelbrot, where compiler FMA choices produce different bit-exact
    /// answers on the same algorithm.
    pub tolerance: u64,
    /// Optional override for the `torajs-aot` clang invocation. When set,
    /// the harness exports `TORAJS_AOT_CLANG_FLAGS=<value>` for the compile
    /// step and `bench/aot-host/build.sh` reads it. Empirically `-O1` beats
    /// `-O3` on some shapes (fib40, startup) while `-O3` beats `-O1` on
    /// others (popcount needs LLVM's loop-idiom recognition for
    /// `cnt.16b`). Default `-O3` works for the median case.
    pub aot_clang_flags: Option<String>,
}

/// Optional per-case `bench.toml` overrides.
///
/// ```toml
/// runs = 3                   # hyperfine --runs for the run command
/// warmup = 1                 # hyperfine --warmup for the run command
/// compile_runs = 5
/// compile_warmup = 1
/// tolerance = 500            # absolute int diff allowed on stdout's last line
/// aot_clang_flags = "-O1"    # override clang flags for torajs-aot compile
/// ```
#[derive(Debug, Default, Deserialize)]
struct CaseConfig {
    runs: Option<u32>,
    warmup: Option<u32>,
    compile_runs: Option<u32>,
    compile_warmup: Option<u32>,
    tolerance: Option<u64>,
    aot_clang_flags: Option<String>,
}

pub fn discover_all(cases_dir: &Path) -> Result<Vec<Case>> {
    if !cases_dir.exists() {
        anyhow::bail!("cases dir does not exist: {}", cases_dir.display());
    }
    let mut out = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(cases_dir)
        .with_context(|| format!("reading {}", cases_dir.display()))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .map(String::from)
            .context("case dir name not utf-8")?;
        let expected_path = path.join("expected.txt");
        if !expected_path.exists() {
            anyhow::bail!(
                "case `{}` is missing expected.txt at {}",
                name,
                expected_path.display()
            );
        }
        let expected_stdout = fs::read_to_string(&expected_path)
            .with_context(|| format!("reading {}", expected_path.display()))?;

        let config_path = path.join("bench.toml");
        let cfg: CaseConfig = if config_path.exists() {
            let text = fs::read_to_string(&config_path)
                .with_context(|| format!("reading {}", config_path.display()))?;
            toml::from_str(&text).with_context(|| format!("parsing {}", config_path.display()))?
        } else {
            CaseConfig::default()
        };

        out.push(Case {
            name,
            dir: path,
            expected_stdout,
            run_warmup: cfg.warmup.unwrap_or(DEFAULT_RUN_WARMUP),
            run_runs: cfg.runs.unwrap_or(DEFAULT_RUN_RUNS),
            compile_warmup: cfg.compile_warmup.unwrap_or(DEFAULT_COMPILE_WARMUP),
            compile_runs: cfg.compile_runs.unwrap_or(DEFAULT_COMPILE_RUNS),
            tolerance: cfg.tolerance.unwrap_or(0),
            aot_clang_flags: cfg.aot_clang_flags,
        });
    }
    Ok(out)
}

/// Compare actual stdout against `case.expected_stdout`. In exact mode
/// (`tolerance == 0`) requires byte-for-byte equality. In tolerance mode,
/// every line except the last must still be byte-equal; the last non-empty
/// line is parsed as an integer in both and compared with `|a - b| <= tol`.
pub fn stdout_matches(case: &Case, actual: &str) -> bool {
    if case.tolerance == 0 {
        return actual == case.expected_stdout;
    }
    // Strip exactly one trailing newline if present, so "x\n" → "x".
    let actual = actual.strip_suffix('\n').unwrap_or(actual);
    let expected = case
        .expected_stdout
        .strip_suffix('\n')
        .unwrap_or(&case.expected_stdout);
    let mut a_lines: Vec<&str> = actual.split('\n').collect();
    let mut e_lines: Vec<&str> = expected.split('\n').collect();
    if a_lines.len() != e_lines.len() {
        return false;
    }
    let a_last = a_lines.pop().unwrap_or("");
    let e_last = e_lines.pop().unwrap_or("");
    if a_lines != e_lines {
        return false;
    }
    let (Ok(a_n), Ok(e_n)) = (a_last.trim().parse::<i64>(), e_last.trim().parse::<i64>()) else {
        return false;
    };
    a_n.abs_diff(e_n) <= case.tolerance
}
