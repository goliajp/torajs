//! Criterion benches for `torajs-throw`.
//!
//! The hot path is `__torajs_throw_check` (the IR-emitted poll after
//! every "may throw" call). Every runtime helper call site costs one
//! load + cbnz; we measure throughput on the happy path (slot is
//! empty, no throw in flight) since the cold path is by definition
//! rare and not perf-sensitive.

use core::hint::black_box;
use criterion::{Criterion, criterion_group, criterion_main};

use torajs_throw::{
    __torajs_throw_check, __torajs_throw_set, __torajs_throw_take, __torajs_throw_take_tag,
};

fn bench_check_happy_path(c: &mut Criterion) {
    // Make sure slot is clear.
    unsafe {
        __torajs_throw_set(0, 0);
        let _ = __torajs_throw_take();
    }
    c.bench_function("throw_check-happy-path-100k", |b| {
        b.iter(|| {
            let mut acc = 0i64;
            for _ in 0..100_000 {
                acc = acc.wrapping_add(black_box(unsafe { __torajs_throw_check() }));
            }
            acc
        });
    });
}

fn bench_set_take_cycle(c: &mut Criterion) {
    c.bench_function("throw_set_take_cycle-100k", |b| {
        b.iter(|| {
            for i in 0..100_000i64 {
                unsafe {
                    __torajs_throw_set(black_box(i), black_box(i.wrapping_mul(7)));
                    let _ = black_box(__torajs_throw_take_tag());
                    let _ = black_box(__torajs_throw_take());
                }
            }
        });
    });
}

criterion_group!(benches, bench_check_happy_path, bench_set_take_cycle);
criterion_main!(benches);
