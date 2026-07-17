//! `arr.toLocaleString()` per ES §23.1.3.32 — every non-nullish
//! element invokes ITS `toLocaleString` method (observable: a user
//! object's hook runs once per occurrence; test262
//! S15.4.4.3_A1_T1); undefined / null render as the empty string;
//! the pieces join with ",". Pre-fix the Arr arm delegated to plain
//! `join`, which stringifies elements without ever dispatching the
//! element method.

use core::ffi::c_void;

use torajs_rc::ANY_METHOD_TO_LOCALE_STRING;

use crate::nanbox::{AnyValue, is_null, is_undefined};
use crate::nanbox_encode::__torajs_anyv_box_pointer;
use crate::nanbox_ffi::{__torajs_anyv_rc_dec, __torajs_anyv_to_str};
use crate::{__torajs_str_alloc_pooled, __torajs_str_concat};

unsafe extern "C" {
    /// torajs-arr — kind-aware boxed element read (owned for cells).
    fn __torajs_arr_index_get(arr: *const c_void, idx: i64) -> u64;
    /// torajs-str — release an owned Str.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-throw — pending-throw flag.
    fn __torajs_throw_check() -> i64;
}

/// Arr cell `len` slot mirror.
const ARR_LEN_OFF: usize = 8;

/// Fresh 1-byte "," Str cell (pooled).
unsafe fn comma() -> *mut u8 {
    unsafe {
        let s = __torajs_str_alloc_pooled(1);
        *s.add(16) = b',';
        s
    }
}

/// See module doc. Returns an OWNED boxed Str; a throwing element
/// hook aborts with the pending throw set (caller's throw check
/// propagates) and answers undefined.
pub(crate) unsafe fn arr_to_locale_string(arr: *mut c_void) -> AnyValue {
    unsafe {
        let s = arr_to_locale_str(arr);
        if s.is_null() {
            return crate::nanbox::VALUE_UNDEFINED;
        }
        __torajs_anyv_box_pointer(s as *mut c_void)
    }
}

/// Str-face core — NULL on a pending element-hook throw.
unsafe fn arr_to_locale_str(arr: *mut c_void) -> *mut u8 {
    unsafe {
        let len = (arr.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read();
        let sep = comma();
        // The dynobj arm resolves a monkey-patched hook by NAME
        // (the mid alone can't probe the expando table).
        let name = {
            let bytes = b"toLocaleString";
            let s = __torajs_str_alloc_pooled(bytes.len() as u64);
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), s.add(16), bytes.len());
            s
        };
        let mut acc = __torajs_str_alloc_pooled(0);
        for i in 0..len {
            if i > 0 {
                let next = __torajs_str_concat(acc, sep) as *mut u8;
                __torajs_str_drop(acc as *mut c_void);
                acc = next;
            }
            let v = __torajs_arr_index_get(arr, i as i64);
            if is_undefined(v) || is_null(v) {
                __torajs_anyv_rc_dec(v);
                continue;
            }
            // §23.1.3.32 step 4.b.i-ii — Invoke(element,
            // "toLocaleString"): the element's own hook wins
            // (dynobj arm resolves the monkey-patch first).
            let r = crate::method_call::any_method_call_inner(
                v,
                ANY_METHOD_TO_LOCALE_STRING,
                name,
                core::ptr::null_mut(),
                core::ptr::null(),
                0,
            );
            __torajs_anyv_rc_dec(v);
            if __torajs_throw_check() != 0 {
                __torajs_anyv_rc_dec(r);
                __torajs_str_drop(acc as *mut c_void);
                __torajs_str_drop(sep as *mut c_void);
                __torajs_str_drop(name as *mut c_void);
                return core::ptr::null_mut();
            }
            let s = __torajs_anyv_to_str(r);
            __torajs_anyv_rc_dec(r);
            if __torajs_throw_check() != 0 {
                __torajs_str_drop(s as *mut c_void);
                __torajs_str_drop(acc as *mut c_void);
                __torajs_str_drop(sep as *mut c_void);
                __torajs_str_drop(name as *mut c_void);
                return core::ptr::null_mut();
            }
            let next = __torajs_str_concat(acc, s as *const u8) as *mut u8;
            __torajs_str_drop(s as *mut c_void);
            __torajs_str_drop(acc as *mut c_void);
            acc = next;
        }
        __torajs_str_drop(sep as *mut c_void);
        __torajs_str_drop(name as *mut c_void);
        acc
    }
}

/// Extern face for the typed-tier SSA lane (`xs.toLocaleString()`
/// on an `Arr<Any>` receiver) — a fresh owned Str; on a throwing
/// element hook the pending throw is set and an empty Str
/// placeholder returns for the caller's throw check to unwind.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_to_locale_string(arr: *mut c_void) -> *mut u8 {
    unsafe {
        let s = arr_to_locale_str(arr);
        if s.is_null() {
            return __torajs_str_alloc_pooled(0);
        }
        s
    }
}
