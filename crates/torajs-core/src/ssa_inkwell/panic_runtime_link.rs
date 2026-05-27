//! Wire helpers for v0.7 Step 9b — custom torajs-panic-runtime
//! staticlib linked via `-Wl,-force_load`.
//!
//! Two responsibilities, kept out of `link.rs` to honour the
//! `.claude/rules/common/file-size.md` 500-line cap:
//!
//! 1. **Pick the staticlib path**: locate `libtorajs_panic_runtime-*.a`
//!    in the temp-extracted staticlib paths so `link.rs` can emit the
//!    `-Wl,-force_load,<path>` arg.
//!
//! 2. **Filter intentional ld warnings**: `-Wl,-force_load` makes ld
//!    emit "duplicate symbol" warnings for `rust_begin_unwind` and
//!    `__rust_alloc_error_handler` because every sibling Layer-1+
//!    staticlib bundles `libstd.rlib(panicking.o + alloc.o)` providing
//!    the same `__rustc`-namespaced mangled symbols. ld correctly picks
//!    ours — the warnings are deliberate. We strip exactly those two
//!    warning blocks from ld's stderr and pass everything else through
//!    unchanged so genuine link issues still reach the user.

use std::path::{Path, PathBuf};

/// Symbols our panic_runtime override deliberately collides with —
/// matches the mangled forms in `libstd-XXX.rlib`'s panicking + alloc
/// objects.
const INTENTIONAL_DUP_SYMBOLS: &[&str] = &[
    "__RNvCs4ho9zzRceuO_7___rustc17rust_begin_unwind",
    "__RNvCs4ho9zzRceuO_7___rustc26___rust_alloc_error_handler",
];

/// Locate `libtorajs_panic_runtime-*.a` in the temp-extracted staticlib
/// paths. Returns `Some(path)` when found, `None` otherwise.
pub(super) fn pick_panic_runtime_archive(paths: &[PathBuf]) -> Option<PathBuf> {
    paths
        .iter()
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("libtorajs_panic_runtime-"))
                .unwrap_or(false)
        })
        .cloned()
}

/// Forward ld's captured stderr to our process stderr, skipping the
/// `INTENTIONAL_DUP_SYMBOLS` warning blocks (header + indented body
/// continuation lines). Used in place of `Stdio::inherit()` on the ld
/// child process.
pub(super) fn forward_filtered_stderr(stderr: &str) {
    let mut in_intentional_warning_body = false;
    for line in stderr.lines() {
        let trimmed = line.trim_start();
        if line.starts_with("ld: warning: duplicate symbol")
            && INTENTIONAL_DUP_SYMBOLS.iter().any(|s| line.contains(s))
        {
            in_intentional_warning_body = true;
            continue;
        }
        // Body lines of an ld warning are indented. A non-indented line
        // ends the suppression block.
        if in_intentional_warning_body && (trimmed.is_empty() || line == trimmed) {
            in_intentional_warning_body = false;
        }
        if in_intentional_warning_body {
            continue;
        }
        eprintln!("{line}");
    }
}

/// `path` exists check used by `link.rs` to skip wire if the panic_runtime
/// archive hasn't been emitted yet (very early build, missing build.rs run).
#[allow(dead_code)]
pub(super) fn _archive_exists(p: &Path) -> bool {
    p.exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_returns_none_when_archive_absent() {
        let paths = vec![
            PathBuf::from("/tmp/libtorajs_str-1-aa.a"),
            PathBuf::from("/tmp/libtorajs_rc-1-bb.a"),
        ];
        assert!(pick_panic_runtime_archive(&paths).is_none());
    }

    #[test]
    fn pick_finds_libtorajs_panic_runtime_archive() {
        let paths = vec![
            PathBuf::from("/tmp/libtorajs_str-1-aa.a"),
            PathBuf::from("/tmp/libtorajs_panic_runtime-99-cd.a"),
            PathBuf::from("/tmp/libtorajs_rc-1-bb.a"),
        ];
        let found = pick_panic_runtime_archive(&paths).expect("must find");
        assert_eq!(
            found.file_name().unwrap().to_str().unwrap(),
            "libtorajs_panic_runtime-99-cd.a"
        );
    }
}
