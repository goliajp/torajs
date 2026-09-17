//! `FinalizationRegistry` (§26.2) in the any lane — the constructor
//! kernel both `new FinalizationRegistry(cb)` and the constructor-as-
//! value route reach. The registry cell and its kernels are
//! torajs-weak's (`finreg.rs`).

use core::ffi::c_void;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, box_void_ptr};

unsafe extern "C" {
    fn __torajs_finreg_create(cleanup: u64) -> *mut c_void;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// §26.2.1.1 steps 2–5 — IsCallable(cleanupCallback), then an owned
/// registry retaining it. `cleanup` is borrowed.
///
/// # Safety
/// `cleanup` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_finreg_new(cleanup: AnyValue) -> AnyValue {
    if !crate::promise_capability::is_callable(cleanup) {
        unsafe {
            __torajs_throw_type_error(
                c"First argument to FinalizationRegistry should be a function".as_ptr(),
            )
        };
        return VALUE_UNDEFINED;
    }
    box_void_ptr(unsafe { __torajs_finreg_create(cleanup) })
}
