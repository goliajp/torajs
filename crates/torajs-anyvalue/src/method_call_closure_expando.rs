//! What an own expando entry on a FUNCTION value resolves to —
//! split from `method_call_closure.rs` at the file-size cap.
//!
//! Its host answers "what does calling a method on a function value
//! mean"; this answers the narrower question the expando arm has to
//! settle first, and the two only ever shared a line.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, as_void_ptr, is_cell};

unsafe extern "C" {
    /// torajs-dynobj — own-property probe (5 = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — own-property value read (borrowed).
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> AnyValue;
    /// Borrowed heap-pointer box — no `rc_inc`, same use as the
    /// dynobj arm's receiver.
    fn __torajs_anyv_box_pointer(p: *mut c_void) -> AnyValue;
}

/// A plain closure stored as an expando ON A FUNCTION VALUE, invoked
/// with the function as its receiver. `None` leaves the call to the
/// props-bag walk exactly as before.
///
/// The expando arm delegates the whole dispatch to
/// [`crate::method_call_dynobj::dynobj_method`] against the props
/// bag, and every `this` in there is that bag. For a property READ
/// that is indistinguishable — the properties are in the bag — which
/// is why `K.s = function () { return this.tag }` has always
/// answered. It is not indistinguishable for the function itself:
///
/// ```text
/// const K: any = function () {};
/// K.self = function () { return this };
/// K.self() === K        // bun true, tr false (it was the bag)
/// typeof K.self()       // bun "function", tr "object"
/// ```
///
/// Only a receiver-first closure is intercepted: without that flag
/// `invoke_boxed` never reads a receiver, so the bag walk and this
/// path are the same call. A reified builtin cell and a class-method
/// adapter carry their own dispatch and are left where they were.
pub(crate) unsafe fn expando_this_is_the_function(
    ptr: *mut c_void,
    props: *const c_void,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    unsafe {
        let key = name_str as *const c_void;
        // 4 = ANY_HEAP, a plain cell-valued own entry.
        if __torajs_dynobj_get_tag(props, key) != 4 {
            return None;
        }
        let cell = __torajs_dynobj_get_value(props, key);
        if !is_cell(cell)
            || crate::method_value::builtin_method_mid(as_void_ptr(cell)).is_some()
            || crate::method_value_class::class_method_adapter(as_void_ptr(cell)).is_some()
        {
            return None;
        }
        let (env, entry) = crate::method_call::closure_boxed_entry(cell)?;
        let flags = (env as *const u8).add(6).cast::<u16>().read();
        if flags & torajs_rc::FLAG_CLOSURE_RECV_FIRST == 0 {
            return None;
        }
        Some(crate::method_call::invoke_boxed_recv_first(
            env,
            entry,
            __torajs_anyv_box_pointer(ptr),
            argv,
            argc,
        ))
    }
}
