//! Zero-index-surface receivers of the generic array-like lane —
//! carved out of `method_call_arraylike.rs` verbatim (file-size
//! cap). One self-contained per-mid answer table; the throw-order
//! rationale rides the fn doc below.

use super::*;

/// 刀 2 (RFC 20260714-t262-top-clusters) — ES generic array-like
/// semantics over a receiver with NO index surface at all (bool
/// primitives: ToObject(v) carries no own `length`, so
/// `ToLength(undefined) = 0` and every loop is empty). The HOF
/// family still runs the §23.1.3 IsCallable(callbackfn) check
/// before the (empty) loop — the throw order is observable. Reduce
/// without an initial value throws per §23.1.3.24 step 5. Mutators
/// are excluded by the caller (a primitive receiver cannot grow).
pub(super) unsafe fn arraylike_empty(mid: i64, argv: *const u64, argc: i64) -> AnyValue {
    use torajs_rc::{ANY_METHOD_FLAT, ANY_METHOD_FLAT_MAP};
    let arg0 = if argc > 0 {
        unsafe { *argv }
    } else {
        VALUE_UNDEFINED
    };
    unsafe {
        match mid {
            m if m == ANY_METHOD_INDEX_OF
                || m == ANY_METHOD_LAST_INDEX_OF
                || m == ANY_METHOD_FIND_INDEX
                || m == ANY_METHOD_FIND_LAST_INDEX =>
            {
                if m == ANY_METHOD_FIND_INDEX || m == ANY_METHOD_FIND_LAST_INDEX {
                    if crate::method_call::closure_boxed_entry(arg0).is_none() {
                        return crate::method_call::not_callable();
                    }
                }
                __torajs_anyv_box_i64(-1)
            }
            m if m == ANY_METHOD_INCLUDES => __torajs_anyv_box_from_pair(1, 0),
            m if m == ANY_METHOD_EVERY || m == ANY_METHOD_SOME => {
                if crate::method_call::closure_boxed_entry(arg0).is_none() {
                    return crate::method_call::not_callable();
                }
                __torajs_anyv_box_from_pair(1, i64::from(m == ANY_METHOD_EVERY))
            }
            m if m == ANY_METHOD_FIND || m == ANY_METHOD_FIND_LAST || m == ANY_METHOD_FOR_EACH => {
                if crate::method_call::closure_boxed_entry(arg0).is_none() {
                    return crate::method_call::not_callable();
                }
                VALUE_UNDEFINED
            }
            m if m == ANY_METHOD_AT => VALUE_UNDEFINED,
            m if m == ANY_METHOD_MAP || m == ANY_METHOD_FILTER || m == ANY_METHOD_FLAT_MAP => {
                if crate::method_call::closure_boxed_entry(arg0).is_none() {
                    return crate::method_call::not_callable();
                }
                __torajs_anyv_box_pointer(__torajs_arr_alloc_any(0) as *mut c_void)
            }
            m if m == ANY_METHOD_SLICE || m == ANY_METHOD_FLAT || m == ANY_METHOD_TO_REVERSED => {
                __torajs_anyv_box_pointer(__torajs_arr_alloc_any(0) as *mut c_void)
            }
            m if m == ANY_METHOD_TO_SORTED => {
                // §23.1.3.34 step 1 — the comparator admit still
                // precedes the (empty) product.
                if !is_undefined(arg0) && crate::method_call::closure_boxed_entry(arg0).is_none() {
                    return crate::method_call::not_callable();
                }
                __torajs_anyv_box_pointer(__torajs_arr_alloc_any(0) as *mut c_void)
            }
            // NULL host is never dereferenced at len 0 — toSpliced's
            // product is just the items; with always RangeErrors.
            m if m == ANY_METHOD_TO_SPLICED => {
                crate::method_call_arraylike_copy::arraylike_to_spliced(
                    core::ptr::null_mut(),
                    0,
                    argv,
                    argc,
                )
            }
            m if m == ANY_METHOD_WITH => crate::method_call_arraylike_copy::arraylike_with(
                core::ptr::null_mut(),
                0,
                argv,
                argc,
            ),
            m if m == ANY_METHOD_JOIN || m == ANY_METHOD_TO_STRING => {
                __torajs_anyv_box_pointer(__torajs_str_alloc(b"".as_ptr(), 0) as *mut c_void)
            }
            m if m == ANY_METHOD_REDUCE || m == ANY_METHOD_REDUCE_RIGHT => {
                if crate::method_call::closure_boxed_entry(arg0).is_none() {
                    return crate::method_call::not_callable();
                }
                if argc < 2 {
                    // §23.1.3.24 step 5 — empty + no initial value.
                    __torajs_throw_type_error_msg();
                    return VALUE_UNDEFINED;
                }
                let init = *argv.add(1);
                crate::nanbox_ffi::__torajs_anyv_rc_inc(init);
                init
            }
            _ => crate::method_call::method_no_such(),
        }
    }
}

unsafe fn __torajs_throw_type_error_msg() {
    unsafe extern "C" {
        fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    }
    unsafe {
        __torajs_throw_type_error(c"Reduce of empty array with no initial value".as_ptr());
    }
}
