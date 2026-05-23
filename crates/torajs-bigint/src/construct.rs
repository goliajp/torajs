//! BigInt constructors — `__torajs_bigint_from_{decimal,hex,str,i64}`
//! + `__torajs_bigint_clone`.
//!
//! Port of `runtime_bigint.c` lines 141-265 + 324-350 (P3.3-b,
//! 2026-05-23). Each constructor returns a fresh `+1`-refcount heap
//! pointer; caller owns and must eventually route through
//! `__torajs_bigint_drop_rc`.
//!
//! `from_number` (line 273-322) is **deferred** to a later sub-step
//! because it depends on `static bigint_mag_shl_` / `bigint_mag_shr_`
//! (defined in runtime_bigint.c's shift section, scheduled for the
//! shift family port). It stays C-side until then.

use core::ffi::c_void;

use crate::internal::{
    add_u32_inplace, alloc_raw, free, mul_u32_inplace, normalize, read_len, words_mut, words_ptr,
    write_sign,
};
use crate::layout::{STR_HDR_SIZE, STR_LEN_OFF};
use crate::shift::{mag_shl, mag_shr};

unsafe extern "C" {
    fn __torajs_throw_range_error(msg: *const u8);
}

// ============================================================
// from_decimal — body bytes are ASCII '0'-'9'; non-digit chars
// silently skipped (lexer is expected to filter).
// ============================================================

/// `BigInt(<decimal-digit-string>)` for the SSA-lowered BigInt
/// literal path. `s` is a Str pointer (rodata-baked literal body);
/// `n` is the digit count.
///
/// # Safety
/// `s` is either NULL or a valid Str heap block pointer; `n` must
/// not exceed the Str's body byte count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_from_decimal(s: *const c_void, n: u64) -> *mut u8 {
    let mut b = unsafe { alloc_raw(0) };
    if s.is_null() {
        unsafe { normalize(b) };
        return b;
    }
    let bytes = unsafe { (s as *const u8).add(STR_HDR_SIZE) };
    for i in 0..n {
        let c = unsafe { *bytes.add(i as usize) };
        if !(b'0'..=b'9').contains(&c) {
            continue;
        }
        unsafe {
            mul_u32_inplace(&mut b, 10);
            add_u32_inplace(&mut b, (c - b'0') as u32);
        }
    }
    unsafe { normalize(b) };
    b
}

// ============================================================
// from_hex — `0x` already stripped by the SSA lowering. Tolerant
// of mixed case + skips non-hex chars.
// ============================================================

/// `BigInt(<hex-digit-string>)`. `s` is a Str pointer; `n` is
/// the digit count (excluding any `0x` prefix).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_from_hex(s: *const c_void, n: u64) -> *mut u8 {
    let mut b = unsafe { alloc_raw(0) };
    if s.is_null() {
        unsafe { normalize(b) };
        return b;
    }
    let bytes = unsafe { (s as *const u8).add(STR_HDR_SIZE) };
    for i in 0..n {
        let c = unsafe { *bytes.add(i as usize) };
        let d = if (b'0'..=b'9').contains(&c) {
            (c - b'0') as u32
        } else if (b'a'..=b'f').contains(&c) {
            10 + (c - b'a') as u32
        } else if (b'A'..=b'F').contains(&c) {
            10 + (c - b'A') as u32
        } else {
            continue;
        };
        unsafe {
            mul_u32_inplace(&mut b, 16);
            add_u32_inplace(&mut b, d);
        }
    }
    unsafe { normalize(b) };
    b
}

// ============================================================
// from_str — runtime `BigInt(<string>)` callable form. Auto-
// detects radix from the body's prefix; strips leading sign;
// returns 0n on parse errors (lenient subset of JS spec —
// SyntaxError matching is a follow-up alongside the test262 push).
// ============================================================

/// Helper — parse a single hex digit, returning None for non-hex bytes.
#[inline]
fn hex_digit(c: u8) -> Option<u32> {
    if (b'0'..=b'9').contains(&c) {
        Some((c - b'0') as u32)
    } else if (b'a'..=b'f').contains(&c) {
        Some(10 + (c - b'a') as u32)
    } else if (b'A'..=b'F').contains(&c) {
        Some(10 + (c - b'A') as u32)
    } else {
        None
    }
}

