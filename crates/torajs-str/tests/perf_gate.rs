//! Performance regression gates for `torajs-str`. See [BUDGETS.md].
//!
//! Budgets target 5-10× headroom over the observed dev-machine median
//! so CI catches order-of-magnitude regressions, not micro-noise.
//! Don't quote a budget as a perf claim — use criterion bench medians
//! from `benches/str.rs` for those.
//!
//! Run with `cargo test -p torajs-str --test perf_gate --release`.
//!
//! Note: must be run in `--release` mode. Debug builds are 5-20×
//! slower than release and would tripwire every budget.

use std::time::{Duration, Instant};

use torajs_str::{__torajs_str_eq, __torajs_str_free, __torajs_str_slice, StrBlock};

fn make_str(payload: &[u8]) -> *mut u8 {
    let mut b = StrBlock::alloc(payload.len() as u64);
    let dst = unsafe { b.as_bytes_mut(payload.len() as u64) };
    dst.copy_from_slice(payload);
    b.into_raw()
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

const ITERS: usize = 100_000;

#[test]
fn alloc_free_8byte_under_budget() {
    // BUDGETS.md target: 100k alloc+free pairs ≤ 2 ms median.
    // CI budget 10 ms (5× headroom).
    let median = time_median(
        || {
            for _ in 0..ITERS {
                let p = make_str(b"abcdefgh");
                unsafe { __torajs_str_free(p) };
            }
        },
        11,
    );
    let budget = Duration::from_millis(10);
    assert!(
        median < budget,
        "alloc_free_8byte regressed: median {median:?} >= budget {budget:?}"
    );
}

#[test]
fn eq_48byte_under_budget() {
    // BUDGETS.md target: 100k eq calls ≤ 1 ms median (~3 ns / call).
    // CI budget 5 ms.
    let a = make_str(b"hello-world-from-torajs-str-bench-corpus-aaaaaaa");
    let b = make_str(b"hello-world-from-torajs-str-bench-corpus-aaaaaaa");
    let median = time_median(
        || {
            let mut acc = 0i64;
            for _ in 0..ITERS {
                acc = acc.wrapping_add(unsafe { __torajs_str_eq(a, b) });
            }
            // Sink to keep optimizer honest.
            std::hint::black_box(acc);
        },
        11,
    );
    let budget = Duration::from_millis(5);
    assert!(
        median < budget,
        "eq_48byte regressed: median {median:?} >= budget {budget:?}"
    );
    unsafe {
        __torajs_str_free(a);
        __torajs_str_free(b);
    }
}

#[test]
fn slice_64byte_under_budget() {
    // BUDGETS.md target: 100k slice calls ≤ 5 ms median (~20 ns / call,
    // alloc-dominated).
    // CI budget 25 ms.
    let s = make_str(b"the-quick-brown-fox-jumps-over-the-lazy-dog-aaaaaaaaaaaaaaaaa");
    let median = time_median(
        || {
            for _ in 0..ITERS {
                let r = unsafe { __torajs_str_slice(s, 10, 40) };
                unsafe { __torajs_str_free(r) };
            }
        },
        11,
    );
    let budget = Duration::from_millis(25);
    assert!(
        median < budget,
        "slice_64byte regressed: median {median:?} >= budget {budget:?}"
    );
    unsafe { __torajs_str_free(s) };
}
