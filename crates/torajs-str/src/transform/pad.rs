//! Str padding — `s.padStart(targetLen, padStr)` /
//! `s.padEnd(targetLen, padStr)`.
//!
//! **Byte-length semantics** (not UTF-16 code-unit). Matches the
//! pre-rewrite C `__torajs_str_pad_start` / `_pad_end` byte-level
//! contract; bun-parity holds for ASCII inputs.
//!
//! Behavior:
//! - If `target_len < 0` or `target_len <= s.len()`: return a fresh
//!   alloc holding `s` unchanged (ownership uniform with the
//!   padded path).
//! - Otherwise allocate `target_len` bytes, fill the missing
//!   `target_len - s.len()` slot with bytes from `pad` (cycling),
//!   and copy `s` into the remaining slot. `padStart` prepends;
//!   `padEnd` appends.
//! - If `pad.len() == 0`: fill with `b' '` (space). Spec actually
//!   says "return the original" but the C subset writes spaces;
//!   we preserve that for byte-equivalent ABI.
//!
//! IR-side surface (declared in `ssa_lower::lower`, alloc-noalias
//! whitelisted in `ssa_inkwell::is_alloc_intrinsic`):
//! - `__torajs_str_pad_start(s, target_len: i64, pad) -> Str`
//! - `__torajs_str_pad_end(s, target_len: i64, pad) -> Str`

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

#[inline]
unsafe fn alloc_slice(src: &[u8]) -> *mut u8 {
    let out_len = src.len() as u64;
    let mut block = StrBlock::alloc(out_len);
    if !src.is_empty() {
        let dst = unsafe { block.as_bytes_mut(out_len) };
        dst.copy_from_slice(src);
    }
    block.into_raw()
}

// ============================================================
// Pure-Rust cores
// ============================================================

/// Fill `dst` with bytes from `pad`, cycling as needed. Empty
/// `pad` falls back to ASCII space (matches the C subset). Hot
/// for short pads where `i % pad.len()` reduces to single-cycle
/// indexing — LLVM hoists the modulo on constant-length pads.
#[inline]
pub fn fill_with_pad(dst: &mut [u8], pad: &[u8]) {
    if pad.is_empty() {
        dst.fill(b' ');
        return;
    }
    if pad.len() == 1 {
        // Fast path: single-byte pad → memset-equivalent.
        dst.fill(pad[0]);
        return;
    }
    for (i, b) in dst.iter_mut().enumerate() {
        *b = pad[i % pad.len()];
    }
}

/// `i64 target_len` → optional output length, taking the
/// passthrough decision into account. Returns `None` if the
/// original `s` should be returned unchanged (negative or
/// already-meets-target).
#[inline]
pub fn pad_output_len(target_len: i64, s_len: u64) -> Option<u64> {
    if target_len < 0 || (target_len as u64) <= s_len {
        None
    } else {
        Some(target_len as u64)
    }
}

// ============================================================
// extern "C" wrappers
// ============================================================

/// `s.padStart(targetLen, padStr)`. Prepends pad bytes so the
/// result has exactly `target_len` bytes.
///
/// # Safety
///
/// `s` and `pad` must be valid Str heap blocks. Returned pointer
/// is a fresh refcount=1 Str block owned by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_pad_start(
    s: *const u8,
    target_len: i64,
    pad: *const u8,
) -> *mut u8 {
    let s_len = unsafe { str_len(s) };
    let s_bytes = unsafe { str_bytes(s, s_len) };
    let Some(out_len) = pad_output_len(target_len, s_len) else {
        return unsafe { alloc_slice(s_bytes) };
    };
    let pad_len = unsafe { str_len(pad) };
    let pad_bytes = unsafe { str_bytes(pad, pad_len) };
    let mut block = StrBlock::alloc(out_len);
    let dst = unsafe { block.as_bytes_mut(out_len) };
    let need = (out_len - s_len) as usize;
    fill_with_pad(&mut dst[..need], pad_bytes);
    dst[need..].copy_from_slice(s_bytes);
    block.into_raw()
}

