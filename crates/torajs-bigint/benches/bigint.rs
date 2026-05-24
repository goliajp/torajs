//! Criterion benches for `torajs-bigint` on representative
//! workloads: 64-bit arith (i64 inputs) + larger multiplications
//! built up via repeated squaring.
//!
//! Use the rlib's public re-exports; bench crate can't allocate
//! Str heap-blocks so we synthesize "large" numbers via repeated
//! mul chains.

use core::hint::black_box;
use criterion::{Criterion, criterion_group, criterion_main};
use std::ffi::c_void;

use torajs_bigint::{
    __torajs_bigint_add, __torajs_bigint_drop, __torajs_bigint_from_i64, __torajs_bigint_mul,
};

fn from_i64(v: i64) -> *mut u8 {
    unsafe { __torajs_bigint_from_i64(v) }
}

/// Build a ~2^256-shaped value by repeated squaring of a non-trivial
/// base (e.g. 12345).
fn build_large() -> *mut u8 {
    let base = from_i64(12_345);
    // x = base^2 ≈ 1.5e8
    let x = unsafe { __torajs_bigint_mul(base as *const c_void, base as *const c_void) };
    // x = (base^2)^2 ≈ 2.3e16
    let x2 = unsafe { __torajs_bigint_mul(x as *const c_void, x as *const c_void) };
    // x = ((base^2)^2)^2 ≈ 5.4e32 (well into multi-limb)
    let x3 = unsafe { __torajs_bigint_mul(x2 as *const c_void, x2 as *const c_void) };
    unsafe {
        __torajs_bigint_drop(base as *mut c_void);
        __torajs_bigint_drop(x as *mut c_void);
        __torajs_bigint_drop(x2 as *mut c_void);
    }
    x3
}

fn bench_add_small(c: &mut Criterion) {
    let a = from_i64(12345);
    let b = from_i64(67890);
    c.bench_function("bigint_add-i64-10k", |b_| {
        b_.iter(|| {
            for _ in 0..10_000 {
                let sum = unsafe {
                    __torajs_bigint_add(
                        black_box(a) as *const c_void,
                        black_box(b) as *const c_void,
                    )
                };
                unsafe { __torajs_bigint_drop(sum as *mut c_void) };
            }
        });
    });
    unsafe {
        __torajs_bigint_drop(a as *mut c_void);
        __torajs_bigint_drop(b as *mut c_void);
    }
}

fn bench_mul_multi_limb(c: &mut Criterion) {
    let a = build_large();
    let b = build_large();
    c.bench_function("bigint_mul-multi-limb-1k", |b_| {
        b_.iter(|| {
            for _ in 0..1_000 {
                let prod = unsafe {
                    __torajs_bigint_mul(
                        black_box(a) as *const c_void,
                        black_box(b) as *const c_void,
                    )
                };
                unsafe { __torajs_bigint_drop(prod as *mut c_void) };
            }
        });
    });
    unsafe {
        __torajs_bigint_drop(a as *mut c_void);
        __torajs_bigint_drop(b as *mut c_void);
    }
}

criterion_group!(benches, bench_add_small, bench_mul_multi_limb);
criterion_main!(benches);
