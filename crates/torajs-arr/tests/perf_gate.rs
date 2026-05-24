//! Performance regression gates for `torajs-arr`. See [BUDGETS.md].
//!
//! Hottest crate in the workspace. Budgets target the bench-corpus
//! hot paths (push, push_unchecked) with 5× headroom over observed
//! dev-machine median.
//!
//! Note: these gates exercise the **staticlib path** for push /
//! push_unchecked / shift — NOT the inkwell-emitted alwaysinline
//! IR path that user binaries actually run through. The bench
//! corpus (array-sum-1m / array-map-1m / fifo-queue-100k /
//! stack-pop-1m) is the integrated test of the IR path. These
//! gates protect the cross-staticlib Rust caller path.
//!
//! ## Why we don't `arr_drop`
//!
//! `__torajs_arr_drop` delegates to cross-tier externs in
//! `torajs-value-drop` + `torajs-weak` (__torajs_arrprops_drop_entry,
//! __torajs_value_drop_heap, __torajs_weakref_target_dying). Those
//! externs are workspace-internal and not linked into a standalone
//! `cargo test -p torajs-arr` run. Tests intentionally leak the arrs;
//! the test process exits after the runner finishes so the OS
//! reclaims the memory.
//!
//! Run with `cargo test -p torajs-arr --test perf_gate --release`.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use torajs_arr::{
    __torajs_arr_alloc, __torajs_arr_push, __torajs_arr_push_unchecked, __torajs_arr_reserve,
    __torajs_arr_shift,
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
fn push_100k_under_budget() {
    // BUDGETS.md target: 100k pushes via staticlib extern (not the
    // alwaysinline IR path; that's exercised by array-sum-1m bench).
    // CI budget: 50 ms (5× headroom over ~10 ms observed).
    let median = time_median(
        || {
            let mut arr = unsafe { __torajs_arr_alloc(0) as *mut u8 };
            for i in 0..100_000i64 {
                arr = unsafe { __torajs_arr_push(arr, i) };
            }
            // arr leaked intentionally — see crate doc.
            std::hint::black_box(arr);
        },
        11,
    );
    let budget = Duration::from_millis(50);
    assert!(
        median < budget,
        "arr_push 100k regressed: median {median:?} >= budget {budget:?}"
    );
}

#[test]
fn push_unchecked_100k_after_reserve_under_budget() {
    // The hot path: pre-reserve, then push_unchecked.
    // CI budget: 25 ms.
    let median = time_median(
        || {
            let mut arr = unsafe { __torajs_arr_alloc(0) as *mut u8 };
            arr = unsafe { __torajs_arr_reserve(arr, 100_000) };
            for i in 0..100_000i64 {
                unsafe { __torajs_arr_push_unchecked(arr as *mut c_void, i) };
            }
            std::hint::black_box(arr);
        },
        11,
    );
    let budget = Duration::from_millis(25);
    assert!(
        median < budget,
        "arr_push_unchecked 100k regressed: median {median:?} >= budget {budget:?}"
    );
}

#[test]
fn shift_10k_under_budget() {
    // T-13.5 O(1) deque shift via head_offset.
    // 10k shifts on a pre-populated 10k-element array.
    // CI budget: 5 ms.
    let mut times = Vec::with_capacity(5);
    for _ in 0..5 {
        let mut tmp = unsafe { __torajs_arr_alloc(0) as *mut u8 };
        tmp = unsafe { __torajs_arr_reserve(tmp, 10_000) };
        for i in 0..10_000i64 {
            unsafe { __torajs_arr_push_unchecked(tmp as *mut c_void, i) };
        }
        let start = Instant::now();
        for _ in 0..10_000 {
            let _ = unsafe { __torajs_arr_shift(tmp) };
        }
        times.push(start.elapsed());
        std::hint::black_box(tmp);
    }
    times.sort();
    let median = times[2];
    let budget = Duration::from_millis(5);
    assert!(
        median < budget,
        "arr_shift 10k regressed: median {median:?} >= budget {budget:?}"
    );
}
