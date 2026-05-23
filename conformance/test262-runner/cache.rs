//! Bun oracle cache for the test262 runner.
//!
//! ## Why
//!
//! Each test262 case requires running `bun` first to establish the
//! oracle (expected stdout + spec-conformance verdict). `bun` is
//! ~50-80 ms per spawn × 53,174 cases / 8 workers ≈ 5-8 min per
//! full run on M-series mini. The result is **fully deterministic**
//! for a given (case_bytes, harness_bytes, bun_version) — test262
//! corpus is a frozen-at-vendor-pin snapshot, harness is a single
//! file, bun_version is detected once. Cache hit → skip bun spawn
//! entirely → 0-cost oracle on subsequent runs.
//!
//! ## Layout
//!
//! `~/.torajs/test262-cache/<u64_hex_hash>.cache` per entry. Binary:
//! ```text
//! [success: u8 (1=pass, 0=skip)]
//! [case_len: u32 LE]              ← collision defense: verified on read
//! [harness_len: u32 LE]
//! [stdout_len: u32 LE]
//! [stdout: stdout_len bytes]
//! ```
//!
//! Cache is **invalidation-on-mismatch**: any of (case bytes / harness
//! bytes / bun version) changing flips the hash key, so old entries
//! become orphan. They sit on disk; periodic external prune cleans
//! (LRU based on atime if needed). For now the corpus is stable so
//! the cache grows once + stays warm.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

/// Cached bun oracle outcome for a single test262 case.
pub struct CachedBun {
    pub success: bool,
    pub stdout: Vec<u8>,
}

/// Detected `bun --version` string. Read once at runner startup
/// (`init_version()`); used as a cache-key salt so a bun upgrade
/// auto-invalidates without manual prune.
static BUN_VERSION: OnceLock<String> = OnceLock::new();

/// Whether the cache is enabled for this runner invocation. Default
/// on; `--no-cache` flag flips it off.
static ENABLED: OnceLock<bool> = OnceLock::new();

pub fn set_enabled(enabled: bool) {
    let _ = ENABLED.set(enabled);
}

pub fn enabled() -> bool {
    *ENABLED.get().unwrap_or(&true)
}

/// Resolve the cache directory. `$HOME/.torajs/test262-cache/`.
/// Creates it on first call (best-effort; failures fall through to
/// per-op fs errors which are treated as cache miss).
pub fn cache_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".torajs").join("test262-cache")
}

/// One-time init: detect bun version + create cache dir. Call once
/// from runner main before workers start.
pub fn init() {
    let _ = BUN_VERSION.set(detect_bun_version());
    let _ = std::fs::create_dir_all(cache_dir());
}

fn detect_bun_version() -> String {
    match Command::new("bun").arg("--version").output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "unknown".to_string(),
    }
}

fn bun_version() -> &'static str {
    BUN_VERSION.get().map(String::as_str).unwrap_or("unknown")
}

fn compute_key(case_bytes: &[u8], harness_bytes: &[u8]) -> String {
    let mut h = DefaultHasher::new();
    case_bytes.hash(&mut h);
    b'\xff'.hash(&mut h); // unambiguous separator
    harness_bytes.hash(&mut h);
    b'\xff'.hash(&mut h);
    bun_version().as_bytes().hash(&mut h);
    format!("{:016x}", h.finish())
}

fn entry_path(key: &str) -> PathBuf {
    cache_dir().join(format!("{key}.cache"))
}

