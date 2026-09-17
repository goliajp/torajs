//! `FinalizationRegistry` (§26.2) in the any lane — the constructor
//! kernel both `new FinalizationRegistry(cb)` and the constructor-as-
//! value route reach. The registry cell and its kernels are
//! torajs-weak's (`finreg.rs`).

use core::ffi::c_void;

use torajs_rc::any_method_iter::{ANY_METHOD_REGISTER, ANY_METHOD_UNREGISTER};

use crate::method_call::method_no_such;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, box_bool, box_void_ptr};

unsafe extern "C" {
    fn __torajs_finreg_create(cleanup: u64) -> *mut c_void;
    fn __torajs_finreg_register(
        fr: *mut c_void,
        target: *mut c_void,
        held: u64,
        token: *mut c_void,
    );
    fn __torajs_finreg_unregister(fr: *mut c_void, token: *mut c_void) -> i32;
    /// torajs-weak — CanBeHeldWeakly (§9.13): the cell, or NULL.
    fn __torajs_weak_key_from_any(av: u64) -> *mut c_void;
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

/// Dispatch a method id on a `Tag::FinalizationRegistry` cell. The
/// receiver check (§26.2.3.2 / §26.2.3.3 step 2) is the dispatcher's:
/// only a registry cell reaches this arm.
///
/// # Safety
/// `ptr` is a live registry; `argv` holds `argc` live slots.
pub(crate) unsafe fn finreg_method(
    ptr: *mut c_void,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let arg = |i: i64| -> u64 {
        if i < argc {
            unsafe { argv.add(i as usize).read() }
        } else {
            VALUE_UNDEFINED
        }
    };
    unsafe {
        match mid {
            ANY_METHOD_REGISTER => register(ptr, arg(0), arg(1), arg(2)),
            ANY_METHOD_UNREGISTER => {
                let token = __torajs_weak_key_from_any(arg(0));
                if token.is_null() {
                    return throw(c"unregister requires an object or a non-registered symbol as the unregistration token");
                }
                box_bool(__torajs_finreg_unregister(ptr, token) != 0)
            }
            _ => method_no_such(),
        }
    }
}

/// §26.2.3.2 steps 3–8.
unsafe fn register(fr: *mut c_void, target: u64, held: u64, token: u64) -> AnyValue {
    unsafe {
        let t = __torajs_weak_key_from_any(target);
        if t.is_null() {
            return throw(c"register requires an object or a non-registered symbol as the target");
        }
        // Step 4 — SameValue on a cell is identity, and `target` is a
        // cell here, so the word comparison is exact.
        if held == target {
            return throw(c"register expects the target object and the holdings parameter are not the same. Otherwise, the target can never be collected");
        }
        let tok = if token == VALUE_UNDEFINED {
            core::ptr::null_mut()
        } else {
            let k = __torajs_weak_key_from_any(token);
            if k.is_null() {
                return throw(c"register requires an object or a non-registered symbol as the unregistration token");
            }
            k
        };
        __torajs_finreg_register(fr, t, held, tok);
    }
    VALUE_UNDEFINED
}

unsafe fn throw(msg: &core::ffi::CStr) -> AnyValue {
    unsafe { __torajs_throw_type_error(msg.as_ptr()) };
    VALUE_UNDEFINED
}
