//! BigInt left / right shift.
//!
//! Port of `runtime_bigint.c` lines 393-537 (P3.3-i, 2026-05-23).
//!
//! Public extern fns:
//! - [`__torajs_bigint_shl`] — `a << n` for BigInt
//! - [`__torajs_bigint_shr`] — `a >> n` for BigInt
//!
//! Sign / negative-amount semantics:
//! - `a << -k` ≡ `a >> k` (mutual recursion via cross-tier extern)
//! - `a >> -k` ≡ `a << k`
//! - Positive `a >> n`: truncate (drop low bits)
//! - Negative `a >> n`: floor toward -∞, computed as
//!   `-(((|a| - 1) >> n) + 1)` — uses `crate::bitwise::{mag_dec1, mag_inc1}`
//! - Both `<<` shifts preserve sign (magnitude shifted, sign untouched
//!   unless result is zero)
//!
//! Range check: `n.len > 1` or `n.words[0] > i64::MAX` → `RangeError`
//! via cross-tier `__torajs_throw_range_error`. Bigger shifts would
//! blow memory before the throw-check diverts.
//!
//! P3.3-b's deferred `__torajs_bigint_from_number` ships in the same
//! commit (now that `mag_shl` / `mag_shr` are available) — it lives in
//! `construct.rs` and pulls `mag_shl`/`mag_shr` from here via
//! `pub(crate)`.

use core::ffi::c_void;

use crate::bitwise::{mag_dec1, mag_inc1};
use crate::internal::{alloc_raw, free, read_len, read_sign, words_mut, words_ptr, write_sign};

unsafe extern "C" {
    fn __torajs_throw_range_error(msg: *const u8);
}

// ============================================================
// Magnitude shift helpers (pub(crate) — also used by construct::from_number)
// ============================================================

/// Magnitude left shift by `n` bits. Caller bounds `n` upstream
/// (huge shifts blow memory; the wrapper extern enforces a sane cap
/// via `to_i64_for_shift`). Returns fresh `+1`-rc, sign 0.
pub(crate) unsafe fn mag_shl(a: *const u8, n: u64) -> *mut u8 {
    unsafe {
        let alen = read_len(a);
        if alen == 0 || n == 0 {
            let r = alloc_raw(alen);
            if alen > 0 {
                let src = words_ptr(a);
                let dst = words_mut(r);
                for i in 0..(alen as usize) {
                    *dst.add(i) = *src.add(i);
                }
            }
            return r;
        }
        let limb_shift = (n / 64) as u32;
        let bit_shift = (n % 64) as u32;
        let new_len = alen + limb_shift + 1;
        let r = alloc_raw(new_len);
        let aw = words_ptr(a);
        let rw = words_mut(r);
        if bit_shift == 0 {
            for i in 0..(alen as usize) {
                *rw.add(i + limb_shift as usize) = *aw.add(i);
            }
        } else {
            let mut carry: u64 = 0;
            for i in 0..(alen as usize) {
                let v = *aw.add(i);
                *rw.add(i + limb_shift as usize) = (v << bit_shift) | carry;
                carry = v >> (64 - bit_shift);
            }
            *rw.add(alen as usize + limb_shift as usize) = carry;
        }
        crate::internal::normalize(r);
        r
    }
}

/// Magnitude right shift by `n` bits (truncate). Returns fresh
/// `+1`-rc, sign 0.
pub(crate) unsafe fn mag_shr(a: *const u8, n: u64) -> *mut u8 {
    unsafe {
        let alen = read_len(a);
        let limb_shift = (n / 64) as u32;
        let bit_shift = (n % 64) as u32;
        if limb_shift >= alen {
            return alloc_raw(0);
        }
        let new_len = alen - limb_shift;
        let r = alloc_raw(new_len);
        let aw = words_ptr(a);
        let rw = words_mut(r);
        if bit_shift == 0 {
            for i in 0..(new_len as usize) {
                *rw.add(i) = *aw.add(i + limb_shift as usize);
            }
        } else {
            for i in 0..(new_len as usize) {
                let src_lo = i + limb_shift as usize;
                let lo = *aw.add(src_lo) >> bit_shift;
                let hi = if src_lo + 1 < alen as usize {
                    *aw.add(src_lo + 1) << (64 - bit_shift)
                } else {
                    0
                };
                *rw.add(i) = lo | hi;
            }
        }
        crate::internal::normalize(r);
        r
    }
}

