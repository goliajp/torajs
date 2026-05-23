//! BigInt bitwise ops with two's-complement semantics.
//!
//! Port of `runtime_bigint.c` lines 355-540 (P3.3-h, 2026-05-23).
//!
//! Spec model: a BigInt's bit representation is its two's-complement
//! form in an *infinite* bit-width register. Positive x has finite-
//! magnitude bits + infinite zeros above; negative x has `~|x| + 1`
//! finite bits + infinite ones above.
//!
//! Implementation trick: encode the finite bit-pattern of a negative
//! x as `|x| - 1`. Then the "abstract top bit" is 1, equivalent to
//! `~mag` in our chosen finite encoding. After the bit op, if the
//! abstract top bit is 1, result is negative with magnitude
//! `result_mag + 1` (inverse of the identity).
//!
//! Per-op sign cases:
//!
//! ```text
//! AND  ++ : pos, mag = a AND b
//!      +- : pos, mag = pos AND_NOT (|neg|-1)
//!      -- : neg, mag = ((|a|-1) OR (|b|-1)) + 1
//!
//! OR   ++ : pos, mag = a OR b
//!      +- : neg, mag = ((|neg|-1) AND_NOT pos) + 1
//!      -- : neg, mag = ((|a|-1) AND (|b|-1)) + 1
//!
//! XOR  ++ : pos, mag = a XOR b
//!      +- : neg, mag = (pos XOR (|neg|-1)) + 1
//!      -- : pos, mag = (|a|-1) XOR (|b|-1)
//!
//! NOT  ~x ≡ -(x + 1n)   (universal identity)
//! ```

use core::ffi::c_void;

use crate::internal::{
    alloc_raw, free, normalize, read_len, read_sign, words_mut, words_ptr, write_sign,
};

// ============================================================
// Magnitude bit-level helpers (private)
// ============================================================

/// `a AND b` over magnitudes. Result is truncated to min(a.len, b.len)
/// since high bits of the shorter side are zero — ANDing with zero
/// yields zero.
unsafe fn mag_and(a: *const u8, b: *const u8) -> *mut u8 {
    unsafe {
        let na = read_len(a);
        let nb = read_len(b);
        let n = na.min(nb);
        let r = alloc_raw(n);
        let aw = words_ptr(a);
        let bw = words_ptr(b);
        let rw = words_mut(r);
        for i in 0..(n as usize) {
            *rw.add(i) = *aw.add(i) & *bw.add(i);
        }
        normalize(r);
        r
    }
}

/// `a OR b` over magnitudes. Result has max(a.len, b.len) limbs since
/// the high bits of the longer side are preserved.
unsafe fn mag_or(a: *const u8, b: *const u8) -> *mut u8 {
    unsafe {
        let na = read_len(a) as usize;
        let nb = read_len(b) as usize;
        let n = na.max(nb);
        let r = alloc_raw(n as u32);
        let aw = words_ptr(a);
        let bw = words_ptr(b);
        let rw = words_mut(r);
        for i in 0..n {
            let av = if i < na { *aw.add(i) } else { 0 };
            let bv = if i < nb { *bw.add(i) } else { 0 };
            *rw.add(i) = av | bv;
        }
        normalize(r);
        r
    }
}

/// `a XOR b` over magnitudes. Result has max(a.len, b.len) limbs.
unsafe fn mag_xor(a: *const u8, b: *const u8) -> *mut u8 {
    unsafe {
        let na = read_len(a) as usize;
        let nb = read_len(b) as usize;
        let n = na.max(nb);
        let r = alloc_raw(n as u32);
        let aw = words_ptr(a);
        let bw = words_ptr(b);
        let rw = words_mut(r);
        for i in 0..n {
            let av = if i < na { *aw.add(i) } else { 0 };
            let bv = if i < nb { *bw.add(i) } else { 0 };
            *rw.add(i) = av ^ bv;
        }
        normalize(r);
        r
    }
}

/// `a AND_NOT b` (= `a & ~b`) over magnitudes. Result is at most a's
/// width since high bits of `b` only zero out (already-zero) high bits
/// of `a`.
unsafe fn mag_andnot(a: *const u8, b: *const u8) -> *mut u8 {
    unsafe {
        let na = read_len(a);
        let nb = read_len(b) as usize;
        let r = alloc_raw(na);
        let aw = words_ptr(a);
        let bw = words_ptr(b);
        let rw = words_mut(r);
        for i in 0..(na as usize) {
            let bv = if i < nb { *bw.add(i) } else { 0 };
            *rw.add(i) = *aw.add(i) & !bv;
        }
        normalize(r);
        r
    }
}

/// `|x| + 1` over magnitudes. Used to round-trip the two's-complement
/// trick (negative_x ↔ mag = |x| - 1). pub(crate) — `crate::shift`'s
/// negative-arithmetic-shift-right floor path reuses it.
pub(crate) unsafe fn mag_inc1(a: *const u8) -> *mut u8 {
    unsafe {
        let na = read_len(a) as usize;
        let r = alloc_raw((na + 1) as u32);
        let aw = words_ptr(a);
        let rw = words_mut(r);
        let mut carry: u64 = 1;
        for i in 0..na {
            let sum = (*aw.add(i) as u128) + (carry as u128);
            *rw.add(i) = sum as u64;
            carry = (sum >> 64) as u64;
        }
        *rw.add(na) = carry;
        normalize(r);
        r
    }
}