/// Look up a cached bun outcome. Returns `None` on cache miss / read
/// failure / collision-defense rejection. Collision defense: the
/// stored entry carries the case_bytes + harness_bytes lengths; if
/// they mismatch the caller's lengths we treat the entry as a hash
/// collision and return None (caller re-runs bun, overwrites entry).
pub fn lookup(case_bytes: &[u8], harness_bytes: &[u8]) -> Option<CachedBun> {
    if !enabled() {
        return None;
    }
    let key = compute_key(case_bytes, harness_bytes);
    let path = entry_path(&key);
    let bytes = std::fs::read(&path).ok()?;
    if bytes.len() < 13 {
        return None;
    }
    let success = bytes[0] != 0;
    let cl = u32::from_le_bytes(bytes[1..5].try_into().ok()?) as usize;
    let hl = u32::from_le_bytes(bytes[5..9].try_into().ok()?) as usize;
    let sl = u32::from_le_bytes(bytes[9..13].try_into().ok()?) as usize;
    if bytes.len() != 13 + sl {
        return None; // corrupt entry
    }
    // Collision defense — DefaultHasher u64 has ~2^-64 collision
    // probability per pair at 53k entries (≈ 10^-10 over the full
    // corpus). Length match is a cheap extra sanity check; on
    // mismatch we just re-run bun.
    if cl != case_bytes.len() || hl != harness_bytes.len() {
        return None;
    }
    let stdout = bytes[13..].to_vec();
    Some(CachedBun { success, stdout })
}

/// Write a cache entry. Best-effort: errors silently swallowed (a
/// failed write just means the next run re-spawns bun, no
/// correctness impact).
pub fn insert(case_bytes: &[u8], harness_bytes: &[u8], success: bool, stdout: &[u8]) {
    if !enabled() {
        return;
    }
    let key = compute_key(case_bytes, harness_bytes);
    let path = entry_path(&key);
    let mut buf = Vec::with_capacity(13 + stdout.len());
    buf.push(if success { 1 } else { 0 });
    buf.extend_from_slice(&(case_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(&(harness_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(&(stdout.len() as u32).to_le_bytes());
    buf.extend_from_slice(stdout);
    let _ = std::fs::write(&path, &buf);
}

/// Bun oracle path — cache lookup first, spawn bun on miss with a
/// per-task timeout (defensive vs hang cases; tr side already has its
/// own 30s timeout — bun was previously unbounded). Returns
/// `(success, stdout)`. Errors from bun spawn surface as `Err`; bun
/// timeout returns `Ok((false, ...))` so it classifies as `BunSkip`
/// (consistent with the way bun's own non-zero exits classify).
///
/// Caller responsibility:
/// - Caller has written `tmp_path` containing harness + transformed
///   source before calling this fn.
/// - On cache hit the tmp_path file is still read by tr later, so do
///   NOT remove it before tr runs.
pub fn bun_oracle(
    case_bytes: &[u8],
    harness_bytes: &[u8],
    tmp_path: &std::path::Path,
) -> Result<(bool, Vec<u8>), String> {
    if let Some(c) = lookup(case_bytes, harness_bytes) {
        return Ok((c.success, c.stdout));
    }
    let mut child = Command::new("bun")
        .args(["run", &tmp_path.to_string_lossy()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("bun spawn: {e}"))?;
    // 15s bun timeout — bun usually 50-80 ms per case; anything over
    // 15s is almost certainly a hang, not a slow-but-correct run.
    // Matches feedback_bg_hang_detection — "single hang fixture must
    // not drag the whole batch down". 30s tr timeout already exists
    // downstream; 15s here keeps total per-case <= 45s ceiling.
    let timeout = std::time::Duration::from_secs(15);
    let started = std::time::Instant::now();
    let out = loop {
        match child.try_wait() {
            Ok(Some(_)) => break child.wait_with_output(),
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    // Treat as BunSkip — same outcome as bun exiting
                    // non-zero. Don't cache hangs (re-test next run
                    // in case bun improved or environment changed).
                    return Ok((false, Vec::new()));
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => return Err(format!("bun wait: {e}")),
        }
    };
    let out = out.map_err(|e| format!("bun output: {e}"))?;
    let success = out.status.success();
    insert(case_bytes, harness_bytes, success, &out.stdout);
    Ok((success, out.stdout))
}

/// Stats helper for the run summary. Returns (entry_count,
/// total_bytes_on_disk). Best-effort; ignores I/O errors.
pub fn stats() -> (usize, u64) {
    let dir = cache_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return (0, 0);
    };
    let mut n = 0usize;
    let mut bytes = 0u64;
    for e in entries.flatten() {
        if let Ok(meta) = e.metadata() {
            if meta.is_file() {
                n += 1;
                bytes += meta.len();
            }
        }
    }
    (n, bytes)
}
