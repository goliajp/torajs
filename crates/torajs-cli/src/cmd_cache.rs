//! `tr cache [size | clean [--max-mb N]]` — inspect / prune the
//! per-host ~/.torajs/cache used by `tr run` AOT memoization.
//!
//! Also exposes [`prune_run_cache`] for the `tr run` cache-miss
//! path to call before-compile (eviction amortizes across the
//! upcoming LLVM pass).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::util::{dir_size_bytes, fmt_mb};

/// LRU prune for the run cache. Keeps the directory size at ~2 GB
/// by deleting oldest entries (by modify time) until under the cap.
/// Runs only on cache-miss + before-compile (when the upcoming LLVM
/// pass dwarfs the prune cost). Each cache entry is a single
/// executable plus an optional `.dSYM/` debug-info bundle on macOS;
/// both get cleaned together.
///
/// Cap can be overridden via `TORAJS_CACHE_MAX_MB` (e.g. for CI
/// boxes with tighter disk).
pub(crate) fn prune_run_cache(cache_dir: &Path) {
    const DEFAULT_CAP_MB: u64 = 2 * 1024; // 2 GB
    let cap_bytes = std::env::var("TORAJS_CACHE_MAX_MB")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_CAP_MB)
        * 1024
        * 1024;

    // Collect (mtime, path, size) for every direct child. We don't
    // recurse into .dSYM internals — they're paired with their
    // sibling binary by stem matching at delete time.
    let mut entries: Vec<(std::time::SystemTime, PathBuf, u64)> = Vec::new();
    let read = match std::fs::read_dir(cache_dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    let mut total: u64 = 0;
    for ent in read.flatten() {
        let path = ent.path();
        let meta = match ent.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let size = if meta.is_dir() {
            dir_size_bytes(&path)
        } else {
            meta.len()
        };
        total += size;
        let mtime = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        entries.push((mtime, path, size));
    }
    if total <= cap_bytes {
        return;
    }
    // Sort oldest-first; delete until under cap.
    entries.sort_by_key(|(m, _, _)| *m);
    let mut freed: u64 = 0;
    let need_free = total - cap_bytes;
    for (_, path, size) in entries {
        if freed >= need_free {
            break;
        }
        if path.is_file() {
            let _ = std::fs::remove_file(&path);
            // Pair-delete the matching .dSYM bundle if present.
            let dsym = path.with_extension("dSYM");
            if dsym.exists() {
                let _ = std::fs::remove_dir_all(&dsym);
            }
        } else if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        }
        freed += size;
    }
}

/// `tr cache clean [--max-mb N]` — prune `~/.torajs/cache` to under
/// the given cap (default 2048 MB). LRU eviction by mtime ascending.
/// Pairs deleted with their `.dSYM/` bundles. Reports bytes freed.
///
/// Idempotent + safe to call concurrently with `tr run` invocations
/// (worst case: a concurrent run hits a freshly evicted cache slot
/// and recompiles — same outcome as a cold cache).
pub(crate) fn run_cache_subcmd(args: &[String]) -> ExitCode {
    let sub = args.first().map(String::as_str);
    match sub {
        Some("clean") => {
            let mut cap_mb: u64 = 2048;
            let mut i = 1;
            while i < args.len() {
                if args[i] == "--max-mb" {
                    if let Some(v) = args.get(i + 1).and_then(|s| s.parse::<u64>().ok()) {
                        cap_mb = v;
                        i += 2;
                        continue;
                    }
                    eprintln!("error: --max-mb expects a positive integer");
                    return ExitCode::from(2);
                }
                i += 1;
            }
            let cache_dir = match std::env::var_os("TORAJS_CACHE_DIR")
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".torajs/cache"))
                }) {
                Some(d) => d,
                None => {
                    eprintln!("error: no $HOME and no $TORAJS_CACHE_DIR set");
                    return ExitCode::from(1);
                }
            };
            if !cache_dir.is_dir() {
                println!("cache dir does not exist: {}", cache_dir.display());
                return ExitCode::SUCCESS;
            }
            let before_bytes = dir_size_bytes(&cache_dir);
            // Temporarily override env to push the cap down to cap_mb
            // for the duration of this call; existing prune_run_cache
            // reads TORAJS_CACHE_MAX_MB.
            // SAFETY: single-threaded subcommand entry point.
            unsafe {
                std::env::set_var("TORAJS_CACHE_MAX_MB", cap_mb.to_string());
            }
            prune_run_cache(&cache_dir);
            let after_bytes = dir_size_bytes(&cache_dir);
            let freed = before_bytes.saturating_sub(after_bytes);
            println!(
                "cache: {} → {} ({:+} MB)",
                fmt_mb(before_bytes),
                fmt_mb(after_bytes),
                -(freed as i64 / 1024 / 1024)
            );
            ExitCode::SUCCESS
        }
        Some("size") | None => {
            let cache_dir = std::env::var_os("TORAJS_CACHE_DIR")
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".torajs/cache"))
                });
            if let Some(d) = cache_dir
                && d.is_dir()
            {
                println!("{}: {}", d.display(), fmt_mb(dir_size_bytes(&d)));
            } else {
                println!("cache dir does not exist");
            }
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("error: unknown `tr cache` subcommand `{other}`");
            eprintln!("usage: tr cache [size | clean [--max-mb N]]");
            ExitCode::from(2)
        }
    }
}
