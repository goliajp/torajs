//! Black-box tests for the native-error factory registry. The
//! registry is process-global (a 3-slot `static AtomicPtr<()>`),
//! so tests that register a factory persist that registration for
//! the rest of the test-binary run. We use a marker fn-ptr per test
//! to disambiguate and let later tests verify the slot still holds
//! the latest registration.

use std::ffi::c_void;
use torajs_throw::{
    __torajs_register_native_error, NativeErrorFactory, SLOT_ERROR, SLOT_RANGE_ERROR,
    SLOT_TYPE_ERROR,
};

// Three distinct marker fns so we can verify slot independence.
unsafe extern "C" fn factory_error(_msg: *mut c_void) -> *mut c_void {
    0x1111_1111 as *mut c_void
}
unsafe extern "C" fn factory_type(_msg: *mut c_void) -> *mut c_void {
    0x2222_2222 as *mut c_void
}
unsafe extern "C" fn factory_range(_msg: *mut c_void) -> *mut c_void {
    0x3333_3333 as *mut c_void
}

#[test]
fn out_of_range_slot_is_no_op() {
    // Negative slot — must be ignored, no segfault.
    let placeholder: NativeErrorFactory = factory_error;
    let ptr = placeholder as *mut c_void;
    unsafe {
        __torajs_register_native_error(-1, ptr);
        __torajs_register_native_error(-5, ptr);
        __torajs_register_native_error(3, ptr);
        __torajs_register_native_error(100, ptr);
    }
    // No assertion — passing == not crashing.
}

#[test]
fn register_each_slot_with_distinct_factory() {
    unsafe {
        __torajs_register_native_error(SLOT_ERROR as i64, factory_error as *mut c_void);
        __torajs_register_native_error(SLOT_TYPE_ERROR as i64, factory_type as *mut c_void);
        __torajs_register_native_error(SLOT_RANGE_ERROR as i64, factory_range as *mut c_void);
    }
    // No reader API exposed, so we can't directly assert the
    // stored values from this crate's surface. The smoke test for
    // "no overwrite spillage between slots" is end-to-end via the
    // ssa-emitted `synthesize_module_init` which registers all
    // three at startup; if the slots aliased, conformance gate
    // would catch it (TypeError catches would receive RangeError
    // instances or vice-versa).
}

#[test]
fn register_null_factory_is_allowed() {
    // Null fn-ptr = "unregister"; lookup falls back to bare-string
    // throw. The registry doesn't reject this.
    unsafe {
        __torajs_register_native_error(SLOT_ERROR as i64, std::ptr::null_mut());
    }
}

#[test]
fn slot_const_discriminants_match_c_abi() {
    // The C-side `__TORAJS_SLOT_*` constants must match these
    // values exactly or the codegen-emitted register calls go to
    // the wrong slot.
    assert_eq!(SLOT_ERROR, 0);
    assert_eq!(SLOT_TYPE_ERROR, 1);
    assert_eq!(SLOT_RANGE_ERROR, 2);
}
