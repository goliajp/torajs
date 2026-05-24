//! Performance regression gates for `torajs-capture-box`. See [BUDGETS.md].
//!
//! Hot path is the alloc-inc-drop cycle on every closure-captured-let
//! lifetime. BUDGETS.md target: 100k cycles ≤ 5 ms.
//!
//! Run with `cargo test -p torajs-capture-box --test perf_gate --release`.

use std::time::{Duration, Instant};

use torajs_capture_box::{
    __torajs_capture_box_alloc, __torajs_capture_box_drop, __torajs_capture_box_inc,
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

#[test]
fn alloc_inc_drop_cycle_100k_under_budget() {
    // BUDGETS.md target: 100k cycles ≤ 5 ms (~50 ns/cycle).
    // CI budget: 25 ms (5× headroom).
    let median = time_median(
        || {
            for i in 0..100_000i64 {
                // alloc → rc=0, inc → rc=1, drop → rc=0 → freed.
                // One-inc / one-drop pair matches closure-construct +
                // env-drop lifecycle for a 1-capture closure.
                let slot = __torajs_capture_box_alloc(i);
                unsafe { __torajs_capture_box_inc(slot) };
                unsafe { __torajs_capture_box_drop(slot) };
            }
        },
        11,
    );
    let budget = Duration::from_millis(25);
    assert!(
        median < budget,
        "alloc_inc_drop_cycle regressed: median {median:?} >= budget {budget:?}"
    );
}

#[test]
fn alloc_drop_no_inc_100k_under_budget() {
    // BUDGETS.md target: 100k cycles ≤ 5 ms (rc=0 fast-free path).
    // CI budget: 25 ms.
    let median = time_median(
        || {
            for i in 0..100_000i64 {
                let slot = __torajs_capture_box_alloc(i);
                unsafe { __torajs_capture_box_drop(slot) };
            }
        },
        11,
    );
    let budget = Duration::from_millis(25);
    assert!(
        median < budget,
        "alloc_drop_no_inc regressed: median {median:?} >= budget {budget:?}"
    );
}
