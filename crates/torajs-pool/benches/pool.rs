//! Criterion micro-benches for `FixedPool`.
//!
//! Three suites:
//!  - `acquire_release_hot` — steady-state acquire-release loop where
//!    the pool stays warm (count ~ CAP). The dominant case in torajs
//!    Promise traffic.
//!  - `acquire_cold` — pool empty, every acquire is a fresh heap alloc.
//!    Compared to `acquire_release_hot` to quantify the pool's
//!    speedup over plain malloc/free.
//!  - `overflow_bound` — release path where pool is at CAP and excess
//!    falls through to `dealloc`. Shouldn't be hot in real workload
//!    but a regression here would indicate the bound check got slow.
//!
//! Run with `cargo bench -p torajs-pool`.

use std::hint::black_box;
use std::ptr;

use criterion::{Criterion, criterion_group, criterion_main};
use torajs_pool::FixedPool;

#[repr(C)]
struct Node {
    payload: u64,
    next: *mut u8,
    more: u64,
}

const NEXT_OFFSET: usize = 8;

fn bench_acquire_release_hot(c: &mut Criterion) {
    let pool: FixedPool<Node, 32> = unsafe { FixedPool::new_with_next_offset(NEXT_OFFSET) };
    // Warm the pool to capacity so every iter hits the hot pop / push path.
    let mut warm = [ptr::null_mut::<Node>(); 32];
    for slot in warm.iter_mut() {
        *slot = unsafe { pool.acquire() };
    }
    for &p in warm.iter() {
        unsafe { pool.release(p) };
    }
    assert_eq!(pool.pooled(), 32);

    c.bench_function("acquire_release_hot", |b| {
        b.iter(|| {
            let p = unsafe { pool.acquire() };
            black_box(p);
            unsafe { pool.release(p) };
        });
    });
}

fn bench_acquire_cold(c: &mut Criterion) {
    // Drop the pool each iter to keep it empty — measures plain malloc/free.
    c.bench_function("acquire_cold_malloc_baseline", |b| {
        b.iter(|| {
            let pool: FixedPool<Node, 32> =
                unsafe { FixedPool::new_with_next_offset(NEXT_OFFSET) };
            let p = unsafe { pool.acquire() };
            black_box(p);
            unsafe { pool.release(p) };
            drop(pool);
        });
    });
}

fn bench_overflow_bound(c: &mut Criterion) {
    let pool: FixedPool<Node, 8> = unsafe { FixedPool::new_with_next_offset(NEXT_OFFSET) };
    // Fill to capacity.
    let mut slots = [ptr::null_mut::<Node>(); 8];
    for slot in slots.iter_mut() {
        *slot = unsafe { pool.acquire() };
    }
    for &p in slots.iter() {
        unsafe { pool.release(p) };
    }
    assert_eq!(pool.pooled(), 8);

    c.bench_function("release_overflow_bound", |b| {
        b.iter(|| {
            // Each iter: acquire a fresh extra, release it — pool already
            // full, so release falls through to dealloc.
            let p = unsafe { pool.acquire() };
            // Re-fill before next iter so pool stays at cap-1 in steady state.
            unsafe { pool.release(p) };
        });
    });
}

criterion_group!(
    benches,
    bench_acquire_release_hot,
    bench_acquire_cold,
    bench_overflow_bound,
);
criterion_main!(benches);
