//! Criterion bench on the enqueue + drain cycle. Real workloads
//! (Promise-heavy code) tend to enqueue + drain in short bursts of
//! 1-10 tasks; we measure the per-task overhead across batch sizes.

use core::hint::black_box;
use criterion::{Criterion, criterion_group, criterion_main};

use torajs_microtask::{
    __torajs_microtask_enqueue, __torajs_microtask_pending_count,
    __torajs_microtask_run_until_idle, MicrotaskFn,
};

static COUNT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

unsafe extern "C" fn task_noop(_arg: i64) {
    COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

fn drain_clean() {
    unsafe { __torajs_microtask_run_until_idle() };
    assert_eq!(unsafe { __torajs_microtask_pending_count() }, 0);
}

fn bench_enqueue_drain_burst_8(c: &mut Criterion) {
    let f: MicrotaskFn = task_noop;
    c.bench_function("microtask_enqueue_drain_burst_8-10k", |b| {
        b.iter(|| {
            for _ in 0..10_000 {
                for i in 0..8i64 {
                    unsafe { __torajs_microtask_enqueue(Some(black_box(f)), black_box(i)) };
                }
                unsafe { __torajs_microtask_run_until_idle() };
            }
        });
    });
    drain_clean();
}

fn bench_enqueue_drain_burst_64(c: &mut Criterion) {
    let f: MicrotaskFn = task_noop;
    c.bench_function("microtask_enqueue_drain_burst_64-1k", |b| {
        b.iter(|| {
            for _ in 0..1_000 {
                for i in 0..64i64 {
                    unsafe { __torajs_microtask_enqueue(Some(black_box(f)), black_box(i)) };
                }
                unsafe { __torajs_microtask_run_until_idle() };
            }
        });
    });
    drain_clean();
}

criterion_group!(
    benches,
    bench_enqueue_drain_burst_8,
    bench_enqueue_drain_burst_64
);
criterion_main!(benches);
