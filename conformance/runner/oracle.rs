//! The bun oracle and its on-disk cache.
//!
//! Split out of `main.rs` under the 500-line file rule when the cache
//! key learned about bun versions. It belongs apart anyway: this is
//! the gate's measurement apparatus, and the one thing it must never
//! do is answer with something other than what the bun on this
//! machine says today.

use crate::exec::exec;
use std::path::{Path, PathBuf};

/// Return bun's stdout for `src`. On cache hit (content-hash match)
/// reads the cached bytes directly; on miss runs `bun run` once and
/// writes the result into
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
pub(crate) fn get_or_fill_bun_oracle(src: &Path) -> Result<String, String> {
    let bytes = std::fs::read(src).map_err(|e| format!("read {}: {e}", src.display()))?;
    let hash = content_hash(&bytes);
    // No readable version means no trustworthy key, so there is no
    // cache — every case pays the bun spawn rather than risk being
    // served an answer from an unknown oracle.
    let Some(cache_dir) = bun_oracle_cache_dir() else {
        let (out, _) = exec("bun", &["run", src.to_str().unwrap()])?;
        return Ok(out);
    };
    let cache_path = cache_dir.join(format!("{hash}.out"));
    if let Ok(s) = std::fs::read_to_string(&cache_path) {
        return Ok(s);
    }
    let (out, _) = exec("bun", &["run", src.to_str().unwrap()])?;
    let _ = std::fs::create_dir_all(&cache_dir);
    let _ = std::fs::write(&cache_path, &out);
    Ok(out)
}

fn content_hash(bytes: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    // Convention tag — bump if the cached bytes stop meaning "bun's
    // stdout for this source". The oracle's own identity lives in
    // the directory name, not here.
    "oracle-v1".hash(&mut h);
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
