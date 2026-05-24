//! Integration tests for the capture-box lifecycle. Inline unit
//! tests in `src/lib.rs` cover the happy-path API; these tests
//! exercise invariants that benefit from black-box framing
//! (visibility through the crate's public surface only).

use torajs_capture_box::{
    __torajs_capture_box_alloc, __torajs_capture_box_drop, __torajs_capture_box_inc,
};

/// `inc` is monotonic — N increments leave the box live for N drops.
/// The previous (N-1) drops must NOT free the underlying allocation;
/// the Nth one does. Detecting the free indirectly: after N drops
/// the slot pointer is dangling, but a fresh alloc may (or may not,
/// depending on allocator) reuse the same address. We test the
/// no-crash + no-leak shape: walk through inc/drop pairs in
/// configurations 1..=8 and verify each completes without
/// double-free / use-after-free (which would be caught by miri /
/// asan if enabled).
#[test]
fn inc_then_drop_balanced_n_pairs() {
    for n in 1..=8 {
        let slot = __torajs_capture_box_alloc(0x1234_5678);
        for _ in 0..n {
            unsafe { __torajs_capture_box_inc(slot) };
        }
        // Confirm read-through works at any inc count.
        let v = unsafe { *(slot as *const i64) };
        assert_eq!(v, 0x1234_5678, "value persists across {n} incs");
        for _ in 0..n {
            unsafe { __torajs_capture_box_drop(slot) };
        }
    }
}

/// Multiple boxes don't share state — concurrent allocs from the
/// same thread return distinct slots, and dropping one doesn't
/// disturb the others.
#[test]
fn multiple_boxes_isolated() {
    let a = __torajs_capture_box_alloc(11);
    let b = __torajs_capture_box_alloc(22);
    let c = __torajs_capture_box_alloc(33);

    assert_ne!(a, b);
    assert_ne!(b, c);
    assert_ne!(a, c);

    assert_eq!(unsafe { *(a as *const i64) }, 11);
    assert_eq!(unsafe { *(b as *const i64) }, 22);
    assert_eq!(unsafe { *(c as *const i64) }, 33);

    // Drop b in the middle — a and c must remain readable.
    unsafe { __torajs_capture_box_drop(b) };
    assert_eq!(unsafe { *(a as *const i64) }, 11);
    assert_eq!(unsafe { *(c as *const i64) }, 33);

    unsafe { __torajs_capture_box_drop(a) };
    unsafe { __torajs_capture_box_drop(c) };
}

/// Value-slot alignment invariant — every alloc returns a pointer
/// aligned for `i64` so direct `*mut i64` reads/writes are
/// well-defined.
#[test]
fn value_slot_8_aligned() {
    for init in [
        0,
        1,
        -1,
        i64::MIN,
        i64::MAX,
        0x1234_5678_9abc_def0_u64 as i64,
    ] {
        let slot = __torajs_capture_box_alloc(init);
        let addr = slot as usize;
        assert_eq!(addr % 8, 0, "value-slot must be 8-aligned for init={init}");
        let v = unsafe { *(slot as *const i64) };
        assert_eq!(v, init);
        unsafe { __torajs_capture_box_drop(slot) };
    }
}

/// Null inputs to `inc` / `drop` are no-ops (matches the
/// safety-contract documentation in `lib.rs`).
#[test]
fn null_inputs_no_op() {
    unsafe {
        __torajs_capture_box_inc(core::ptr::null_mut());
        __torajs_capture_box_drop(core::ptr::null_mut());
        // Repeat several times — confirm idempotent + no segfault.
        for _ in 0..16 {
            __torajs_capture_box_inc(core::ptr::null_mut());
            __torajs_capture_box_drop(core::ptr::null_mut());
        }
    }
}

/// Asymmetric drop without prior inc is the "promoted-but-never-
/// captured" edge case (see lib.rs doc on rc=0 initial state). The
/// box must free cleanly here.
#[test]
fn drop_without_any_inc_frees_box() {
    for _ in 0..16 {
        let slot = __torajs_capture_box_alloc(0);
        unsafe { __torajs_capture_box_drop(slot) };
    }
    // No assertion past drop — if drop double-freed or leaked, asan
    // / miri / a stress run on this in a loop would catch it.
}