/// `BigInt(<runtime string value>)`. Reads the Str's len from
/// offset 8, body bytes from offset 16. Recognized prefixes
/// (with optional leading `+` / `-` sign):
/// - `0x` / `0X` → hex
/// - `0o` / `0O` → octal
/// - `0b` / `0B` → binary
/// - otherwise → decimal
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_from_str(s: *const c_void) -> *mut u8 {
    if s.is_null() {
        return unsafe { __torajs_bigint_from_decimal(core::ptr::null(), 0) };
    }
    let len = unsafe { *((s as *const u8).add(STR_LEN_OFF) as *const u64) };
    let bytes = unsafe { (s as *const u8).add(STR_HDR_SIZE) };

    // Strip a leading sign so radix prefixes that follow ("- 0x...")
    // are still recognized.
    let mut negative = 0u32;
    let mut off: u64 = 0;
    if len > 0 {
        let c = unsafe { *bytes };
        if c == b'-' {
            negative = 1;
            off = 1;
        } else if c == b'+' {
            off = 1;
        }
    }

    let mut r = unsafe { alloc_raw(0) };

    // Helper closure to consume the rest as the given radix's digits.
    // (Cannot factor across radixes cleanly because each prefix has
    // its own digit filter — mirror C's per-prefix loop.)
    let prefix_check = |c0: u8, c1_lower: u8, c1_upper: u8| -> bool {
        len - off >= 2 && unsafe { *bytes.add(off as usize) } == c0 && {
            let c = unsafe { *bytes.add(off as usize + 1) };
            c == c1_lower || c == c1_upper
        }
    };

    if prefix_check(b'0', b'x', b'X') {
        for i in (off + 2)..len {
            let c = unsafe { *bytes.add(i as usize) };
            if let Some(d) = hex_digit(c) {
                unsafe {
                    mul_u32_inplace(&mut r, 16);
                    add_u32_inplace(&mut r, d);
                }
            }
        }
    } else if prefix_check(b'0', b'o', b'O') {
        for i in (off + 2)..len {
            let c = unsafe { *bytes.add(i as usize) };
            if (b'0'..=b'7').contains(&c) {
                unsafe {
                    mul_u32_inplace(&mut r, 8);
                    add_u32_inplace(&mut r, (c - b'0') as u32);
                }
            }
        }
    } else if prefix_check(b'0', b'b', b'B') {
        for i in (off + 2)..len {
            let c = unsafe { *bytes.add(i as usize) };
            if c == b'0' || c == b'1' {
                unsafe {
                    mul_u32_inplace(&mut r, 2);
                    add_u32_inplace(&mut r, (c - b'0') as u32);
                }
            }
        }
    } else {
        for i in off..len {
            let c = unsafe { *bytes.add(i as usize) };
            if (b'0'..=b'9').contains(&c) {
                unsafe {
                    mul_u32_inplace(&mut r, 10);
                    add_u32_inplace(&mut r, (c - b'0') as u32);
                }
            }
        }
    }
    unsafe { normalize(r) };
    if negative == 1 && unsafe { read_len(r) } > 0 {
        unsafe { write_sign(r, 1) };
    }
    r
}

// ============================================================
// from_i64 — i64 → 1-limb BigInt with sign extraction. Handles
// INT64_MIN via unsigned arithmetic.
// ============================================================

/// `BigInt(<i64>)`. Returns a 0-limb BigInt for `v == 0`, otherwise
/// 1-limb with the magnitude.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_from_i64(v: i64) -> *mut u8 {
    if v == 0 {
        return unsafe { alloc_raw(0) };
    }
    let b = unsafe { alloc_raw(1) };
    let w = unsafe { words_mut(b) };
    if v < 0 {
        unsafe { write_sign(b, 1) };
        // INT64_MIN's magnitude doesn't fit in i64 — promote via
        // unsigned (matches C version: `(uint64_t)(-(v + 1)) + 1`).
        unsafe { *w = (-(v + 1)) as u64 + 1 };
    } else {
        unsafe { *w = v as u64 };
    }
    b
}

