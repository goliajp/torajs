//! BigInt multiplication — schoolbook + Karatsuba.
//!
//! Port of `runtime_bigint.c` lines 253-391 (P3.3-d, 2026-05-23).
//!
//! Two-tier algorithm:
//! - **Schoolbook** (O(n²)) for small operands. Tight inner loop with
//!   `u128` carry; copies straight from the C version.
//! - **Karatsuba** (O(n^log₂3)) above [`KARATSUBA_THRESHOLD`]. Recurses
//!   via [`mag_mul`] dispatcher; identity:
//!   ```text
//!   x*y = z2 · B² + z1 · B + z0
//!       where z0 = xl·yl, z2 = xh·yh
//!             z1 = (xl + xh)(yl + yh) − z0 − z2
//!             B  = 2^(64·m), m = ⌈max(|x|,|y|)/2⌉
//!   ```
//!
//! All mag helpers operate on magnitudes only; sign is set by the
//! `__torajs_bigint_mul` dispatcher. Helpers are Rust-private (no C
//! ABI) — C-side `bigint_mag_mul_*` static fns remain for any future
//! cross-tier use, but currently only the extern wrapper goes through
//! the boundary.

use core::ffi::c_void;

use crate::arith::mag_add;
use crate::internal::{
    alloc_raw, free, normalize, read_len, read_sign, words_mut, words_ptr, write_sign,
};

/// Crossover from schoolbook to Karatsuba. Matches C's
/// `KARATSUBA_THRESHOLD` (V3-04 ship-1 era; documented in the C
/// source as observed-around-30-40-limbs on dev machines).
const KARATSUBA_THRESHOLD: u32 = 32;

// ============================================================
// Schoolbook
// ============================================================

/// O(n²) schoolbook magnitude multiplication.
unsafe fn mag_mul_schoolbook(a: *const u8, b: *const u8) -> *mut u8 {
    unsafe {
        let na = read_len(a);
        let nb = read_len(b);
        let r = alloc_raw(na + nb);
        let aw = words_ptr(a);
        let bw = words_ptr(b);
        let rw = words_mut(r);
        for i in 0..(na as usize) {
            let mut carry: u64 = 0;
            for j in 0..(nb as usize) {
                let cur = (*rw.add(i + j) as u128)
                    + (*aw.add(i) as u128) * (*bw.add(j) as u128)
                    + (carry as u128);
                *rw.add(i + j) = cur as u64;
                carry = (cur >> 64) as u64;
            }
            *rw.add(i + nb as usize) = (*rw.add(i + nb as usize)).wrapping_add(carry);
        }
        normalize(r);
        r
    }
}

// ============================================================
// Split / add-at / sub-in-place helpers (Karatsuba glue)
// ============================================================

/// Split `a` into low (limbs [0..m)) + high (limbs [m..len)).
/// Both outputs are fresh `+1`-rc magnitudes; if `a.len <= m` the
/// high half has len 0. Caller owns both pointers and must free.
unsafe fn mag_split_at(a: *const u8, m: u32) -> (*mut u8, *mut u8) {
    unsafe {
        let alen = read_len(a);
        let lo_len = alen.min(m);
        let hi_len = if alen > m { alen - m } else { 0 };

        let lo = alloc_raw(lo_len);
        if lo_len > 0 {
            let src = words_ptr(a);
            let dst = words_mut(lo);
            for i in 0..(lo_len as usize) {
                *dst.add(i) = *src.add(i);
            }
        }
        normalize(lo);

        let hi = alloc_raw(hi_len);
        if hi_len > 0 {
            let src = words_ptr(a);
            let dst = words_mut(hi);
            for i in 0..(hi_len as usize) {
                *dst.add(i) = *src.add(m as usize + i);
            }
        }
        normalize(hi);

        (lo, hi)
    }
}

