//! Performance regression gates for `torajs-bigint`. See [BUDGETS.md].
//!
//! BigInt isn't on any bench-corpus hot loop today, but the
//! BUDGETS.md targets are well-documented; codify them as CI gates
//! with 5× headroom so a future polish that accidentally adds a
//! per-op alloc or panic check fails.
//!
//! Run with `cargo test -p torajs-bigint --test perf_gate --release`.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use torajs_bigint::{
    __torajs_bigint_add, __torajs_bigint_drop, __torajs_bigint_from_i64, __torajs_bigint_mul,
};

fn time_median<F: FnMut()>(mut op: F, samples: usize) -> Duration {
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        op();
        times.push(start.elapsed());
    }
    times.sort();
    times[samples / 2]
}

fn from_i64(v: i64) -> *mut u8 {
    unsafe { __torajs_bigint_from_i64(v) }
}

fn build_multi_limb() -> *mut u8 {
    // Build ~10^32 via repeated squaring of 12345.
    let base = from_i64(12_345);
    let x = unsafe { __torajs_bigint_mul(base as *const c_void, base as *const c_void) };
    let x2 = unsafe { __torajs_bigint_mul(x as *const c_void, x as *const c_void) };
    let x3 = unsafe { __torajs_bigint_mul(x2 as *const c_void, x2 as *const c_void) };
    unsafe {
        __torajs_bigint_drop(base as *mut c_void);
        __torajs_bigint_drop(x as *mut c_void);
        __torajs_bigint_drop(x2 as *mut c_void);
    }
    x3
}

#[test]
fn add_i64_10k_under_budget() {
    // BUDGETS.md target: 10k i64-add ≤ 5 ms (~500 ns/add — alloc-dom).
    // CI budget: 25 ms.
    let a = from_i64(12_345);
    let b = from_i64(67_890);
    let median = time_median(
        || {
            for _ in 0..10_000 {
                let sum = unsafe { __torajs_bigint_add(a as *const c_void, b as *const c_void) };
                unsafe { __torajs_bigint_drop(sum as *mut c_void) };
            }
        },
        11,
    );
    let budget = Duration::from_millis(25);
    assert!(
        median < budget,
        "bigint_add i64 regressed: median {median:?} >= budget {budget:?}"
    );
    unsafe {
        __torajs_bigint_drop(a as *mut c_void);
        __torajs_bigint_drop(b as *mut c_void);
    }
}

#[test]
fn mul_multi_limb_1k_under_budget() {
    // BUDGETS.md target: 1k multi-limb mul ≤ 5 ms.
    // CI budget: 50 ms.
    let a = build_multi_limb();
    let b = build_multi_limb();
    let median = time_median(
        || {
            for _ in 0..1_000 {
                let prod = unsafe { __torajs_bigint_mul(a as *const c_void, b as *const c_void) };
                unsafe { __torajs_bigint_drop(prod as *mut c_void) };
            }
        },
        11,
    );
    let budget = Duration::from_millis(50);
    assert!(
        median < budget,
        "bigint_mul multi-limb regressed: median {median:?} >= budget {budget:?}"
    );
    unsafe {
        __torajs_bigint_drop(a as *mut c_void);
        __torajs_bigint_drop(b as *mut c_void);
    }
}
