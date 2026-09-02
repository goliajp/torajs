//! `__torajs_dynobj_iter_slow_mode` — whether bun's inspect can take
//! its fast property walk on this object.
//!
//! bun (`bindings.cpp` `canPerformFastPropertyEnumerationForIterationBun`)
//! walks the JSC Structure directly unless the object has an
//! accessor property, an array-index key, or an own `__proto__`; the
//! slow walk it takes otherwise hides `__proto__` / `@@toStringTag`
//! only when they are non-enumerable (see anyvalue's `key_hidden`).
//! The dictionary-mode arm of that predicate has no counterpart
//! here (a dynobj has no structure transitions).

use core::ffi::c_void;

use crate::accessor::value_is_accessor;
use crate::iter::{__torajs_dynobj_iter_key, __torajs_dynobj_iter_len, __torajs_dynobj_iter_value};
use crate::iter_print_order::__torajs_dynobj_iter_index_count;
use crate::probe::key_str_bytes;

/// 1 when bun would take the slow walk on `obj` (null answers 0).
///
/// # Safety
/// `obj` is NULL or a live dynobj cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_iter_slow_mode(obj: *const c_void) -> i32 {
    if obj.is_null() {
        return 0;
    }
    unsafe {
        if __torajs_dynobj_iter_index_count(obj) > 0 {
            return 1;
        }
        let len = __torajs_dynobj_iter_len(obj);
        for i in 0..len {
            let key = __torajs_dynobj_iter_key(obj, i);
            if key.is_null() {
                continue;
            }
            if value_is_accessor(__torajs_dynobj_iter_value(obj, i)) {
                return 1;
            }
            if let Some((p, n, latin1)) = key_str_bytes(key)
                && latin1
                && n == 9
                && core::slice::from_raw_parts(p, 9) == b"__proto__"
            {
                return 1;
            }
        }
    }
    0
}
