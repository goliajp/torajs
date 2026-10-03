//! `__torajs_str_concat_i64` / `__torajs_str_concat_f64` — fused
//! `a + String(n)` in one allocation.
//!
//! The lowering spells `lit + i.toString()` (and the implicit
//! `lit + i` coerce) as three adjacent calls:
//!
//! ```text
//! %t = call __torajs_i64_to_str(%n)
//! %r = call __torajs_str_concat(%a, %t)
//! call __torajs_str_drop(%t)
//! ```
//!
//! which pays a full Str cell round-trip (alloc + digit copy +
//! drop) for a value whose only reader is the concat one
//! instruction later. torajs-egraph's `concat_num_fuse` pass
//! rewrites the triple onto these kernels, which format the digits
//! into a stack buffer and write them straight after `a`'s payload
//! in the single result allocation — the intermediate cell never
//! exists. S1-A2 decomposition attack B1.
//!
//! Digits are always ASCII, so the result encoding is simply `a`'s
//! encoding: Latin-1 keeps a byte-wise tail write, UTF-16 widens
//! each digit to a `(byte, 0)` pair. A NULL `a` (the JS `undefined`
//! sentinel in a Str-typed slot, RC-4 F1b-2) takes the cold path:
//! mint the digits as a real Str and delegate to
//! [`__torajs_str_concat`], which owns the undefined-text rule.

use crate::block::StrBlock;
use crate::concat::{
    __torajs_str_concat, str_is_latin1, str_len, str_payload, widen_latin1_into_utf16,
};
use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_ASCII};
use crate::str_drop::__torajs_str_drop;
use torajs_rc::HeapHeader;

unsafe extern "C" {
    /// JS-spec f64 → decimal digits (NaN / ±Infinity literals,
    /// `String(-0)` → `"0"`), provided by libtorajs_num.a at link
    /// time (same-layer C-ABI bridge, see torajs-num
    /// `str_bridge.rs` module docs for the pattern). Writes into a
    /// ≥ 32-byte buffer, returns the byte count.
    fn __torajs_f64_js_digits(d: f64, out: *mut u8) -> i64;
}

/// Shared tail: fresh Str holding `a ++ digits`, where `write` fills
/// the `d_len` ASCII digit bytes at the pointer it is handed. A
/// Latin-1 `a` hands it the result's own tail, so the digits land in
/// place; a UTF-16 `a` hands it a stack line and widens.
///
/// # Safety
/// `a` must be a valid, non-NULL Str heap block; `d_len` ≤ 32.
unsafe fn concat_ascii_tail(a: *const u8, d_len: usize, write: impl FnOnce(*mut u8)) -> *mut u8 {
    let a_len = unsafe { str_len(a) };
    let a_is_latin1 = unsafe { str_is_latin1(a) };
    let total_len = a_len + d_len as u32;
    // Digits are ASCII, so the result is exactly as ASCII as `a`.
    let ascii = unsafe { (*(a as *const HeapHeader)).flags } & STR_FLAG_IS_ASCII;
    let block = StrBlock::alloc_with_flags(total_len, a_is_latin1, ascii);
    let unit = if a_is_latin1 { 1 } else { 2 };
    let a_byte_cnt = a_len as usize * unit;
    // SAFETY: the block's payload holds `a_byte_cnt + d_len * unit`
    // bytes and `a`'s holds `a_byte_cnt`
    unsafe {
        let dst = block.into_raw();
        let payload = dst.add(STR_DATA_OFF);
        let a_payload = str_payload(a, a_byte_cnt);
        core::ptr::copy_nonoverlapping(a_payload.as_ptr(), payload, a_byte_cnt);
        let d_slot = payload.add(a_byte_cnt);
        if a_is_latin1 {
            write(d_slot);
        } else {
            let mut line = [0u8; 32];
            write(line.as_mut_ptr());
            let wide = core::slice::from_raw_parts_mut(d_slot, d_len * 2);
            widen_latin1_into_utf16(&line[..d_len], wide);
        }
        dst
    }
}

/// Cold path for a NULL `a`: mint the digits as a real Str and let
/// [`__torajs_str_concat`] apply its undefined-text rule, dropping
/// the temp after. Bit-for-bit the pre-fusion behavior.
#[cold]
#[inline(never)]
unsafe fn concat_null_cold(a: *const u8, digits: &[u8]) -> *mut u8 {
    let mut block = StrBlock::alloc_with_encoding(digits.len() as u32, true);
    let dst = unsafe { block.as_bytes_mut(digits.len() as u32) };
    dst.copy_from_slice(digits);
    let tmp = block.into_raw();
    let r = unsafe { __torajs_str_concat(a, tmp) };
    unsafe { __torajs_str_drop(tmp) };
    r
}

/// `a + String(n)` for i64 `n` — one allocation, no intermediate
/// Str cell.
///
/// # Safety
/// `a` is a valid Str heap block or NULL (undefined sentinel).
/// Returned pointer is a fresh refcount=1 Str owned by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_concat_i64(a: *const u8, n: i64) -> *mut u8 {
    use torajs_fmt::itoa::{itoa_len, itoa_write};
    let d_len = itoa_len(n);
    if a.is_null() {
        let mut buf = [0u8; 32];
        unsafe { itoa_write(n, buf.as_mut_ptr(), d_len) };
        return unsafe { concat_null_cold(a, &buf[..d_len]) };
    }
    unsafe { concat_ascii_tail(a, d_len, |slot| itoa_write(n, slot, d_len)) }
}

/// `a + String(d)` for f64 `d` — one allocation, no intermediate
/// Str cell. Digit semantics (shortest round-trip, NaN / ±Infinity,
/// `String(-0)` → `"0"`) live in torajs-num's
/// `__torajs_f64_js_digits`, the same source `__torajs_f64_to_str`
/// formats through.
///
/// # Safety
/// See [`__torajs_str_concat_i64`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_concat_f64(a: *const u8, d: f64) -> *mut u8 {
    let mut buf = [0u8; 32];
    let len = unsafe { __torajs_f64_js_digits(d, buf.as_mut_ptr()) };
    let digits = &buf[..len as usize];
    if a.is_null() {
        return unsafe { concat_null_cold(a, digits) };
    }
    unsafe {
        concat_ascii_tail(a, digits.len(), |slot| {
            core::ptr::copy_nonoverlapping(digits.as_ptr(), slot, digits.len())
        })
    }
}
