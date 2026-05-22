//! Performance regression gates for `torajs-pool`. See [BUDGETS.md].
//!
//! Every gated path here runs in the torajs runtime per Promise
//! alloc / release; the budgets are set with 15-30× headroom over
//! observed P95 on a dev machine so CI catches order-of-magnitude
//! regressions, not micro-noise. Don't quote a budget as a perf
//! claim — quote the criterion bench median from
//! `benches/pool.rs` output instead.
//!
//! Run with `cargo test -p torajs-pool --test perf_gate`.

use std::ptr;
use std::time::{Duration, Instant};

use torajs_pool::FixedPool;

#[repr(C)]
struct Node {
    payload: u64,
    next: *mut u8,
    more: u64,
}

const NEXT_OFFSET: usize = 8;
const ITERS: usize = 100_000;

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

#[test]
fn acquire_release_hot_loop_under_budget() {
    // Hot path: pool warm (count ~ CAP), each iter pops + pushes a slot.
    // BUDGETS.md target: 100k acquire+release pairs ≤ 5 ms median.
    let pool: FixedPool<Node, 32> = unsafe { FixedPool::new_with_next_offset(NEXT_OFFSET) };
    // Warm to capacity.
    let mut warm = [ptr::null_mut::<Node>(); 32];
    for slot in warm.iter_mut() {
        *slot = unsafe { pool.acquire() };
    }
    for &p in warm.iter() {
        unsafe { pool.release(p) };
    }
    assert_eq!(pool.pooled(), 32);

    let median = time_median(
        || {
            for _ in 0..ITERS {
                let p = unsafe { pool.acquire() };
                unsafe { pool.release(p) };
            }
        },
        11,
    );
    let budget = Duration::from_millis(5);
    assert!(
        median < budget,
        "acquire_release_hot regressed: median {median:?} >= budget {budget:?}"
    );
}

#[test]
fn pooled_count_invariant() {
    // Functional invariant: pooled count never exceeds CAP and never goes
    // negative across acquire/release cycles.
    let pool: FixedPool<Node, 4> = unsafe { FixedPool::new_with_next_offset(NEXT_OFFSET) };
    assert_eq!(pool.pooled(), 0);
    assert_eq!(pool.capacity(), 4);
    let p = unsafe { pool.acquire() };
    assert_eq!(pool.pooled(), 0);
    unsafe { pool.release(p) };
    assert_eq!(pool.pooled(), 1);
    // Release past cap doesn't grow count.
    for _ in 0..10 {
        let p = unsafe { pool.acquire() };
        unsafe { pool.release(p) };
    }
    assert!(pool.pooled() <= 4);
}
