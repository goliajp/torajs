//! `__torajs_str_concat` — fresh Str holding `a.bytes ++ b.bytes`.
//!
//! Port of `ssa_inkwell::define_str_concat` (P3.1-g.4, 2026-05-23).
//! Single pool-aware alloc + two copy_from_slice calls; both operand
//! Strs are read-only and the caller's drops still fire on them.
//!
//! Returns NULL only if alloc panics on OOM (matches the IR shape:
//! str_alloc_pooled abort-on-OOM). Inputs must be valid Str heap
//! blocks; the IR-side caller always reads STR_LEN before the call
//! site so a NULL-Str input would have already crashed earlier.

use crate::alloc::StrBlock;
use crate::layout::{STR_DATA_OFF, STR_LEN_OFF};

// ============================================================
// Layout-aware FFI helpers (sub-module-local)
// ============================================================

#[inline]
unsafe fn str_len(p: *const u8) -> u64 {
    unsafe { (p.add(STR_LEN_OFF) as *const u64).read() }
}

#[inline]
unsafe fn str_bytes<'a>(p: *const u8, len: u64) -> &'a [u8] {
    unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), len as usize) }
}

// ============================================================
// extern "C" wrapper
// ============================================================

/// `a + b` for Str operands. Allocates `a.len + b.len` bytes,
/// copies `a` then `b` into the payload, returns the new Str.
///
/// # Safety
///
/// `a` and `b` must be valid Str heap blocks. Returned pointer is
/// a fresh refcount=1 Str block owned by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_concat(a: *const u8, b: *const u8) -> *mut u8 {
    let a_len = unsafe { str_len(a) };
    let b_len = unsafe { str_len(b) };
    let total = a_len + b_len;
    let mut block = StrBlock::alloc(total);
    let a_used = a_len as usize;
    let b_used = b_len as usize;
    if total > 0 {
        let dst = unsafe { block.as_bytes_mut(total) };
        if a_used > 0 {
            let a_bytes = unsafe { str_bytes(a, a_len) };
            dst[..a_used].copy_from_slice(a_bytes);
        }
        if b_used > 0 {
            let b_bytes = unsafe { str_bytes(b, b_len) };
            dst[a_used..a_used + b_used].copy_from_slice(b_bytes);
        }
    }
    block.into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::__torajs_str_free;

    fn make_str(payload: &[u8]) -> *mut u8 {
        let mut b = StrBlock::alloc(payload.len() as u64);
        let dst = unsafe { b.as_bytes_mut(payload.len() as u64) };
        dst.copy_from_slice(payload);
        b.into_raw()
    }

    fn read_str(p: *const u8) -> Vec<u8> {
        let len = unsafe { str_len(p) };
        unsafe { str_bytes(p, len) }.to_vec()
    }

    #[test]
    fn concat_basic() {
        let a = make_str(b"foo");
        let b = make_str(b"bar");
        let r = unsafe { __torajs_str_concat(a, b) };
        assert_eq!(read_str(r), b"foobar");
        unsafe { __torajs_str_free(a) };
        unsafe { __torajs_str_free(b) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn concat_first_empty() {
        let a = make_str(b"");
        let b = make_str(b"bar");
        let r = unsafe { __torajs_str_concat(a, b) };
        assert_eq!(read_str(r), b"bar");
        unsafe { __torajs_str_free(a) };
        unsafe { __torajs_str_free(b) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn concat_second_empty() {
        let a = make_str(b"foo");
        let b = make_str(b"");
        let r = unsafe { __torajs_str_concat(a, b) };
        assert_eq!(read_str(r), b"foo");
        unsafe { __torajs_str_free(a) };
        unsafe { __torajs_str_free(b) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn concat_both_empty() {
        let a = make_str(b"");
        let b = make_str(b"");
        let r = unsafe { __torajs_str_concat(a, b) };
        assert_eq!(read_str(r), b"");
        unsafe { __torajs_str_free(a) };
        unsafe { __torajs_str_free(b) };
        unsafe { __torajs_str_free(r) };
    }
}
