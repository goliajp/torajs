//! Str padding — `s.padStart(targetLen, padStr)` /
//! `s.padEnd(targetLen, padStr)`.
//!
//! `target_len` is the JS-spec **code unit** count per ES §22.1.3.18.
//! The result is allocated with the widest input encoding (Latin-1 if
//! both `s` and `pad` are Latin-1, else UTF-16 LE), and the Latin-1
//! side is widened to UTF-16 when the encodings disagree.
//!
//! Behavior:
//! - If `target_len < 0` or `target_len <= s.length`: return a fresh
//!   alloc holding `s` unchanged (ownership uniform with the
//!   padded path).
//! - Otherwise allocate `target_len` code units, fill the missing
//!   `target_len - s.length` slot with code units from `pad`
//!   (cycling), and copy `s` into the remaining slot. `padStart`
//!   prepends; `padEnd` appends.
//! - If `pad.length == 0`: fill with U+0020 (ASCII space). Spec
//!   actually says "return the original" but the C subset writes
//!   spaces; we preserve that for byte-equivalent ABI.
//!
//! IR-side surface (declared in `ssa_lower::lower`, alloc-noalias
//! whitelisted in `ssa_inkwell::is_alloc_intrinsic`):
//! - `__torajs_str_pad_start(s, target_len: i64, pad) -> Str`
//! - `__torajs_str_pad_end(s, target_len: i64, pad) -> Str`

use crate::block::StrBlock;
use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use alloc::vec::Vec;
use torajs_rc::HeapHeader;

// ============================================================
// Layout-aware FFI helpers (sub-module-local)
// ============================================================

#[inline]
unsafe fn str_view<'a>(p: *const u8) -> (&'a [u8], u32, bool) {
    let length = unsafe { (p.add(STR_LEN_OFF) as *const u32).read() };
    let header = unsafe { &*(p as *const HeapHeader) };
    let is_latin1 = (header.flags & STR_FLAG_IS_LATIN1) != 0;
    let byte_cnt = if is_latin1 {
        length as usize
    } else {
        (length as usize) * 2
    };
    let payload = unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), byte_cnt) };
    (payload, length, is_latin1)
}

#[inline]
fn widen_latin1_to_utf16(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() * 2);
    for &b in src {
        out.push(b);
        out.push(0);
    }
    out
}

/// Bring `payload` into the requested output encoding. Returns a
/// borrowed slice when `src_is_latin1 == out_is_latin1`; allocates
/// a widened buffer otherwise (Latin-1 → UTF-16 LE).
enum PayloadBuf<'a> {
    Borrowed(&'a [u8]),
    Owned(Vec<u8>),
}

impl<'a> AsRef<[u8]> for PayloadBuf<'a> {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Borrowed(s) => s,
            Self::Owned(v) => v.as_slice(),
        }
    }
}

#[inline]
fn coerce_payload<'a>(src: &'a [u8], src_is_latin1: bool, out_is_latin1: bool) -> PayloadBuf<'a> {
    if src_is_latin1 == out_is_latin1 {
        PayloadBuf::Borrowed(src)
    } else {
        // Only Latin-1 → UTF-16 ever happens in this module since
        // `out_is_latin1 = s_latin1 && pad_latin1`; the reverse
        // (UTF-16 → Latin-1) would require all codepoints ≤ 0xFF
        // which the spec doesn't promise, so we never do it.
        PayloadBuf::Owned(widen_latin1_to_utf16(src))
    }
}

// ============================================================
// Pure-Rust cores
// ============================================================

/// Fill `dst` (a single-encoding byte buffer) with `pad` bytes,
/// cycling as needed. Empty `pad` falls back to space — the unit
/// is `space_code_unit_bytes` (`b" "` for Latin-1, `b"\x20\x00"`
/// for UTF-16 LE). `stride` is the byte width of one code unit
/// in the destination encoding (1 or 2).
#[inline]
pub fn fill_with_pad(dst: &mut [u8], pad: &[u8], stride: usize, space_unit: &[u8]) {
    if pad.is_empty() {
        // Cycle the space code unit across `dst`.
        for (i, b) in dst.iter_mut().enumerate() {
            *b = space_unit[i % stride];
        }
        return;
    }
    if pad.len() == stride {
        // Fast path: one-code-unit pad → tight memset-ish loop.
        for (i, b) in dst.iter_mut().enumerate() {
            *b = pad[i % stride];
        }
        return;
    }
    for (i, b) in dst.iter_mut().enumerate() {
        *b = pad[i % pad.len()];
    }
}

/// `i64 target_len` → optional output **code-unit** length, taking
/// the passthrough decision into account. Returns `None` if the
/// original `s` should be returned unchanged (negative or
/// already-meets-target).
#[inline]
pub fn pad_output_len(target_len: i64, s_len: u32) -> Option<u32> {
    if target_len < 0 || (target_len as u64) <= s_len as u64 {
        None
    } else {
        Some(target_len as u32)
    }
}

