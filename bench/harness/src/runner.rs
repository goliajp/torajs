use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Deserialize)]
pub struct Runner {
    pub name: String,
    /// Shell command to detect availability + capture version string.
    pub detect: String,
    /// Source filename inside each case directory, e.g. `main.ts`.
    pub src_filename: String,
    /// Optional compile command; templates `{src}` and `{out}` get substituted.
    pub compile: Option<String>,
    /// Run command; templates `{src}` and `{out}` get substituted.
    pub run: String,
}

impl Runner {
    /// Run the detect command. Returns Some(version_string) if exit==0, else None.
    /// `{workspace}` in the detect command is substituted before invocation.
    pub fn detect_version(&self, workspace: &Path) -> Option<String> {
        let cmd = self
            .detect
            .replace("{workspace}", &workspace.to_string_lossy());
        let parts = split_cmd(&cmd);
        if parts.is_empty() {
            return None;
        }
        let output = Command::new(&parts[0]).args(&parts[1..]).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        // some runtimes (e.g. older `go`) print version to stderr; prefer stdout, fall back to stderr
        let s = if stdout.trim().is_empty() {
            stderr
        } else {
            stdout
        };
        Some(s.lines().next().unwrap_or("").to_string())
    }
}

pub fn load_all(runners_dir: &Path) -> Result<Vec<Runner>> {
    if !runners_dir.exists() {
        anyhow::bail!("runners dir does not exist: {}", runners_dir.display());
    }
    let mut out = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(runners_dir)
        .with_context(|| format!("reading {}", runners_dir.display()))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let runner: Runner =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        out.push(runner);
    }
    Ok(out)
}

/// Whitespace split. Does NOT handle quoted args with spaces.
pub fn split_cmd(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

pub struct TemplateContext {
    pub src: String,
    pub out: String,
    pub case: String,
    pub workspace: String,
}

impl TemplateContext {
    pub fn substitute(&self, template: &str) -> String {
        template
            .replace("{src}", &self.src)
            .replace("{out}", &self.out)
            .replace("{case}", &self.case)
            .replace("{workspace}", &self.workspace)
    }
}
