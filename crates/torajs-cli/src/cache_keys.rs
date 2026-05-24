//! Cache-key derivation for the two on-disk caches `tr` maintains:
//! the per-fixture `.o` cache (B-1 phase 2) and the full `tr run`
//! AOT-output cache. Both use std `DefaultHasher` — collision-
//! resistance is good enough for cache use (worst case = false
//! miss + recompile, which is harmless).

use std::path::PathBuf;

/// Hash key for the per-fixture .o cache (B-1 phase 2). Includes
/// only inputs that affect the LLVM .o output:
/// - main source bytes
/// - import-closure file bytes
/// - opt level (hardcoded "O3" for `tr run`)
/// - `TORAJS_COMPILER_REV` — build.rs fingerprint of the compiler
///   `.rs` files (ssa_lower / ssa_inkwell / check / parser / lexer /
///   ast / modules / ssa). Substrate ships don't touch these → cache
///   stays warm across substrate ships, even though tr binary mtime
///   does change.
///
/// Notably absent: tr binary mtime, staticlib content. Both can
/// change without affecting .o bytes; including them would cause
/// false invalidations and defeat the entire point of this cache.
pub(crate) fn fixture_o_cache_key(src: &str, import_closure: &[(PathBuf, Vec<u8>)]) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut h);
    let mut sorted: Vec<&(PathBuf, Vec<u8>)> = import_closure.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, bytes) in &sorted {
        path.hash(&mut h);
        bytes.hash(&mut h);
    }
    "O3".hash(&mut h);
    // Compiler fingerprint from build.rs. Set in
    // crates/torajs-core/build.rs — but build.rs lives in torajs-core,
    // not torajs-cli, so we have to access it via torajs_core's env
    // export route. The build.rs sets `cargo:rustc-env=TORAJS_COMPILER_REV=...`
    // which means env! resolves it ONLY inside torajs-core itself.
    // To reach it from cli, expose via a `pub const` in torajs-core::lib.
    h.write(torajs_core::TORAJS_COMPILER_REV.as_bytes());
    "fixture-o-v1".hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Hash key for the run-cache. Includes the main source + every
/// imported file's path-relative bytes + the torajs CARGO_PKG_VERSION
/// + opt level. Multi-file: an edit to a transitively-imported lib
/// bumps the closure hash → cache slot misses → recompile.
///
/// Stable hashing: std DefaultHasher is FxHash-ish (collision-resistant
/// enough for cache use — worst case is a false miss / recompile, which
/// is harmless). The full-bytes-of-each-file approach is overkill for
/// a 4-file project but stays correct as the import graph grows.
pub(crate) fn run_cache_key(src: &str, import_closure: &[(PathBuf, Vec<u8>)]) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut h);
    // Sort the closure by path so the hash is order-independent (BFS
    // traversal order can vary if the lib graph is rearranged, but the
    // resulting program is the same).
    let mut sorted: Vec<&(PathBuf, Vec<u8>)> = import_closure.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, bytes) in &sorted {
        path.hash(&mut h);
        bytes.hash(&mut h);
    }
    env!("CARGO_PKG_VERSION").hash(&mut h);
    "O3".hash(&mut h);
    /* Hash the running tr binary's mtime so a freshly-rebuilt tr
     * binary doesn't hit cached binaries compiled by an earlier
     * (potentially buggy) version of itself. CARGO_PKG_VERSION stays
     * at "0.1.0" through 0.x dev so it doesn't differentiate; the
     * binary mtime does. Reading from the live binary path (not a
     * build-time stamp) avoids forcing a relink on every cargo run
     * — the cache key recomputes per-execution anyway.
     *
     * NOTE: this MUST stay in the cache key during substrate-port
     * phases. A cached binary linked against the OLD runtime keeps
     * working correctly (it has its own embedded code copy), but
     * running it does NOT exercise the NEW Rust port — masking any
     * regression the port might have introduced. The conformance
     * gate's value depends on actually compiling + running the new
     * code path on every ship. Tracked in conformance perf backlog;
     * see runner --no-aot flag for a less aggressive perf knob. */
    if let Ok(exe) = std::env::current_exe()
        && let Ok(meta) = std::fs::metadata(&exe)
        && let Ok(mtime) = meta.modified()
        && let Ok(d) = mtime.duration_since(std::time::UNIX_EPOCH)
    {
        d.as_secs().hash(&mut h);
    }
    format!("torajs-{:016x}", h.finish())
}
