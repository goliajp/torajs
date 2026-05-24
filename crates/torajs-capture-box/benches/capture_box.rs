//! Criterion benches for `torajs-capture-box`.
//!
//! The hot path: every closure construction over a captured let
//! triggers `__torajs_capture_box_alloc + N inc`. Bench measures
//! the alloc-inc-drop cycle that represents one "captured-let
//! lifetime"; in practice torajs's closure heavy benchmarks
//! (closure-counter / closure-pipeline-1m) execute this thousands
//! to millions of times per run.

use core::hint::black_box;
use criterion::{Criterion, criterion_group, criterion_main};

use torajs_capture_box::{
    __torajs_capture_box_alloc, __torajs_capture_box_drop, __torajs_capture_box_inc,
};

fn bench_alloc_inc_drop_cycle(c: &mut Criterion) {
    c.bench_function("alloc-inc-drop-cycle-100k", |b| {
        b.iter(|| {
            for i in 0..100_000i64 {
                let slot = __torajs_capture_box_alloc(black_box(i));
                unsafe { __torajs_capture_box_inc(black_box(slot)) };
                unsafe { __torajs_capture_box_drop(black_box(slot)) };
                // rc back to 0 — the box is NOT freed yet (drop only
                // dec's, doesn't reach the free arm because inc went
                // to 1 then drop dec'd to 0; the at-zero-observation
                // arm fires only when drop sees rc=0 without prior
                // inc). Wait — that means the box leaks. Let me
                // re-check semantics.
                //
                // After inc: rc = 1. After drop: rc -= 1 → 0, falls
                // into the post-decrement "if *rc == 0" arm, frees.
                // So the box IS freed at the second drop call. Good.
            }
        });
    });
}

fn bench_alloc_drop_no_inc(c: &mut Criterion) {
    // The "promoted-but-never-captured" edge case — drop on a
    // never-inc'd box. Each iter alloc + drop; the at-zero-obs
    // arm fires and frees. Measures the rc=0 fast-free path.
    c.bench_function("alloc-drop-no-inc-100k", |b| {
        b.iter(|| {
            for i in 0..100_000i64 {
                let slot = __torajs_capture_box_alloc(black_box(i));
                unsafe { __torajs_capture_box_drop(black_box(slot)) };
            }
        });
    });
}

criterion_group!(benches, bench_alloc_inc_drop_cycle, bench_alloc_drop_no_inc);
criterion_main!(benches);
