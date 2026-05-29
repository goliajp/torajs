//! Str console print — `console.log(str)` (stdout) and
//! `console.error(str)` (stderr) SSA dispatch targets.
//!
//! Both `__torajs_str_print` and `__torajs_str_print_err` live
//! here as of P3.1-g.2 (2026-05-23).
//!
//! **Buffer-sharing constraint**: `__torajs_str_print` (stdout)
//! uses `torajs_io::__torajs_io_putc_stdout` per-byte (v0.7-A3
//! Step 14-b cutover from libc `putchar`). The IR-emitted
//! `print_i64` / `print_f64` / `print_bool` in `ssa_inkwell/
//! builders.rs` also route through the same symbol (Step 14-c
//! cutover), so all stdout writers share torajs-io's process-
//! global line buffer. No cross-buffer reordering risk: every
//! console.log call ends with '\n' which triggers a flush.
//!
//! `__torajs_str_print_err` (console.error) composes the payload +
//! newline into one buffer and emits it with a single `write(2)`
//! syscall via [`crate::write_stderr`] (v0.7-A5 Step 16-e no_std
//! cutover from `std::io::stderr`). One write keeps the line atomic;
//! the runtime is single-threaded so no cross-writer interleave.
//!
//! NULL → `"null\n"` (Nullable<Str> slots + uncaptured regex
//! groups pass NULL through; printing "null" matches
//! `console.error(null)` semantics).

use alloc::vec::Vec;

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
    fn __torajs_io_putc_stdout(c: i32) -> i32;
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
            unsafe { __torajs_io_putc_stdout(b as i32) };
        }
        return;
    }
    let len = unsafe { (s.add(STR_LEN_OFF) as *const u64).read() } as usize;
    if len > 0 {
        let bytes = unsafe { core::slice::from_raw_parts(s.add(STR_DATA_OFF), len) };
        for &b in bytes {
            unsafe { __torajs_io_putc_stdout(b as i32) };
        }
    }
    unsafe { __torajs_io_putc_stdout(b'\n' as i32) };
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
            unsafe { __torajs_io_putc_stdout(b as i32) };
        }
        return;
    }
    let len = unsafe { (v.add(SUBSTR_LEN_OFF) as *const u64).read() } as usize;
    let parent = unsafe { (v.add(SUBSTR_PARENT_OFF) as *const *const u8).read() };
    let offset = unsafe { (v.add(SUBSTR_OFFSET_OFF) as *const u64).read() } as usize;
    if len > 0 {
        let bytes = unsafe { core::slice::from_raw_parts(parent.add(STR_DATA_OFF + offset), len) };
        for &b in bytes {
            unsafe { __torajs_io_putc_stdout(b as i32) };
        }
    }
    unsafe { __torajs_io_putc_stdout(b'\n' as i32) };
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
    let payload = if s.is_null() {
        None
    } else {
        let len = unsafe { (s.add(STR_LEN_OFF) as *const u64).read() } as usize;
        Some(unsafe { core::slice::from_raw_parts(s.add(STR_DATA_OFF), len) })
    };
    crate::write_stderr(&format_print_err(payload));
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
