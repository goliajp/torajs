//! The bun oracle and its on-disk cache.
//!
//! Split out of `main.rs` under the 500-line file rule when the cache
//! key learned about bun versions. It belongs apart anyway: this is
//! the gate's measurement apparatus, and the one thing it must never
//! do is answer with something other than what the bun on this
//! machine says today.

use crate::exec::{exec, exec_code};
use std::path::{Path, PathBuf};

/// Return bun's stdout for `src`. On cache hit (a hash over every
/// input file, [`oracle_inputs`]) reads the cached bytes directly; on
/// miss runs `bun run` once and writes the result into
/// `conformance/.oracle-cache/<bun version>/<hash>.out`.
///
/// Why this helps: each `bun run` is ~10-20 ms wall time even for
/// trivial fixtures; 3200 cases × 15 ms ≈ 48 s of cumulative bun
/// startup that is pure dead-weight once the oracle output for a
/// given source byte-sequence is known. False misses are harmless
/// (re-runs bun); content-equal sources are served from disk.
///
/// The cache is keyed by BUN VERSION as well as by content, and that
/// is not a nicety. An oracle cache whose key omits the oracle is a
/// measurement apparatus that fails silently and looks exactly like
/// data: after bun 1.3.14 → 1.4.0 the gate went on comparing against
/// answers a retired bun had given, so `Date.prototype` grew
/// `toTemporalInstant` in the real oracle while the gate kept
/// reporting `ok` for a fixture that prints every own name on it.
/// Nothing about that run looked wrong — the fixture passed, the
/// summary was green, and the only way to see it was to run bun by
/// hand. Putting the version in the path also makes the staleness
/// legible on disk instead of buried in a hash: `ls` the directory
/// and you can see which bun each answer came from, and drop the
/// retired ones.
///
/// bun must end with the status the case declares (`expected_exit`);
/// an answer it gave while exiting otherwise is not an oracle.
pub(crate) fn get_or_fill_bun_oracle(src: &Path, expected_exit: i32) -> Result<String, String> {
    let hash = content_hash(&oracle_inputs(src)?);
    // No readable version means no trustworthy key, so there is no
    // cache — every case pays the bun spawn rather than risk being
    // served an answer from an unknown oracle.
    let Some(cache_dir) = bun_oracle_cache_dir() else {
        return bun_run(src, expected_exit);
    };
    let cache_path = cache_dir.join(format!("{hash}.out"));
    if let Ok(s) = std::fs::read_to_string(&cache_path) {
        return Ok(s);
    }
    let out = bun_run(src, expected_exit)?;
    let _ = std::fs::create_dir_all(&cache_dir);
    let _ = std::fs::write(&cache_path, &out);
    Ok(out)
}

fn bun_run(src: &Path, expected_exit: i32) -> Result<String, String> {
    let (out, err, code) = exec_code("bun", &["run", src.to_str().unwrap()])?;
    if code != expected_exit {
        return Err(format!(
            "bun exited {code}, the case declares {expected_exit}: {}",
            err.trim()
        ));
    }
    Ok(out)
}

/// Every file bun reads to answer `src`, as (path, bytes) in a fixed
/// order: the entry first, then what it can import. A key over the
/// entry alone served a stale answer whenever a case changed only a
/// file its entry imports — the gate then compared tr against what bun
/// said about the OLD sibling. A directory case (`<dir>/main.ts`)
/// takes its whole directory; a single-file case takes the files its
/// relative specifiers name, transitively. Over-including only costs a
/// cache miss; missing an input is the failure this guards against.
/// The entry's path is part of the key too: bun's messages can name it.
fn oracle_inputs(src: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let read = |p: &Path| std::fs::read(p).map_err(|e| format!("read {}: {e}", p.display()));
    let mut out = vec![(src.to_path_buf(), read(src)?)];
    if src.file_name().is_some_and(|n| n == "main.ts")
        && let Some(dir) = src.parent()
    {
        let mut files = Vec::new();
        list_files(dir, &mut files).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
        files.sort();
        for f in files.into_iter().filter(|f| f != src) {
            let bytes = read(&f)?;
            out.push((f, bytes));
        }
        return Ok(out);
    }
    let mut seen = vec![src.to_path_buf()];
    let mut i = 0;
    while i < out.len() {
        let base = out[i].0.parent().unwrap_or(Path::new(".")).to_path_buf();
        for spec in relative_specifiers(&out[i].1) {
            let Some(dep) = resolve_relative(&base, &spec) else {
                continue;
            };
            if !seen.contains(&dep) {
                seen.push(dep.clone());
                let bytes = read(&dep)?;
                out.push((dep, bytes));
            }
        }
        i += 1;
    }
    Ok(out)
}

