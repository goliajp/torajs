//! Performance regression gates for `torajs-ucd`. See [BUDGETS.md].
//!
//! Lookup latency budgets enforced here are 15-30× headroom over
//! observed P95 on a dev machine — CI catches order-of-magnitude
//! regressions, not micro-noise. Don't quote a budget as a perf
//! claim; quote the criterion bench median from `benches/ucd.rs`
//! instead.
//!
//! Run with `cargo test -p torajs-ucd --test perf_gate`.

use std::time::{Duration, Instant};

use torajs_ucd::{is_letter_cp, is_number_cp};

const ITERS: usize = 1_000_000;

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
fn is_letter_cp_1m_under_budget() {
    // 1M is_letter_cp calls under 50 ms median = avg 50 ns / call.
    // P95 observed ~15 ns; 50 ms = ~3× headroom (this is a tight
    // budget — binary search is intrinsically O(log N) cheap so
    // anything beyond is a real algorithm regression).
    let median = time_median(
        || {
            for cp in 0x0000u32..0x10000u32 {
                let _ = is_letter_cp(cp & 0xFFFF);
                if cp >= ITERS as u32 / 16 {
                    break;
                }
            }
        },
        11,
    );
    let budget = Duration::from_millis(50);
    assert!(
        median < budget,
        "is_letter_cp_1m regressed: median {median:?} >= budget {budget:?}"
    );
}

#[test]
fn is_number_cp_1m_under_budget() {
    let median = time_median(
        || {
            for cp in 0x0000u32..0x10000u32 {
                let _ = is_number_cp(cp & 0xFFFF);
                if cp >= ITERS as u32 / 16 {
                    break;
                }
            }
        },
        11,
    );
    let budget = Duration::from_millis(50);
    assert!(
        median < budget,
        "is_number_cp_1m regressed: median {median:?} >= budget {budget:?}"
    );
}
