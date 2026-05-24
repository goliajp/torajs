//! Criterion bench for `__torajs_value_drop_heap`. We can't fully
//! exercise the per-tag dispatch arms because they call into
//! workspace-internal staticlibs (libtorajs_str.a etc.) that
//! aren't linked when running `cargo bench -p torajs-value-drop`
//! standalone. What we CAN measure is the null-input fast-path
//! cost — that's the cbnz + ret pair every caller hits when their
//! input happens to be NULL (e.g. an empty Array<Any> slot).

use core::hint::black_box;
use criterion::{Criterion, criterion_group, criterion_main};

use torajs_value_drop::__torajs_value_drop_heap;

fn bench_null_input(c: &mut Criterion) {
    c.bench_function("value_drop_heap-null-100k", |b| {
        b.iter(|| {
            for _ in 0..100_000 {
                unsafe { __torajs_value_drop_heap(black_box(core::ptr::null_mut())) };
            }
        });
    });
}

criterion_group!(benches, bench_null_input);
criterion_main!(benches);
