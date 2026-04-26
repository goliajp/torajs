use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Case {
    pub name: String,
    pub dir: PathBuf,
    pub expected_stdout: String,
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
        out.push(Case {
            name,
            dir: path,
            expected_stdout,
        });
    }
    Ok(out)
}