/// Add `addend`'s limbs into `result` starting at limb-offset `off`,
/// propagating carry through result's high end. Caller MUST ensure
/// result has enough limbs allocated (worst-case product width).
unsafe fn mag_add_in_place_at(result: *mut u8, addend: *const u8, off: u32) {
    unsafe {
        let rlen = read_len(result) as usize;
        let alen = read_len(addend) as usize;
        let rw = words_mut(result);
        let aw = words_ptr(addend);
        let off = off as usize;
        let mut carry: u64 = 0;
        let mut i: usize = 0;
        while i < alen {
            let sum = (*rw.add(off + i) as u128) + (*aw.add(i) as u128) + (carry as u128);
            *rw.add(off + i) = sum as u64;
            carry = (sum >> 64) as u64;
            i += 1;
        }
        while carry != 0 && off + i < rlen {
            let sum = (*rw.add(off + i) as u128) + (carry as u128);
            *rw.add(off + i) = sum as u64;
            carry = (sum >> 64) as u64;
            i += 1;
        }
    }
}

/// Subtract `b`'s limbs from `result` in place. Caller MUST ensure
/// `result ≥ b` (Karatsuba's z1 = sum_prod − z0 − z2 guarantees this).
unsafe fn mag_sub_in_place(result: *mut u8, b: *const u8) {
    unsafe {
        let rlen = read_len(result) as usize;
        let blen = read_len(b) as usize;
        let rw = words_mut(result);
        let bw = words_ptr(b);
        let mut borrow: u64 = 0;
        let mut i: usize = 0;
        while i < blen {
            let diff = (*rw.add(i) as u128)
                .wrapping_sub(*bw.add(i) as u128)
                .wrapping_sub(borrow as u128);
            *rw.add(i) = diff as u64;
            borrow = ((diff >> 64) & 1) as u64;
            i += 1;
        }
        while borrow != 0 && i < rlen {
            let diff = (*rw.add(i) as u128).wrapping_sub(borrow as u128);
            *rw.add(i) = diff as u64;
            borrow = ((diff >> 64) & 1) as u64;
            i += 1;
        }
    }
}

// ============================================================
// Karatsuba recursion + dispatcher
// ============================================================

unsafe fn mag_mul_karatsuba(a: *const u8, b: *const u8) -> *mut u8 {
    unsafe {
        let na = read_len(a);
        let nb = read_len(b);
        let n = na.max(nb);
        let m = (n + 1) / 2;

        let (al, ah) = mag_split_at(a, m);
        let (bl, bh) = mag_split_at(b, m);

        let z0 = mag_mul(al, bl);
        let z2 = mag_mul(ah, bh);
        let sum_a = mag_add(al, ah);
        let sum_b = mag_add(bl, bh);
        let z1 = mag_mul(sum_a, sum_b);

        free(al as *mut c_void);
        free(ah as *mut c_void);
        free(bl as *mut c_void);
        free(bh as *mut c_void);
        free(sum_a as *mut c_void);
        free(sum_b as *mut c_void);

        mag_sub_in_place(z1, z0);
        mag_sub_in_place(z1, z2);
        normalize(z1);

        // Result width upper bound: a.len + b.len.
        let r = alloc_raw(na + nb);
        mag_add_in_place_at(r, z0, 0);
        mag_add_in_place_at(r, z1, m);
        mag_add_in_place_at(r, z2, 2 * m);

        free(z0 as *mut c_void);
        free(z1 as *mut c_void);
        free(z2 as *mut c_void);
        normalize(r);
        r
    }
}

/// Magnitude-only multiplication dispatcher. Picks schoolbook for
/// small operands, Karatsuba above the threshold.
pub(crate) unsafe fn mag_mul(a: *const u8, b: *const u8) -> *mut u8 {
    unsafe {
        let na = read_len(a);
        let nb = read_len(b);
        if na == 0 || nb == 0 {
            return alloc_raw(0);
        }
        let mn = na.min(nb);
        if mn < KARATSUBA_THRESHOLD {
            mag_mul_schoolbook(a, b)
        } else {
            mag_mul_karatsuba(a, b)
        }
    }
}

// ============================================================
// extern "C" entry point
// ============================================================

/// `a * b` for BigInt.
///
/// # Safety
/// `a_` and `b_` must be valid BigInt heap block pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_mul(a_: *const c_void, b_: *const c_void) -> *mut u8 {
    let a = a_ as *const u8;
    let b = b_ as *const u8;
    unsafe {
        let r = mag_mul(a, b);
        let a_sign = read_sign(a);
        let b_sign = read_sign(b);
        let r_sign = if (a_sign ^ b_sign) != 0 { 1 } else { 0 };
        write_sign(r, r_sign);
        if read_len(r) == 0 {
            write_sign(r, 0);
        }
        normalize(r);
        r
    }
}