// ============================================================
// extern "C" wrappers
// ============================================================

/// Common entry: build the result Str for either pad variant. The
/// caller picks whether `s` goes before (`pad_end=false` → padStart)
/// or after (`pad_end=true` → padEnd) the pad fill.
unsafe fn pad_impl(s: *const u8, target_len: i64, pad: *const u8, pad_end: bool) -> *mut u8 {
    let (s_payload, s_length, s_latin1) = unsafe { str_view(s) };
    let Some(out_length) = pad_output_len(target_len, s_length) else {
        // Pass-through path — fresh alloc with `s`'s encoding.
        let mut block = StrBlock::alloc_with_encoding(s_length, s_latin1);
        if !s_payload.is_empty() {
            let dst = unsafe { block.as_bytes_mut(s_payload.len() as u32) };
            dst.copy_from_slice(s_payload);
        }
        return block.into_raw();
    };
    let (pad_payload, _, pad_latin1) = unsafe { str_view(pad) };
    let out_latin1 = s_latin1 && pad_latin1;
    let stride = if out_latin1 { 1usize } else { 2usize };
    let space_unit: [u8; 2] = if out_latin1 {
        [0x20, 0x00]
    } else {
        [0x20, 0x00]
    };
    // (Same bytes for both encodings — the loop only reads the
    // first `stride` of them. Keeping a single buffer simplifies
    // the call below.)
    let out_byte_cnt = (out_length as usize) * stride;
    let s_buf = coerce_payload(s_payload, s_latin1, out_latin1);
    let pad_buf = coerce_payload(pad_payload, pad_latin1, out_latin1);
    let s_byte_cnt = s_buf.as_ref().len();
    let mut block = StrBlock::alloc_with_encoding(out_length, out_latin1);
    let dst = unsafe { block.as_bytes_mut(out_byte_cnt as u32) };
    if pad_end {
        dst[..s_byte_cnt].copy_from_slice(s_buf.as_ref());
        fill_with_pad(
            &mut dst[s_byte_cnt..],
            pad_buf.as_ref(),
            stride,
            &space_unit,
        );
    } else {
        let fill_byte_cnt = out_byte_cnt - s_byte_cnt;
        fill_with_pad(
            &mut dst[..fill_byte_cnt],
            pad_buf.as_ref(),
            stride,
            &space_unit,
        );
        dst[fill_byte_cnt..].copy_from_slice(s_buf.as_ref());
    }
    block.into_raw()
}

/// `s.padStart(targetLen, padStr)`. Prepends pad code units so the
/// result has exactly `target_len` code units.
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
    unsafe { pad_impl(s, target_len, pad, false) }
}

/// `s.padEnd(targetLen, padStr)`. Appends pad code units so the
/// result has exactly `target_len` code units.
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
    unsafe { pad_impl(s, target_len, pad, true) }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    use crate::block::__torajs_str_free;
    use alloc::vec::Vec;

    fn make_str(payload: &[u8]) -> *mut u8 {
        let mut b = StrBlock::alloc(payload.len() as u32);
        let dst = unsafe { b.as_bytes_mut(payload.len() as u32) };
        dst.copy_from_slice(payload);
        b.into_raw()
    }

    fn read_payload(p: *const u8) -> Vec<u8> {
        let (payload, _, _) = unsafe { str_view(p) };
        payload.to_vec()
    }

    #[test]
    fn ffi_pad_start_zero_prepends() {
        let s = make_str(b"5");
        let pad = make_str(b"0");
        let r = unsafe { __torajs_str_pad_start(s, 3, pad) };
        assert_eq!(read_payload(r), b"005");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(pad) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_pad_end_zero_appends() {
        let s = make_str(b"5");
        let pad = make_str(b"0");
        let r = unsafe { __torajs_str_pad_end(s, 3, pad) };
        assert_eq!(read_payload(r), b"500");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(pad) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_pad_start_cycles_multi_byte() {
        let s = make_str(b"abc");
        let pad = make_str(b"xy");
        let r = unsafe { __torajs_str_pad_start(s, 8, pad) };
        assert_eq!(read_payload(r), b"xyxyxabc");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(pad) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_pad_end_cycles_multi_byte() {
        let s = make_str(b"abc");
        let pad = make_str(b"xy");
        let r = unsafe { __torajs_str_pad_end(s, 8, pad) };
        assert_eq!(read_payload(r), b"abcxyxyx");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(pad) };
        unsafe { __torajs_str_free(r) };
    }

    #[test]
    fn ffi_pad_start_target_leq_slen_is_passthrough() {
        let s = make_str(b"hello");
        let pad = make_str(b"x");
        let r = unsafe { __torajs_str_pad_start(s, 3, pad) };
        assert_eq!(read_payload(r), b"hello");
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
        assert_eq!(read_payload(rs), b"hi");
        assert_eq!(read_payload(re), b"hi");
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
        assert_eq!(read_payload(r), b"   42");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(pad) };
        unsafe { __torajs_str_free(r) };
    }
}
