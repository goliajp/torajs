//! BigInt addition / subtraction.
//!
//! Port of `runtime_bigint.c` lines 175-269 (P3.3-c, 2026-05-23).
//!
//! Layout-aware sign-magnitude arithmetic:
//! - `mag_cmp(a, b)` — compare magnitudes, returns -1 / 0 / 1
//! - `mag_add(a, b)` — magnitude addition (no sign awareness)
//! - `mag_sub(a, b)` — magnitude subtraction; precondition `|a| ≥ |b|`
//! - [`__torajs_bigint_add`] / [`__torajs_bigint_sub`] — signed add / sub
//!   that dispatch into the mag_* helpers based on sign agreement
//!
//! The mag helpers are Rust-private (no C ABI) so they don't collide
//! with C-side `bigint_mag_{cmp,add,sub}` still used by the not-yet-
//! ported cmp / eq / mul / etc fns in `runtime_bigint.c`.

use core::ffi::c_void;

use crate::internal::{
    alloc_raw, normalize, read_len, read_sign, words_mut, words_ptr, write_sign,
};

// ============================================================
// Magnitude primitives — private to this crate.
// ============================================================

/// Compare magnitudes only. Returns -1 / 0 / 1.
pub(crate) unsafe fn mag_cmp(a: *const u8, b: *const u8) -> i32 {
    unsafe {
        let na = read_len(a) as usize;
        let nb = read_len(b) as usize;
        if na != nb {
            return if na < nb { -1 } else { 1 };
        }
        let aw = words_ptr(a);
        let bw = words_ptr(b);
        let mut i = na as isize - 1;
        while i >= 0 {
            let av = *aw.add(i as usize);
            let bv = *bw.add(i as usize);
            if av != bv {
                return if av < bv { -1 } else { 1 };
            }
            i -= 1;
        }
        0
    }
}

/// Magnitude addition. Returns fresh `+1`-rc BigInt with sign=0.
/// Caller is responsible for setting the final sign.
pub(crate) unsafe fn mag_add(a: *const u8, b: *const u8) -> *mut u8 {
    unsafe {
        let na = read_len(a) as usize;
        let nb = read_len(b) as usize;
        let n = na.max(nb);
        let out = alloc_raw((n + 1) as u32);
        let ow = words_mut(out);
        let aw = words_ptr(a);
        let bw = words_ptr(b);
        let mut carry: u64 = 0;
        for i in 0..n {
            let av = if i < na { *aw.add(i) } else { 0 };
            let bv = if i < nb { *bw.add(i) } else { 0 };
            let sum = (av as u128) + (bv as u128) + (carry as u128);
            *ow.add(i) = sum as u64;
            carry = (sum >> 64) as u64;
        }
        *ow.add(n) = carry;
        normalize(out);
        out
    }
}

/// Magnitude subtraction. Caller MUST ensure `|a| ≥ |b|`.
/// Returns fresh `+1`-rc BigInt with sign=0.
pub(crate) unsafe fn mag_sub(a: *const u8, b: *const u8) -> *mut u8 {
    unsafe {
        let na = read_len(a) as usize;
        let nb = read_len(b) as usize;
        let out = alloc_raw(na as u32);
        let ow = words_mut(out);
        let aw = words_ptr(a);
        let bw = words_ptr(b);
        let mut borrow: u64 = 0;
        for i in 0..na {
            let av = *aw.add(i);
            let bv = if i < nb { *bw.add(i) } else { 0 };
            // u128 wrapping mirrors C's `unsigned __int128` underflow trick:
            // high bit of (av - bv - borrow) in u128 indicates the borrow.
            let diff = (av as u128)
                .wrapping_sub(bv as u128)
                .wrapping_sub(borrow as u128);
            *ow.add(i) = diff as u64;
            borrow = ((diff >> 64) & 1) as u64;
        }
        normalize(out);
        out
    }
}

// ============================================================
// extern "C" entry points
// ============================================================

/// `a + b` for BigInt.
///
/// # Safety
/// `a_` and `b_` must be valid BigInt heap block pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_add(a_: *const c_void, b_: *const c_void) -> *mut u8 {
    let a = a_ as *const u8;
    let b = b_ as *const u8;
    unsafe {
        let a_sign = read_sign(a);
        let b_sign = read_sign(b);
        let r;
        if a_sign == b_sign {
            r = mag_add(a, b);
            write_sign(r, a_sign);
        } else {
            let c = mag_cmp(a, b);
            if c == 0 {
                r = alloc_raw(0);
            } else if c > 0 {
                r = mag_sub(a, b);
                write_sign(r, a_sign);
            } else {
                r = mag_sub(b, a);
                write_sign(r, b_sign);
            }
        }
        normalize(r);
        r
    }
}

/// `a - b` for BigInt.
///
/// # Safety
/// `a_` and `b_` must be valid BigInt heap block pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_sub(a_: *const c_void, b_: *const c_void) -> *mut u8 {
    let a = a_ as *const u8;
    let b = b_ as *const u8;
    unsafe {
        let a_sign = read_sign(a);
        let b_sign = read_sign(b);
        let r;
        if a_sign != b_sign {
            r = mag_add(a, b);
            write_sign(r, a_sign);
        } else {
            let c = mag_cmp(a, b);
            if c == 0 {
                r = alloc_raw(0);
            } else if c > 0 {
                r = mag_sub(a, b);
                write_sign(r, a_sign);
            } else {
                r = mag_sub(b, a);
                write_sign(r, if a_sign == 0 { 1 } else { 0 });
            }
        }
        normalize(r);
        r
    }
}
