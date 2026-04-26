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
}

/// Optional per-case `bench.toml` overrides.
///
/// ```toml
/// runs = 3              # hyperfine --runs for the run command
/// warmup = 1            # hyperfine --warmup for the run command
/// compile_runs = 5
/// compile_warmup = 1
/// ```
#[derive(Debug, Default, Deserialize)]
struct CaseConfig {
    runs: Option<u32>,
    warmup: Option<u32>,
    compile_runs: Option<u32>,
    compile_warmup: Option<u32>,
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
        });
    }
    Ok(out)
}
