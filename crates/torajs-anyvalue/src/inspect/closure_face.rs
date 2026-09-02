//! What a closure cell's inspect face reads.
//!
//! Three answers, in the order the cell shapes discriminate them:
//! a builtin CONSTRUCTOR reads as the class it is (563-06, see
//! [`super::ctor_class_form`]); a reified builtin METHOD cell
//! (chunk 715) prints its interned method name directly — its
//! `fn_addr` is the throwing native entry and never hits the
//! registry; every ordinary closure keeps the fn-addr table lookup
//! (`__torajs_fn_print_inline`), which is also where the anonymous
//! `[Function]` spelling lives.

use core::ffi::c_void;

use super::formatters::{__torajs_fn_print_inline, put_bytes};

/// Emit a closure cell's `[Function: <name>]` form, no trailing
/// newline.
///
/// # Safety
/// `closure` is a live `Tag::Closure` cell.
pub(super) unsafe fn put_closure_fn_name(closure: *const c_void) {
    // 563-06 — a builtin constructor reads as the class it is.
    if unsafe { super::ctor_class_form::put_ctor_class_form(closure) } {
        return;
    }
    if let Some(name) = unsafe { crate::method_value::builtin_method_name(closure as *mut c_void) }
    {
        unsafe {
            put_bytes(b"[Function: ");
            put_bytes(name.as_bytes());
            put_bytes(b"]");
        }
    } else {
        // B6c — a class-method face prints its adapter's registry
        // row (the user-visible method name).
        let fn_addr = unsafe { crate::method_value_class::registry_addr(closure as *mut c_void) };
        unsafe { __torajs_fn_print_inline(fn_addr) };
    }
}