fn list_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            list_files(&p, out)?;
        } else {
            out.push(p);
        }
    }
    Ok(())
}

/// Every quoted string in `src` that starts `./` or `../` — a superset
/// of the relative module specifiers, which is the safe direction.
fn relative_specifiers(src: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < src.len() {
        let q = src[i];
        if (q == b'"' || q == b'\'' || q == b'`')
            && (src[i + 1..].starts_with(b"./") || src[i + 1..].starts_with(b"../"))
            && let Some(len) = src[i + 1..].iter().position(|&c| c == q || c == b'\n')
            && src[i + 1 + len] == q
        {
            out.push(String::from_utf8_lossy(&src[i + 1..i + 1 + len]).into_owned());
            i += len + 2;
            continue;
        }
        i += 1;
    }
    out
}

fn resolve_relative(base: &Path, spec: &str) -> Option<PathBuf> {
    let p = base.join(spec);
    if p.is_file() {
        return Some(p);
    }
    ["ts", "cts", "mts", "js"]
        .iter()
        .map(|ext| base.join(format!("{spec}.{ext}")))
        .find(|p| p.is_file())
}

fn content_hash(inputs: &[(PathBuf, Vec<u8>)]) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (path, bytes) in inputs {
        path.hash(&mut h);
        bytes.hash(&mut h);
    }
    // Convention tag — bump if the cached bytes stop meaning "bun's
    // stdout for these inputs". The oracle's own identity lives in
    // the directory name, not here.
    "oracle-v2".hash(&mut h);
    format!("{:016x}", h.finish())
}

/// `conformance/.oracle-cache/<bun version>`, or `None` when
/// `bun --version` cannot be read.
fn bun_oracle_cache_dir() -> Option<PathBuf> {
    static VERSION: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let version = VERSION
        .get_or_init(|| {
            let (out, _) = exec("bun", &["--version"]).ok()?;
            let v = out.trim();
            // A version is a path component here, so refuse anything
            // that is not one rather than writing somewhere strange.
            if v.is_empty() || v.contains('/') || v.contains(std::path::MAIN_SEPARATOR) {
                return None;
            }
            Some(format!("bun-{v}"))
        })
        .as_deref()?;
    Some(repo_root_oracle_cache_dir().join(version))
}

fn repo_root_oracle_cache_dir() -> PathBuf {
    std::env::current_dir()
        .map(|p| p.join("conformance/.oracle-cache"))
        .unwrap_or_else(|_| PathBuf::from("conformance/.oracle-cache"))
}

#[cfg(test)]
mod tests {
    use super::relative_specifiers;

    #[test]
    fn finds_relative_specifiers_in_every_quote_style() {
        let src = br#"import { a } from "./lib.ts";
export * from '../up/m';
const m = await import(`./dyn.js`);
const s = "not/relative"; const t = "./unterminated
"#;
        assert_eq!(
            relative_specifiers(src),
            vec!["./lib.ts", "../up/m", "./dyn.js"]
        );
    }

    #[test]
    fn ignores_a_quote_at_the_end_of_the_source() {
        assert!(relative_specifiers(b"x = \"").is_empty());
        assert!(relative_specifiers(b"'./").is_empty());
    }
}
