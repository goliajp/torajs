//! The only invariant of `__torajs_value_drop_heap` that's testable
//! without running the full torajs build graph (the per-tag dispatch
//! arms link against workspace-internal staticlibs like libtorajs_str.a)
//! is the null-input contract: `NULL` must be a no-op.
//!
//! Real per-tag dispatch verification happens via the integrated
//! conformance gate (685 fixtures exercise every type-tag drop path
//! end-to-end).

use std::ptr;
use torajs_value_drop::__torajs_value_drop_heap;

#[test]
fn null_input_is_no_op() {
    unsafe { __torajs_value_drop_heap(ptr::null_mut()) };
    // No assertion past the call — passing == not crashing.
}

#[test]
fn null_input_is_idempotent() {
    for _ in 0..64 {
        unsafe { __torajs_value_drop_heap(ptr::null_mut()) };
    }
}
