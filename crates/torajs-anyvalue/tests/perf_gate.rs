//! Performance regression gates for `torajs-anyvalue`. See [BUDGETS.md].
//!
//! Budgets set with ~10× headroom over observed P95. Don't quote
//! a budget as a perf claim — quote the criterion bench median.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use torajs_anyvalue::{__torajs_any_box, __torajs_any_box_drop, AnyBox};
use torajs_rc::AnySlotTag;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_target_dying(_target: *mut c_void) {}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_value_drop_heap(_child: *mut c_void) {}

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
fn method_alloc_drop_pair_under_budget() {
    // 100k alloc-drop pairs ≤ 50 ms (per pair ≈ 500 ns budget;
    // observed ≈ 50-80 ns post-LTO, so ~10× headroom).
    let median = time_median(
        || {
            for i in 0..ITERS {
                let p = AnyBox::alloc(AnySlotTag::I64, i as i64);
                unsafe { AnyBox::drop_owned(p) };
            }
        },
        11,
    );
    let budget = Duration::from_millis(50);
    assert!(
        median < budget,
        "alloc_drop_pair regressed: median {median:?} >= budget {budget:?}"
    );
}

#[test]
fn ffi_alloc_drop_pair_under_budget() {
    // Same budget; FFI shim should have negligible overhead under
    // fat LTO.
    let median = time_median(
        || unsafe {
            for i in 0..ITERS {
                let p = __torajs_any_box(2 /* I64 */, i as i64);
                __torajs_any_box_drop(p);
            }
        },
        11,
    );
    let budget = Duration::from_millis(50);
    assert!(
        median < budget,
        "ffi_alloc_drop_pair regressed: median {median:?} >= budget {budget:?}"
    );
}

#[test]
fn anybox_layout_size_invariant() {
    use std::mem::{align_of, size_of};
    assert_eq!(size_of::<AnyBox>(), 24);
    assert_eq!(align_of::<AnyBox>(), 8);
}