/// `|x| - 1` over magnitudes. Pre-condition: `|x| >= 1`. Caller is
/// either the bitwise `+-` / `--` sign-case dispatcher (negative
/// operand always `len > 0`) or `crate::shift`'s `shr` floor-toward-
/// negative-infinity path.
pub(crate) unsafe fn mag_dec1(a: *const u8) -> *mut u8 {
    unsafe {
        let na = read_len(a);
        let r = alloc_raw(na);
        let aw = words_ptr(a);
        let rw = words_mut(r);
        let mut borrow: u64 = 1;
        for i in 0..(na as usize) {
            let diff = (*aw.add(i) as u128).wrapping_sub(borrow as u128);
            *rw.add(i) = diff as u64;
            borrow = ((diff >> 64) & 1) as u64;
        }
        normalize(r);
        r
    }
}

// ============================================================
// extern "C" entry points
// ============================================================

unsafe extern "C" {
    /// Cross-tier — torajs-bigint::arith. NOT used here directly anymore
    /// (negation via mag-level inc1 / dec1 + sign stamp), but declared
    /// for `__torajs_bigint_not` which uses the `-a - 1n` identity via
    /// add + neg.
    fn __torajs_bigint_add(a: *const c_void, b: *const c_void) -> *mut u8;
    fn __torajs_bigint_neg(a: *const c_void) -> *mut u8;
}

/// `~a` ≡ `-(a + 1n)` — universal identity, no sign-case dispatch
/// needed. Costs one tiny intermediate 1n alloc + the existing add /
/// neg externs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_not(a_: *const c_void) -> *mut u8 {
    unsafe {
        // Build 1n on the fly.
        let one = alloc_raw(1);
        *words_mut(one) = 1;
        let plus_one = __torajs_bigint_add(a_, one as *const c_void);
        free(one as *mut c_void);
        let r = __torajs_bigint_neg(plus_one as *const c_void);
        free(plus_one as *mut c_void);
        r
    }
}

/// `a & b` with two's-complement semantics.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_and(a_: *const c_void, b_: *const c_void) -> *mut u8 {
    let a = a_ as *const u8;
    let b = b_ as *const u8;
    unsafe {
        let a_sign = read_sign(a);
        let b_sign = read_sign(b);
        let r;
        if a_sign == 0 && b_sign == 0 {
            r = mag_and(a, b);
        } else if a_sign != 0 && b_sign != 0 {
            // -- : mag = ((|a|-1) OR (|b|-1)) + 1, sign = 1
            let am = mag_dec1(a);
            let bm = mag_dec1(b);
            let or_ = mag_or(am, bm);
            free(am as *mut c_void);
            free(bm as *mut c_void);
            r = mag_inc1(or_);
            free(or_ as *mut c_void);
            if read_len(r) > 0 {
                write_sign(r, 1);
            }
        } else {
            // +- : mag = pos AND_NOT (|neg| - 1), sign = 0
            let (p, n) = if a_sign != 0 { (b, a) } else { (a, b) };
            let nm = mag_dec1(n);
            r = mag_andnot(p, nm);
            free(nm as *mut c_void);
        }
        normalize(r);
        r
    }
}

/// `a | b` with two's-complement semantics.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_or(a_: *const c_void, b_: *const c_void) -> *mut u8 {
    let a = a_ as *const u8;
    let b = b_ as *const u8;
    unsafe {
        let a_sign = read_sign(a);
        let b_sign = read_sign(b);
        let r;
        if a_sign == 0 && b_sign == 0 {
            r = mag_or(a, b);
        } else if a_sign != 0 && b_sign != 0 {
            // -- : mag = ((|a|-1) AND (|b|-1)) + 1, sign = 1
            let am = mag_dec1(a);
            let bm = mag_dec1(b);
            let and_ = mag_and(am, bm);
            free(am as *mut c_void);
            free(bm as *mut c_void);
            r = mag_inc1(and_);
            free(and_ as *mut c_void);
            if read_len(r) > 0 {
                write_sign(r, 1);
            }
        } else {
            // +- : mag = ((|neg|-1) AND_NOT pos) + 1, sign = 1
            let (p, n) = if a_sign != 0 { (b, a) } else { (a, b) };
            let nm = mag_dec1(n);
            let andnot = mag_andnot(nm, p);
            free(nm as *mut c_void);
            r = mag_inc1(andnot);
            free(andnot as *mut c_void);
            if read_len(r) > 0 {
                write_sign(r, 1);
            }
        }
        normalize(r);
        r
    }
}

/// `a ^ b` with two's-complement semantics.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_xor(a_: *const c_void, b_: *const c_void) -> *mut u8 {
    let a = a_ as *const u8;
    let b = b_ as *const u8;
    unsafe {
        let a_sign = read_sign(a);
        let b_sign = read_sign(b);
        let r;
        if a_sign == 0 && b_sign == 0 {
            r = mag_xor(a, b);
        } else if a_sign != 0 && b_sign != 0 {
            // -- : mag = (|a|-1) XOR (|b|-1), sign = 0
            let am = mag_dec1(a);
            let bm = mag_dec1(b);
            r = mag_xor(am, bm);
            free(am as *mut c_void);
            free(bm as *mut c_void);
        } else {
            // +- : mag = (pos XOR (|neg|-1)) + 1, sign = 1
            let (p, n) = if a_sign != 0 { (b, a) } else { (a, b) };
            let nm = mag_dec1(n);
            let xor_ = mag_xor(p, nm);
            free(nm as *mut c_void);
            r = mag_inc1(xor_);
            free(xor_ as *mut c_void);
            if read_len(r) > 0 {
                write_sign(r, 1);
            }
        }
        normalize(r);
        r
    }
}