/// `s.padEnd(targetLen, padStr)`. Appends pad bytes so the result
/// has exactly `target_len` bytes.
///
/// # Safety
///
/// See [`__torajs_str_pad_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_pad_end(
    s: *const u8,
    target_len: i64,
    pad: *const u8,
) -> *mut u8 {
    let s_len = unsafe { str_len(s) };
    let s_bytes = unsafe { str_bytes(s, s_len) };
    let Some(out_len) = pad_output_len(target_len, s_len) else {
        return unsafe { alloc_slice(s_bytes) };
    };
    let pad_len = unsafe { str_len(pad) };
    let pad_bytes = unsafe { str_bytes(pad, pad_len) };
    let mut block = StrBlock::alloc(out_len);
    let dst = unsafe { block.as_bytes_mut(out_len) };
    let s_used = s_len as usize;
    dst[..s_used].copy_from_slice(s_bytes);
    fill_with_pad(&mut dst[s_used..], pad_bytes);
    block.into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_empty_pad_falls_back_to_space() {
        let mut dst = [0u8; 4];
        fill_with_pad(&mut dst, b"");
        assert_eq!(&dst, b"    ");
    }

    #[test]
    fn fill_single_byte_pad_uses_fast_path() {
        let mut dst = [0u8; 4];
        fill_with_pad(&mut dst, b"*");
        assert_eq!(&dst, b"****");
    }

    #[test]
    fn fill_multi_byte_pad_cycles() {
        let mut dst = [0u8; 7];
        fill_with_pad(&mut dst, b"xy");
        assert_eq!(&dst, b"xyxyxyx");
    }

    #[test]
    fn fill_pad_exact_length_no_cycle() {
        let mut dst = [0u8; 4];
        fill_with_pad(&mut dst, b"abcd");
        assert_eq!(&dst, b"abcd");
    }

    #[test]
    fn output_len_passthrough_paths() {
        assert_eq!(pad_output_len(-1, 5), None);
        assert_eq!(pad_output_len(0, 5), None);
        assert_eq!(pad_output_len(5, 5), None);
        assert_eq!(pad_output_len(4, 5), None);
    }

    #[test]
    fn output_len_extends() {
        assert_eq!(pad_output_len(8, 5), Some(8));
        assert_eq!(pad_output_len(100, 0), Some(100));
    }

    // ============================================================
    // FFI round-trip tests
    // ============================================================

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
    fn ffi_pad_start_zero_prepends() {
        let s = make_str(b"5");
        let pad = make_str(b"0");
        let r = unsafe { __torajs_str_pad_start(s, 3, pad) };
        assert_eq!(read_str(r), b"005");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(pad) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_pad_end_zero_appends() {
        let s = make_str(b"5");
        let pad = make_str(b"0");
        let r = unsafe { __torajs_str_pad_end(s, 3, pad) };
        assert_eq!(read_str(r), b"500");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(pad) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_pad_start_cycles_multi_byte() {
        let s = make_str(b"abc");
        let pad = make_str(b"xy");
        let r = unsafe { __torajs_str_pad_start(s, 8, pad) };
        assert_eq!(read_str(r), b"xyxyxabc");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(pad) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_pad_end_cycles_multi_byte() {
        let s = make_str(b"abc");
        let pad = make_str(b"xy");
        let r = unsafe { __torajs_str_pad_end(s, 8, pad) };
        assert_eq!(read_str(r), b"abcxyxyx");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(pad) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_pad_start_target_leq_slen_is_passthrough() {
        let s = make_str(b"hello");
        let pad = make_str(b"x");
        let r = unsafe { __torajs_str_pad_start(s, 3, pad) };
        assert_eq!(read_str(r), b"hello");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(pad) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_pad_negative_target_is_passthrough() {
        let s = make_str(b"hi");
        let pad = make_str(b"x");
        let rs = unsafe { __torajs_str_pad_start(s, -1, pad) };
        let re = unsafe { __torajs_str_pad_end(s, -1, pad) };
        assert_eq!(read_str(rs), b"hi");
        assert_eq!(read_str(re), b"hi");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(pad) };
        unsafe { __torajs_str_free(rs) };
        unsafe { __torajs_str_free(re) };
    }

    #[test]
    fn ffi_pad_empty_pad_fills_with_space() {
        let s = make_str(b"42");
        let pad = make_str(b"");
        let r = unsafe { __torajs_str_pad_start(s, 5, pad) };
        assert_eq!(read_str(r), b"   42");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(pad) };
        unsafe { __torajs_str_free(r) };
    }
}
