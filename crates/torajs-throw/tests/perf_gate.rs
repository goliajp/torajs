//! Performance regression gates for `torajs-throw`. See [BUDGETS.md].
//!
//! Hot path is `__torajs_throw_check` (the IR-emitted poll after every
//! "may throw" runtime helper call). BUDGETS.md target: 100k checks
//! ≤ 200 µs (~2 ns/check).
//!
//! Run with `cargo test -p torajs-throw --test perf_gate --release`.

use std::time::{Duration, Instant};

use torajs_throw::{
    __torajs_throw_check, __torajs_throw_set, __torajs_throw_take, __torajs_throw_take_tag,
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

fn reset_slot() {
    unsafe {
        __torajs_throw_set(0, 0);
        let _ = __torajs_throw_take();
    }
}

#[test]
fn throw_check_happy_path_100k_under_budget() {
    // BUDGETS.md target: 100k polls ≤ 200 µs (~2 ns/call).
    // CI budget: 1 ms (5× headroom).
    reset_slot();
    let median = time_median(
        || {
            let mut acc = 0i64;
            for _ in 0..100_000 {
                acc = acc.wrapping_add(unsafe { __torajs_throw_check() });
            }
            std::hint::black_box(acc);
        },
        11,
    );
    let budget = Duration::from_millis(1);
    assert!(
        median < budget,
        "throw_check happy-path regressed: median {median:?} >= budget {budget:?}"
    );
}

#[test]
fn throw_set_take_cycle_100k_under_budget() {
    // BUDGETS.md target: 100k cycles ≤ 3 ms (cold path).
    // CI budget: 15 ms (5× headroom).
    reset_slot();
    let median = time_median(
        || {
            for i in 0..100_000i64 {
                unsafe {
                    __torajs_throw_set(i, i.wrapping_mul(7));
                    let _ = __torajs_throw_take_tag();
                    let _ = __torajs_throw_take();
                }
            }
        },
        11,
    );
    let budget = Duration::from_millis(15);
    assert!(
        median < budget,
        "throw_set_take cycle regressed: median {median:?} >= budget {budget:?}"
    );
}
