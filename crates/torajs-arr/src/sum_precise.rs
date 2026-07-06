//! `Math.sumPrecise(arr)` per ES2025 §21.3.2.32 — correctly-rounded
//! sum of an Array<Number>. The result is the f64 closest (with ties
//! broken to even) to the real-valued sum of the inputs treated as
//! exact rationals; intermediate rounding is avoided.
//!
//! The implementation is Shewchuk's online expansion-sum (`grow_expansion`
//! / `expansion_sum`) followed by Python's `_msum_fsum` correctly-rounded
//! tail. Spec edge cases (NaN, ±Inf, ±0, empty) are filtered out before
//! the expansion-sum loop.
//!
//! Reference: Shewchuk, "Adaptive Precision Floating-Point Arithmetic and
//! Fast Robust Geometric Predicates" (1996); Python `Modules/mathmodule.c
//! :math_fsum_impl`.
//!
//! ## Spec edge-case behavior (bun parity verified)
//! - NaN anywhere → NaN
//! - +Inf and -Inf both present → NaN
//! - only +Inf present → +Inf
//! - only -Inf present → -Inf
//! - empty array → -0.0
//! - single -0.0 → -0.0; single +0.0 → +0.0; sum-to-zero → +0.0

use core::ffi::c_void;

use crate::layout::{ARR_LEN_OFF, arr_data};

// Mirrors the deque-aware `head` u32 field that lives in the cap
// slot's upper half (matches `join.rs` / `print.rs`).
const ARR_HEAD_OFF: usize = 20;

#[inline]
unsafe fn arr_len(arr: *const u8) -> u64 {
    unsafe { *(arr.add(ARR_LEN_OFF) as *const u64) }
}

#[inline]
unsafe fn arr_head(arr: *const u8) -> u32 {
    unsafe { *(arr.add(ARR_HEAD_OFF) as *const u32) }
}

#[inline]
unsafe fn slot_f64(arr: *const u8, head: u32, i: u64) -> f64 {
    unsafe {
        let p = arr_data(arr).add((head as usize + i as usize) * 8);
        *(p as *const f64)
    }
}

#[inline]
unsafe fn slot_i64(arr: *const u8, head: u32, i: u64) -> i64 {
    unsafe {
        let p = arr_data(arr).add((head as usize + i as usize) * 8);
        *(p as *const i64)
    }
}

fn sum_precise_inner<I: Iterator<Item = f64>>(items: I) -> f64 {
    let mut partials: Vec<f64> = Vec::with_capacity(16);
    let mut has_nan = false;
    let mut has_pos_inf = false;
    let mut has_neg_inf = false;

    for x in items {
        if x.is_nan() {
            has_nan = true;
            continue;
        }
        if x == f64::INFINITY {
            has_pos_inf = true;
            continue;
        }
        if x == f64::NEG_INFINITY {
            has_neg_inf = true;
            continue;
        }

        // Shewchuk's online `grow_expansion` — fold `x` into the
        // running non-overlapping partial expansion.
        let mut x = x;
        let mut idx = 0usize;
        for j in 0..partials.len() {
            let mut y = partials[j];
            if x.abs() < y.abs() {
                core::mem::swap(&mut x, &mut y);
            }
            let hi = x + y;
            let lo = y - (hi - x);
            if lo != 0.0 {
                partials[idx] = lo;
                idx += 1;
            }
            x = hi;
        }
        partials.truncate(idx);
        partials.push(x);
    }

    if has_nan {
        return f64::NAN;
    }
    match (has_pos_inf, has_neg_inf) {
        (true, true) => return f64::NAN,
        (true, false) => return f64::INFINITY,
        (false, true) => return f64::NEG_INFINITY,
        (false, false) => {}
    }

    if partials.is_empty() {
        // Inputs all canceled exactly to zero — spec yields +0.0.
        return 0.0;
    }

    // Python `math_fsum_impl` correctly-rounded tail (BSD-licensed
    // reference). Iterates from high → low summing pairwise and uses
    // the parity of the discarded bits to break ties.
    let n = partials.len();
    let mut hi = partials[n - 1];
    let mut lo = 0.0f64;
    let mut i = n - 1;
    while i > 0 {
        i -= 1;
        let x = hi;
        let y = partials[i];
        hi = x + y;
        lo = y - (hi - x);
        if lo != 0.0 {
            break;
        }
    }
    // Tie-break: if the next-lower partial has the same sign as `lo`,
    // round half-to-even via the doubled-lo trick (cf. Python fsum).
    if i > 0 && ((lo < 0.0 && partials[i - 1] < 0.0) || (lo > 0.0 && partials[i - 1] > 0.0)) {
        let y = lo * 2.0;
        let x = hi + y;
        if y == x - hi {
            hi = x;
        }
    }
    hi
}

/// `Math.sumPrecise(arr)` for an Array<F64> — correctly-rounded sum.
///
/// # Safety
/// `arr` must be null or a live Array<f64> heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_sum_precise(arr: *const c_void) -> f64 {
    if arr.is_null() {
        return -0.0;
    }
    let arr = arr as *const u8;
    let len = unsafe { arr_len(arr) };
    if len == 0 {
        return -0.0;
    }
    let head = unsafe { arr_head(arr) };
    sum_precise_inner((0..len).map(|k| unsafe { slot_f64(arr, head, k) }))
}

/// `Math.sumPrecise(arr)` for an Array<I64> — integer slots widen to
/// f64 before entering the expansion-sum loop. Within ±2^53 every
/// step is exact; beyond that, Shewchuk handles the conversion
/// rounding the same way it handles input f64 rounding.
///
/// # Safety
/// `arr` must be null or a live Array<i64> heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_sum_precise_i64(arr: *const c_void) -> f64 {
    if arr.is_null() {
        return -0.0;
    }
    let arr = arr as *const u8;
    let len = unsafe { arr_len(arr) };
    if len == 0 {
        return -0.0;
    }
    let head = unsafe { arr_head(arr) };
    sum_precise_inner((0..len).map(|k| unsafe { slot_i64(arr, head, k) } as f64))
}
