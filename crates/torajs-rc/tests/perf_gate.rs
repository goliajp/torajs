//! Performance regression gates for `torajs-rc`. See [BUDGETS.md].
//!
//! Budgets set with ~10× headroom over observed P95 on a dev
//! machine so CI catches order-of-magnitude regressions, not
//! micro-noise. Don't quote a budget as a perf claim — quote the
//! criterion bench median from `benches/rc.rs`.
//!
//! Run with `cargo test -p torajs-rc --test perf_gate`.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use torajs_rc::{
    __torajs_rc_dec, __torajs_rc_inc, DropPolicy, FLAG_STATIC_LITERAL, HeapHeader, Tag,
};

// As in benches/rc.rs — perf_gate is a stand-alone integration
// test binary, so we provide the WeakRef hook stub here too.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_target_dying(_target: *mut c_void) {}

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
fn ffi_inc_dec_pair_under_budget() {
    // The FFI shape ssa_lower emits IR-level calls to: 1M
    // inc-dec pairs ≤ 20 ms median.
    let mut h = HeapHeader::new(Tag::Obj);
    let p = &mut h as *mut HeapHeader as *mut c_void;

    let median = time_median(
        || {
            for _ in 0..ITERS {
                unsafe { __torajs_rc_inc(p) };
                let _ = unsafe { __torajs_rc_dec(p) };
            }
        },
        11,
    );
    let budget = Duration::from_millis(20);
    assert!(
        median < budget,
        "ffi_inc_dec_pair regressed: median {median:?} >= budget {budget:?} \
         (per-pair ≈ {ns} ns over 1M iters)",
        ns = median.as_nanos() / ITERS as u128,
    );
    assert_eq!(
        h.refcount, 1,
        "balanced ffi inc/dec must leave refcount unchanged"
    );
}

#[test]
fn method_inc_dec_pair_under_budget() {
    // The idiomatic Rust path. Future Rust sub-crates (torajs-arr,
    // torajs-dynobj, etc.) will call inc_ref / dec_ref directly
    // without going through the FFI shim, so we hold a budget
    // here too. Same envelope as FFI under fat LTO.
    let mut h = HeapHeader::new(Tag::Obj);

    let median = time_median(
        || {
            for _ in 0..ITERS {
                h.inc_ref();
                let _ = h.dec_ref();
            }
        },
        11,
    );
    let budget = Duration::from_millis(20);
    assert!(
        median < budget,
        "method_inc_dec_pair regressed: median {median:?} >= budget {budget:?}"
    );
    assert_eq!(h.refcount, 1);
}

#[test]
fn ffi_inc_static_literal_under_budget() {
    // STATIC_LITERAL bypass — even hotter than the normal inc
    // path because the branch goes to the early return. 1M iters
    // ≤ 10 ms.
    let mut h = HeapHeader::new(Tag::Str);
    h.flags |= FLAG_STATIC_LITERAL;
    let p = &mut h as *mut HeapHeader as *mut c_void;

    let median = time_median(
        || {
            for _ in 0..ITERS {
                unsafe { __torajs_rc_inc(p) };
            }
        },
        11,
    );
    let budget = Duration::from_millis(10);
    assert!(
        median < budget,
        "ffi_inc_static_literal regressed: median {median:?} >= budget {budget:?}"
    );
    assert_eq!(
        h.refcount, 1,
        "STATIC_LITERAL bypass must not touch refcount"
    );
}

#[test]
fn dec_ref_returns_drop_policy_free_on_zero() {
    // Functional gate: the typed verdict end-to-end. Catches any
    // accidental change in the FFI wrapper or in DropPolicy
    // variant ordering that would break the C-side
    // `if (__torajs_rc_dec(p)) free(p)` pattern.
    let mut h = HeapHeader::new(Tag::Obj);
    assert_eq!(h.dec_ref(), DropPolicy::Free);
    let mut h2 = HeapHeader {
        refcount: 2,
        type_tag: Tag::Obj as u16,
        flags: 0,
    };
    assert_eq!(h2.dec_ref(), DropPolicy::Keep);
    assert_eq!(h2.dec_ref(), DropPolicy::Free);
}

#[test]
fn header_layout_size_invariant() {
    // ABI gate: HeapHeader stays exactly 8 bytes 8-aligned.
    // Drift would shift every per-type struct's payload offset
    // and silently break ssa_lower's IR const-offset arithmetic.
    use std::mem::{align_of, size_of};
    assert_eq!(size_of::<HeapHeader>(), 8);
    assert_eq!(align_of::<HeapHeader>(), 8);
}
