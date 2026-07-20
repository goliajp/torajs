//! Wrapper-receiver expando method dispatch — split from
//! `method_call.rs` (file-size cap). See the probe doc below; the
//! dispatcher calls it before the view-through surface so the ES
//! own-property order holds.

use core::ffi::c_void;

use crate::method_call::{closure_cell_entry, invoke_with_this, not_callable};
use crate::nanbox::AnyValue;

unsafe extern "C" {
    /// torajs-dynobj — own-property probe pair ((5, 0) = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
}

/// Wrapper-receiver expando method probe — `Some(result)` when the
/// wrapper's lazy props dynobj owns `name` as a live entry and the
/// stored value dispatches: a reified builtin cell re-dispatches
/// its ORIGINAL mid against the wrapper receiver (the generic
/// `.call` semantics — a String.prototype method ToStrings the
/// receiver), any other closure invokes with the wrapper as `this`
/// through the receiver channel, and a stored non-callable is the
/// §13.3.6 TypeError. `None` = no expando entry; the caller falls
/// through to the view-through surface.
pub(crate) unsafe fn wrapper_expando_method(
    recv: AnyValue,
    ptr: *mut c_void,
    mid: i64,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    unsafe {
        let props = crate::member_get::wrapper_props(ptr);
        if props.is_null() {
            return None;
        }
        // A known-mid call site carries no name bytes — mint the
        // canonical name from the meta row for the expando probe
        // (cold path; the expando table is a monkey-patch surface).
        let mut minted: *mut u8 = core::ptr::null_mut();
        let key: *const u8 = if !name_str.is_null() {
            name_str
        } else if let Some((nm, _)) = torajs_rc::any_method_meta(mid) {
            let s = crate::__torajs_str_alloc_pooled(nm.len() as u64);
            core::ptr::copy_nonoverlapping(nm.as_ptr(), s.add(16), nm.len());
            minted = s;
            s
        } else {
            return None;
        };
        let release = |m: *mut u8| {
            if !m.is_null() {
                crate::__torajs_str_drop(m as *mut c_void);
            }
        };
        if __torajs_dynobj_get_tag(props, key as *const c_void) == 5 {
            release(minted);
            return None;
        }
        let cell = __torajs_dynobj_get_value(props, key as *const c_void) as *mut c_void;
        release(minted);
        // Reified builtin method cell — re-dispatch the original id
        // with the wrapper as receiver (the closure_method .call
        // path's CallTarget::Builtin arm, generic coerce included).
        if let Some(orig_mid) = crate::method_value::builtin_method_mid(cell) {
            if let Some(out) =
                crate::method_call_closure::generic_str_this(orig_mid, recv, argv, argc)
            {
                return Some(out);
            }
            return Some(crate::method_call::any_method_redispatch(
                recv, orig_mid, argv, argc,
            ));
        }
        if let Some((env, entry)) = closure_cell_entry(cell) {
            return Some(invoke_with_this(env, entry, recv, argv, argc));
        }
        Some(not_callable())
    }
}
