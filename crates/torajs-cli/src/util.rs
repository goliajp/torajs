//! Stateless utility fns extracted from `main.rs` during the
//! file-size known-debt sweep (cli/main.rs 1228 → < 500 LOC). None
//! of these depend on torajs-core or on any other module — they're
//! pure stdlib helpers used across the cmd_* sub-modules.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Format a byte count as a human-friendly MB / GB string (used by
/// `tr cache size` reporting).
pub(crate) fn fmt_mb(bytes: u64) -> String {
    let mb = bytes as f64 / 1024.0 / 1024.0;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{:.0} MB", mb)
    }
}

/// Recursive directory-size sum. Returns 0 on any read failure
/// (cache reports should degrade gracefully on missing / unreadable
/// entries rather than abort the whole subcommand).
pub(crate) fn dir_size_bytes(p: &Path) -> u64 {
    let mut sum: u64 = 0;
    let read = match std::fs::read_dir(p) {
        Ok(r) => r,
        Err(_) => return 0,
    };
    for ent in read.flatten() {
        let meta = match ent.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            sum += dir_size_bytes(&ent.path());
        } else {
            sum += meta.len();
        }
    }
    sum
}

/// `execvp`-replace the current process with `p`. Used by `tr run`'s
/// JIT-cache fast path so we hand the cached binary's exit code +
/// signal back to the original caller transparently.
pub(crate) fn exec_binary(p: &Path) -> ExitCode {
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new(p).exec();
    eprintln!("exec {}: {err}", p.display());
    ExitCode::from(1)
}

/// Hex-encoded nanos-since-epoch suffix for per-build temp dir names.
/// Cheap + unique-enough for the few hundred parallel `tr build`
/// invocations a single host might run (the bench / conformance
/// harness already collision-tests this empirically).
pub(crate) fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{n:x}")
}

/// Read source from a path or `-` for stdin. Mirrors common Unix-CLI
/// convention.
pub(crate) fn read_source(arg: &str) -> Result<String, String> {
    if arg == "-" {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| format!("reading stdin: {e}"))?;
        Ok(s)
    } else {
        std::fs::read_to_string(arg).map_err(|e| format!("reading {arg}: {e}"))
    }
}

/// Directory that relative `import` paths resolve against. For a file
/// argument, that's the file's parent directory (canonicalized so
/// `import "./x"` and `import "x"` from `./bench/foo.ts` both land at
/// the same absolute path). For stdin, fall back to the current
/// working directory — `import "./x"` from stdin means `./x` from
/// `cwd`.
pub(crate) fn base_dir_for(file_arg: &str) -> PathBuf {
    if file_arg == "-" {
        return std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    }
    let p = PathBuf::from(file_arg);
    let parent = p.parent().filter(|p| !p.as_os_str().is_empty());
    let dir = match parent {
        Some(d) => d.to_path_buf(),
        None => PathBuf::from("."),
    };
    dir.canonicalize().unwrap_or(dir)
}

/// Tiny utility: convert a byte offset to (line, col) (both 1-based).
/// `(0, 0)` byte offset (the no-source-location sentinel from the
/// Diagnostic substrate) maps to line 1, col 1 — same convention as
/// the LSP's file:1:1 anchor.
pub(crate) fn byte_to_line_col(text: &str, byte: u32) -> (u32, u32) {
    let target = byte as usize;
    let mut line = 1u32;
    let mut col = 1u32;
    for (i, ch) in text.char_indices() {
        if i >= target {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}
