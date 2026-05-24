//! Performance regression gates for `torajs-num`. See [BUDGETS.md].
//!
//! Budgets target 5× headroom over observed dev-machine median.
//! Math intrinsics are wraps around `f64`-method / libm calls; the
//! wrap adds zero overhead by design. These gates catch the case
//! where a future polish refactor accidentally adds dispatch /
//! allocation / panic path that wasn't there before.
//!
//! Run with `cargo test -p torajs-num --test perf_gate --release`.

use std::time::{Duration, Instant};

// Use the rlib's pub re-exports — the `extern "C"` block approach
// fails to link against the staticlib's no_mangle symbols from test
// crate context (matches the torajs-bigint tests/spec_cases.rs lesson).
use torajs_num::{__torajs_math_floor, __torajs_math_pow, __torajs_math_sqrt};

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
fn sqrt_hot_loop_under_budget() {
    // BUDGETS.md target: 100k sqrt ≤ 1 ms median (~5 ns/call).
    // CI budget 5 ms.
    let median = time_median(
        || {
            let mut acc = 0.0f64;
            for i in 0..100_000 {
                acc += unsafe { __torajs_math_sqrt(i as f64) };
            }
            std::hint::black_box(acc);
        },
        11,
    );
    let budget = Duration::from_millis(5);
    assert!(
        median < budget,
        "math_sqrt regressed: median {median:?} >= budget {budget:?}"
    );
}

#[test]
fn pow_hot_loop_under_budget() {
    // BUDGETS.md target: 10k pow ≤ 0.5 ms median (~20 ns/call).
    // CI budget 2.5 ms.
    let median = time_median(
        || {
            let mut acc = 0.0f64;
            for i in 0..10_000 {
                acc += unsafe { __torajs_math_pow(i as f64 / 100.0, 2.5) };
            }
            std::hint::black_box(acc);
        },
        11,
    );
    let budget = Duration::from_micros(2500);
    assert!(
        median < budget,
        "math_pow regressed: median {median:?} >= budget {budget:?}"
    );
}

#[test]
fn floor_hot_loop_under_budget() {
    // BUDGETS.md target: 100k floor ≤ 0.5 ms median (~2 ns/call).
    // CI budget 2.5 ms.
    let median = time_median(
        || {
            let mut acc = 0.0f64;
            for i in 0..100_000 {
                acc += unsafe { __torajs_math_floor(i as f64 / 7.0) };
            }
            std::hint::black_box(acc);
        },
        11,
    );
    let budget = Duration::from_micros(2500);
    assert!(
        median < budget,
        "math_floor regressed: median {median:?} >= budget {budget:?}"
    );
}
