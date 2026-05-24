//! Str console print — `console.log(str)` (stdout) and
//! `console.error(str)` (stderr) SSA dispatch targets.
//!
//! Both `__torajs_str_print` and `__torajs_str_print_err` live
//! here as of P3.1-g.2 (2026-05-23).
//!
//! **Buffer-sharing constraint**: `__torajs_str_print` (stdout)
//! intentionally uses `extern "C" putchar` per-byte rather than
//! Rust `std::io::stdout`, because the still-IR-emitted
//! `print_i64` / `print_f64` / `print_bool` use putchar via the C
//! stdio buffer. If `__torajs_str_print` bypassed that buffer (as
//! `std::io::stdout` does), mixed `console.log("a"); console.log(5)`
//! sequences would reorder on flush. Per-byte putchar is slower
//! than a single `write(2)` but is the minimal cross-buffer fix
//! until print_i64 et al. also port to Rust (later P3.1-g sub-step).
//!
//! `__torajs_str_print_err` uses `std::io::stderr` because the
//! stderr-writing sibling fns (`print_i64_err` etc.) all go through
//! C stdio's stderr, which is line-buffered for terminals and
//! flushes after each '\n' on POSIX — interleaving risk is low
//! enough that the bulk-write win pays.
//!
//! NULL → `"null\n"` (Nullable<Str> slots + uncaptured regex
//! groups pass NULL through; printing "null" matches
//! `console.error(null)` semantics).

use std::io::Write;

use crate::layout::{STR_DATA_OFF, STR_LEN_OFF};
use crate::substr::{SUBSTR_LEN_OFF, SUBSTR_OFFSET_OFF, SUBSTR_PARENT_OFF};

// ============================================================
// Pure-Rust core
// ============================================================

/// Compose the bytes that [`__torajs_str_print_err`] would write,
/// for unit-testability of the byte-slicing path. Production
/// callers use the extern wrapper which writes directly to stderr.
#[inline]
pub fn format_print_err(payload: Option<&[u8]>) -> Vec<u8> {
    match payload {
        None => b"null\n".to_vec(),
        Some(bytes) => {
            let mut out = Vec::with_capacity(bytes.len() + 1);
            out.extend_from_slice(bytes);
            out.push(b'\n');
            out
        }
    }
}

// ============================================================
// extern "C" wrappers
// ============================================================

unsafe extern "C" {
    fn putchar(c: i32) -> i32;
}

/// `console.log(str)` — write `s`'s payload bytes + newline to
/// stdout via per-byte `putchar`. NULL → `"null\n"`.
///
/// Uses putchar (NOT `std::io::stdout`) so the output shares C
/// stdio's stdout buffer with `print_i64` / `print_f64` /
/// `print_bool` — otherwise mixed-type `console.log` sequences
/// reorder on flush. See module docs for the cross-buffer detail.
///
/// # Safety
///
/// `s` must be either NULL or a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_print(s: *const u8) {
    if s.is_null() {
        for &b in b"null\n" {
            unsafe { putchar(b as i32) };
        }
        return;
    }
    let len = unsafe { (s.add(STR_LEN_OFF) as *const u64).read() } as usize;
    if len > 0 {
        let bytes = unsafe { core::slice::from_raw_parts(s.add(STR_DATA_OFF), len) };
        for &b in bytes {
            unsafe { putchar(b as i32) };
        }
    }
    unsafe { putchar(b'\n' as i32) };
}

/// `console.log(substr)` — write a Substr's view (parent bytes +
/// offset slice) + newline to stdout via per-byte `putchar`. Substr
/// layout `{ hdr@0, len@8, parent@16, offset@24 }` is read directly
/// (no materialize). NULL → `"null\n"`.
///
/// Same buffer-sharing concern as [`__torajs_str_print`]: this is
/// the console.log path for Substr-typed receivers, so it must use
/// the same stdio buffer as print_i64 / print_bool / str_print.
///
/// # Safety
///
/// `v` must be NULL or a valid Substr heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_print(v: *const u8) {
    if v.is_null() {
        for &b in b"null\n" {
            unsafe { putchar(b as i32) };
        }
        return;
    }
    let len = unsafe { (v.add(SUBSTR_LEN_OFF) as *const u64).read() } as usize;
    let parent = unsafe { (v.add(SUBSTR_PARENT_OFF) as *const *const u8).read() };
    let offset = unsafe { (v.add(SUBSTR_OFFSET_OFF) as *const u64).read() } as usize;
    if len > 0 {
        let bytes = unsafe { core::slice::from_raw_parts(parent.add(STR_DATA_OFF + offset), len) };
        for &b in bytes {
            unsafe { putchar(b as i32) };
        }
    }
    unsafe { putchar(b'\n' as i32) };
}

/// `console.error(str)` — write `s`'s payload bytes + newline to
/// stderr. NULL → `"null\n"`. Same single-lock pattern as
/// [`__torajs_str_print`] above, just on stderr.
///
/// # Safety
///
/// `s` must be either NULL or a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_print_err(s: *const u8) {
    let stderr = std::io::stderr();
    let mut lock = stderr.lock();
    if s.is_null() {
        let _ = lock.write_all(b"null\n");
        return;
    }
    let len = unsafe { (s.add(STR_LEN_OFF) as *const u64).read() } as usize;
    if len > 0 {
        let bytes = unsafe { core::slice::from_raw_parts(s.add(STR_DATA_OFF), len) };
        let _ = lock.write_all(bytes);
    }
    let _ = lock.write_all(b"\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_null_yields_literal_string() {
        assert_eq!(format_print_err(None), b"null\n");
    }

    #[test]
    fn format_empty_payload_yields_just_newline() {
        assert_eq!(format_print_err(Some(b"")), b"\n");
    }

    #[test]
    fn format_appends_newline_to_payload() {
        assert_eq!(format_print_err(Some(b"hello")), b"hello\n");
    }

    #[test]
    fn format_preserves_non_utf8_bytes() {
        // Byte-level Str layout: raw bytes pass through unchanged,
        // including 0xFF and other non-UTF-8 sequences.
        assert_eq!(format_print_err(Some(b"\xFF\x00\x80")), b"\xFF\x00\x80\n");
    }
}