// ============================================================
// Shift amount extraction (private)
// ============================================================

/// Extract `n` as a signed `i64` shift amount. Returns 0 (no-op shift)
/// after arming a RangeError if the amount is absurdly large — caller
/// bails before consuming the result; ssa_lower's throw-check diverts
/// the continuation.
unsafe fn to_i64_for_shift(n: *const u8) -> i64 {
    unsafe {
        let len = read_len(n);
        if len == 0 {
            return 0;
        }
        if len > 1 {
            __torajs_throw_range_error(b"BigInt shift amount too large\0".as_ptr());
            return 0;
        }
        let v = *words_ptr(n);
        if v > i64::MAX as u64 {
            __torajs_throw_range_error(b"BigInt shift amount too large\0".as_ptr());
            return 0;
        }
        let s = v as i64;
        if read_sign(n) != 0 { -s } else { s }
    }
}

// ============================================================
// extern "C" entry points
// ============================================================

/// `a << n` for BigInt. Negative `n` → routes to `>>`.
///
/// # Safety
/// `a_` and `n_` must be valid BigInt heap pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_shl(a_: *const c_void, n_: *const c_void) -> *mut u8 {
    let a = a_ as *const u8;
    let n = n_ as *const u8;
    unsafe {
        let shift = to_i64_for_shift(n);
        if shift == 0 {
            // Clone of a.
            let alen = read_len(a);
            let r = alloc_raw(alen);
            if alen > 0 {
                let src = words_ptr(a);
                let dst = words_mut(r);
                for i in 0..(alen as usize) {
                    *dst.add(i) = *src.add(i);
                }
            }
            write_sign(r, read_sign(a));
            return r;
        }
        if shift < 0 {
            // `a << -k` ≡ `a >> k`. Build a fresh positive |n| BigInt
            // to hand to shr (we don't mutate the caller's `n`).
            let nlen = read_len(n);
            let abs_n = alloc_raw(nlen);
            if nlen > 0 {
                let src = words_ptr(n);
                let dst = words_mut(abs_n);
                for i in 0..(nlen as usize) {
                    *dst.add(i) = *src.add(i);
                }
            }
            // abs_n has sign 0 already (alloc_raw zero-inits sign).
            let r = __torajs_bigint_shr(a_, abs_n as *const c_void);
            free(abs_n as *mut c_void);
            return r;
        }
        let r = mag_shl(a, shift as u64);
        write_sign(r, if read_len(r) == 0 { 0 } else { read_sign(a) });
        r
    }
}

/// `a >> n` for BigInt. Negative `n` → routes to `<<`. Negative `a`
/// uses floor-toward-negative-infinity semantics
/// (`-(((|a| - 1) >> n) + 1)`).
///
/// # Safety
/// Same as [`__torajs_bigint_shl`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_shr(a_: *const c_void, n_: *const c_void) -> *mut u8 {
    let a = a_ as *const u8;
    let n = n_ as *const u8;
    unsafe {
        let shift = to_i64_for_shift(n);
        if shift == 0 {
            let alen = read_len(a);
            let r = alloc_raw(alen);
            if alen > 0 {
                let src = words_ptr(a);
                let dst = words_mut(r);
                for i in 0..(alen as usize) {
                    *dst.add(i) = *src.add(i);
                }
            }
            write_sign(r, read_sign(a));
            return r;
        }
        if shift < 0 {
            let nlen = read_len(n);
            let abs_n = alloc_raw(nlen);
            if nlen > 0 {
                let src = words_ptr(n);
                let dst = words_mut(abs_n);
                for i in 0..(nlen as usize) {
                    *dst.add(i) = *src.add(i);
                }
            }
            let r = __torajs_bigint_shl(a_, abs_n as *const c_void);
            free(abs_n as *mut c_void);
            return r;
        }
        // Positive shift. Positive a → truncate; negative a → floor.
        if read_sign(a) == 0 {
            return mag_shr(a, shift as u64);
        }
        // Negative a: -(((|a| - 1) >> n) + 1)
        let am = mag_dec1(a);
        let shifted = mag_shr(am, shift as u64);
        free(am as *mut c_void);
        let r = mag_inc1(shifted);
        free(shifted as *mut c_void);
        if read_len(r) > 0 {
            write_sign(r, 1);
        }
        r
    }
}
