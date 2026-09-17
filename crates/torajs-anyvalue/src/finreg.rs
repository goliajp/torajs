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
    fn __torajs_throw_check() -> i64;
    fn __torajs_finreg_queue_pop() -> *mut c_void;
    fn __torajs_finreg_take_pending(fr: *mut c_void, out: *mut u64) -> i32;
    fn __torajs_host_job_drain_install(drain: unsafe extern "C" fn() -> i32);
    fn __torajs_value_drop_heap(p: *mut c_void);
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
    unsafe { __torajs_host_job_drain_install(drain_one) };
    box_void_ptr(unsafe { __torajs_finreg_create(cleanup) })
}

/// §9.13 CleanupFinalizationRegistry for the registry at the head of
/// the cleanup queue: each pending cell's held value goes to the
/// callback, `this` undefined. A throw ends the job (the remaining
/// cells stay pending) and is left for the exit checkpoint to report.
unsafe extern "C" fn drain_one() -> i32 {
    unsafe {
        let fr = __torajs_finreg_queue_pop();
        if fr.is_null() {
            return 0;
        }
        // `FinReg::cleanup` (torajs-weak layout, +8). The queue's
        // reference keeps the registry and its callback alive.
        let cleanup = *(fr as *const u64).add(1);
        let mut held = 0u64;
        let mut outcome = 1;
        while __torajs_finreg_take_pending(fr, &mut held) != 0 {
            let r = crate::method_call_closure_dispatch::__torajs_any_call(cleanup, &held, 1);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(r);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(held);
            if __torajs_throw_check() != 0 {
                outcome = -1;
                break;
            }
        }
        __torajs_value_drop_heap(fr);
        outcome
    }
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
