//! Str stderr print — `console.error(str)` SSA dispatch target.
//!
//! Only `__torajs_str_print_err` (the stderr-fd path) lives here.
//! `__torajs_str_print` (stdout-fd) is still LLVM-IR-emitted by
//! `ssa_inkwell::define_str_print`; consolidates here in a later
//! P3.1-g sub-step when its sibling defines port together.
//!
//! NULL → `"null\n"` (Nullable<Str> slots + uncaptured regex
//! groups pass NULL through; printing "null" matches
//! `console.error(null)` semantics).

use std::io::Write;

use crate::layout::{STR_DATA_OFF, STR_LEN_OFF};

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
// extern "C" wrapper
// ============================================================

/// `console.error(str)` — write `s`'s payload bytes + newline to
/// stderr. NULL → `"null\n"`. Single lock guard around the write
/// so concurrent calls don't interleave a single line (the v0
/// runtime is single-threaded but the lock costs ~nothing).
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
