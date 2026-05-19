use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::runner::Runner;

#[derive(Debug, Clone, Deserialize)]
pub struct VersionFloor {
    pub floor: String,
    pub hint: String,
}

#[derive(Debug, Deserialize)]
pub struct LatestStableConfig {
    pub runners: HashMap<String, VersionFloor>,
}

pub fn load_floors(path: &Path) -> Result<LatestStableConfig> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let config: LatestStableConfig =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(config)
}

/// Extract the first `<num>.<num>.<num>` triple from a version string.
/// Tolerant of arbitrary prefixes/suffixes — works for `v26.1.0` (node),
/// `1.3.14` (bun), `go version go1.25.6 darwin/arm64`, `rustc 1.95.0 (…)`.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let bytes = s.as_bytes();
    let n = bytes.len();
    let is_digit = |b: u8| b.is_ascii_digit();
    let mut i = 0;
    while i < n {
        if !is_digit(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        let mut nums = [0u32; 3];
        let mut nidx = 0;
        let mut cur: u32 = 0;
        let mut cur_started = false;
        let mut j = i;
        while j < n {
            if is_digit(bytes[j]) {
                cur = cur.checked_mul(10)?.checked_add((bytes[j] - b'0') as u32)?;
                cur_started = true;
                j += 1;
            } else if bytes[j] == b'.' && cur_started && nidx < 2 {
                nums[nidx] = cur;
                nidx += 1;
                cur = 0;
                cur_started = false;
                j += 1;
            } else {
                break;
            }
        }
        if cur_started && nidx == 2 {
            nums[2] = cur;
            return Some((nums[0], nums[1], nums[2]));
        }
        i = j.max(start + 1);
    }
    None
}

/// True iff `detected >= floor` by semver tuple ordering. Returns None when
/// either side cannot be parsed (caller decides whether that's fatal).
pub fn version_satisfies(detected: &str, floor: &str) -> Option<bool> {
    let d = parse_version(detected)?;
    let f = parse_version(floor)?;
    Some(d >= f)
}

/// Resolve the absolute path of the binary the detect command will execute,
/// using the same PATH-search semantics Rust's `Command::new(name)` uses.
/// This catches PATH drift: e.g. `nvm use node` activates `~/.nvm/.../v26.1.0`
/// in the shell, but cargo-launched subprocesses may still pick up a stale
/// `/opt/homebrew/bin/node` shim — bench would silently measure the wrong
/// toolchain. Returns None for in-tree runners (path starts with `{` template
/// or absolute) and on resolution failure.
fn resolve_binary_path(detect_cmd: &str) -> Option<String> {
    let first = detect_cmd.split_whitespace().next()?;
    if first.starts_with('{') || first.starts_with('/') {
        return None; // templated or absolute — nothing to resolve
    }
    let output = Command::new("/usr/bin/which").arg(first).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// Run pre-bench verification: every runner with a floor entry must detect
/// a version and that version must be ≥ floor. On any failure, collect ALL
/// drifts before bailing so the user sees the whole picture in one go.
pub fn verify_versions(
    runners: &[Runner],
    floors: &LatestStableConfig,
    workspace: &Path,
) -> Result<()> {
    let mut errors: Vec<String> = Vec::new();
    let mut summary: Vec<String> = Vec::new();
    for runner in runners {
        let Some(floor_cfg) = floors.runners.get(&runner.name) else {
            // In-tree runner or unlisted; nothing to verify.
            continue;
        };
        let detected = match runner.detect_version(workspace) {
            Some(v) => v,
            None => {
                errors.push(format!(
                    "{}: not detected (command `{}` failed)\n  fix: {}",
                    runner.name, runner.detect, floor_cfg.hint
                ));
                continue;
            }
        };
        let detected_trim = detected.trim();
        let bin_path =
            resolve_binary_path(&runner.detect).unwrap_or_else(|| "<unresolved>".to_string());
        match version_satisfies(detected_trim, &floor_cfg.floor) {
            Some(true) => {
                summary.push(format!(
                    "  {:<10} {}  (floor {})  [{}]",
                    runner.name, detected_trim, floor_cfg.floor, bin_path
                ));
            }
            Some(false) => {
                errors.push(format!(
                    "{}: detected `{}` < floor `{}`\n  fix: {}",
                    runner.name, detected_trim, floor_cfg.floor, floor_cfg.hint
                ));
            }
            None => {
                errors.push(format!(
                    "{}: could not parse semver from detect output `{}`",
                    runner.name, detected_trim
                ));
            }
        }
    }
    if !errors.is_empty() {
        anyhow::bail!(
            "bench latest-stable verify failed ({} drift{}):\n{}\n\n\
             Policy lives in bench/latest-stable.toml — bump the floor there \
             (with reason in the commit message) once you've updated the toolchain.",
            errors.len(),
            if errors.len() == 1 { "" } else { "s" },
            errors.join("\n")
        );
    }
    if !summary.is_empty() {
        eprintln!(
            "→ B-BENCH-VER: latest-stable verify OK ({} runner{}):",
            summary.len(),
            if summary.len() == 1 { "" } else { "s" }
        );
        for line in &summary {
            eprintln!("{line}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_node_style() {
        assert_eq!(parse_version("v26.1.0"), Some((26, 1, 0)));
    }

    #[test]
    fn parse_bare_semver() {
        assert_eq!(parse_version("1.3.14"), Some((1, 3, 14)));
    }

    #[test]
    fn parse_go_style() {
        assert_eq!(
            parse_version("go version go1.25.6 darwin/arm64"),
            Some((1, 25, 6))
        );
    }

    #[test]
    fn parse_rustc_style() {
        assert_eq!(
            parse_version("rustc 1.95.0 (59807616e 2026-04-14)"),
            Some((1, 95, 0))
        );
    }

    #[test]
    fn parse_python_style() {
        assert_eq!(parse_version("Python 3.13.0"), Some((3, 13, 0)));
    }

    #[test]
    fn parse_no_triple() {
        assert_eq!(parse_version("no version here"), None);
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("42"), None);
    }

    #[test]
    fn satisfies_basic() {
        assert_eq!(version_satisfies("v26.1.0", "26.0.0"), Some(true));
        assert_eq!(version_satisfies("v25.9.0", "26.0.0"), Some(false));
        assert_eq!(version_satisfies("1.3.14", "1.3.0"), Some(true));
        assert_eq!(version_satisfies("1.3.14", "1.3.14"), Some(true));
        assert_eq!(version_satisfies("1.3.13", "1.3.14"), Some(false));
    }

    #[test]
    fn satisfies_unparseable() {
        assert_eq!(version_satisfies("garbage", "1.0.0"), None);
        assert_eq!(version_satisfies("1.0.0", "garbage"), None);
    }

    #[test]
    fn satisfies_skips_leading_commit_hash() {
        // `rustc 1.95.0 (59807616e 2026-04-14)` — first N.N.N is the version,
        // commit hash `59807616` would be parsed only if our scanner kept going,
        // but we return the first match.
        assert_eq!(
            version_satisfies("rustc 1.95.0 (59807616e 2026-04-14)", "1.95.0"),
            Some(true)
        );
    }
}
