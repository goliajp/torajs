//! Performance regression gates for `torajs-microtask`. See [BUDGETS.md].
//!
//! Hot path is `__torajs_microtask_enqueue` + `_run_until_idle` —
//! every Promise reaction + every `queueMicrotask` user call goes
//! through here.
//!
//! Run with `cargo test -p torajs-microtask --test perf_gate --release`.

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use torajs_microtask::{
    __torajs_microtask_enqueue, __torajs_microtask_pending_count,
    __torajs_microtask_run_until_idle, MicrotaskFn,
};

static COUNT: AtomicI64 = AtomicI64::new(0);

unsafe extern "C" fn task_noop(_arg: i64) {
    COUNT.fetch_add(1, Ordering::Relaxed);
}

fn drain_clean() {
    unsafe { __torajs_microtask_run_until_idle() };
    assert_eq!(unsafe { __torajs_microtask_pending_count() }, 0);
}

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
fn enqueue_drain_burst_8_10k_under_budget() {
    // BUDGETS.md target: 10k cycles × (8 enqueues + drain) ≤ 5 ms.
    // CI budget: 25 ms (5× headroom).
    let f: MicrotaskFn = task_noop;
    drain_clean();
    let median = time_median(
        || {
            for _ in 0..10_000 {
                for i in 0..8i64 {
                    unsafe { __torajs_microtask_enqueue(Some(f), i) };
                }
                unsafe { __torajs_microtask_run_until_idle() };
            }
        },
        11,
    );
    let budget = Duration::from_millis(25);
    assert!(
        median < budget,
        "microtask burst-8 regressed: median {median:?} >= budget {budget:?}"
    );
    drain_clean();
}

#[test]
fn enqueue_drain_burst_64_1k_under_budget() {
    // BUDGETS.md target: 1k cycles × (64 enqueues + drain) ≤ 2 ms.
    // CI budget: 10 ms.
    let f: MicrotaskFn = task_noop;
    drain_clean();
    let median = time_median(
        || {
            for _ in 0..1_000 {
                for i in 0..64i64 {
                    unsafe { __torajs_microtask_enqueue(Some(f), i) };
                }
                unsafe { __torajs_microtask_run_until_idle() };
            }
        },
        11,
    );
    let budget = Duration::from_millis(10);
    assert!(
        median < budget,
        "microtask burst-64 regressed: median {median:?} >= budget {budget:?}"
    );
    drain_clean();
}
