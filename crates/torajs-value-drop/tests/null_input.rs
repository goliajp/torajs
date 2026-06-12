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

// Linker stubs for the per-tag `__torajs_*_drop` externs the
// dispatch body references. The shipped binary resolves them from
// sibling staticlibs at `tr build` link time; this test binary has
// no staticlib injection (standard cargo test link), so panicking
// stubs satisfy the linker — same convention as torajs-arr's
// `__torajs_value_drop_heap` unit-test stub. Only the null path
// runs here, which dispatches nowhere; a stub actually firing is a
// test failure by construction.
macro_rules! per_tag_drop_stub {
    ($($name:ident),+ $(,)?) => {$(
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(_p: *mut std::ffi::c_void) {
            panic!(concat!(
                "torajs-value-drop integration-test stub: ",
                stringify!($name),
                " must not be called (null-path-only tests)"
            ));
        }
    )+};
}

per_tag_drop_stub!(
    __torajs_str_drop,
    __torajs_arr_drop,
    __torajs_bigint_drop,
    __torajs_weakref_drop,
    __torajs_weakmap_drop,
    __torajs_weakset_drop,
    __torajs_map_drop,
    __torajs_map_iter_drop,
    __torajs_arr_iter_drop,
    __torajs_dynobj_drop,
    __torajs_response_drop,
);

// Transitive extern: torajs-rc's `__torajs_rc_dec` notifies the weak
// registry on death. No-op stub (not panicking) — same convention as
// torajs-anyvalue's unit-test stub: a future test legitimately
// reaching rc_dec must not trip over the notification hook.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_target_dying(_target: *mut std::ffi::c_void) {}

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