// ============================================================
// clone — fresh +1-rc copy of an existing BigInt. Caller's input
// lifetime unchanged (no rc transfer).
// ============================================================

/// `BigInt(<bigint>)` — clone.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_clone(a: *const c_void) -> *mut u8 {
    let a = a as *const u8;
    let len = unsafe { read_len(a) };
    let r = unsafe { alloc_raw(len) };
    if len > 0 {
        let src = unsafe { words_ptr(a) };
        let dst = unsafe { words_mut(r) };
        for i in 0..(len as usize) {
            unsafe { *dst.add(i) = *src.add(i) };
        }
    }
    let sign = unsafe { crate::internal::read_sign(a) };
    unsafe { write_sign(r, sign) };
    r
}

// ============================================================
// from_number — P3.3-b 推迟到 P3.3-i (依赖 mag_shl / mag_shr,
// now available). JS spec: reject non-finite + non-integer with
// RangeError. Conversion uses f64 mantissa-exponent split via Rust's
// `f64::to_bits` (avoids the libc frexp call that the C version used).
// ============================================================

/// `BigInt(<number>)` — V3-03. JS spec rejects non-finite + non-
/// integer Numbers with RangeError.
///
/// f64 → BigInt conversion:
/// - extract IEEE-754 mantissa (52 explicit bits + 1 implicit) and
///   biased exponent via `to_bits`
/// - reconstruct unbiased shift = exp - 1075 (mantissa is a 53-bit
///   integer when normalized; bias 1023 + 52 = 1075)
/// - shift the 53-bit mantissa left if shift > 0, right if < 0
///
/// # Safety
/// No memory hazards. Returns a fresh `+1`-rc BigInt heap pointer;
/// caller must eventually drop via `__torajs_bigint_drop_rc`. On
/// RangeError throw arm, returns NULL (ssa_lower's throw-check
/// diverts the continuation before the NULL is consumed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_from_number(v: f64) -> *mut u8 {
    unsafe {
        if !v.is_finite() || v.floor() != v {
            __torajs_throw_range_error(b"BigInt() expects a finite integer Number\0".as_ptr());
            return core::ptr::null_mut();
        }
        if v == 0.0 {
            return alloc_raw(0);
        }
        let negative = v < 0.0;
        let absv = if negative { -v } else { v };

        // Mantissa-exponent decomposition via raw bits.
        // f64 layout: 1 sign | 11 exp | 52 mantissa
        let bits = absv.to_bits();
        let raw_exp = ((bits >> 52) & 0x7ff) as i32;
        let raw_mantissa = bits & 0x000f_ffff_ffff_ffff;
        // Normal numbers: implicit leading 1 bit → mantissa = (1 << 52) | raw
        // Subnormals: raw_exp == 0; mantissa = raw (no implicit 1). For
        // integer-valued absv > 0 a subnormal is impossible (subnormals
        // are < 2^-1022), but handle it defensively to match the spec.
        let (m_int, e_unbiased): (u64, i32) = if raw_exp == 0 {
            (raw_mantissa, -1022)
        } else {
            ((1u64 << 52) | raw_mantissa, raw_exp - 1023)
        };
        // value = m_int * 2^(e_unbiased - 52); shift = e_unbiased - 52
        let shift: i32 = e_unbiased - 52;

        let mut r = alloc_raw(1);
        *words_mut(r) = m_int;
        normalize(r);

        if shift > 0 {
            let shifted = mag_shl(r, shift as u64);
            free(r as *mut c_void);
            r = shifted;
        } else if shift < 0 {
            // Right shift by -shift drops trailing zeros of m_int (the
            // mantissa already encoded the value's position; the trailing
            // bits are guaranteed zero for integer-valued Numbers).
            let shifted = mag_shr(r, (-shift) as u64);
            free(r as *mut c_void);
            r = shifted;
        }
        if negative && read_len(r) > 0 {
            write_sign(r, 1);
        }
        r
    }
}
