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

unsafe extern "C" {
    /// torajs-fnname — the anonymous `[Function]` form on its own,
    /// for a face the fn-name table cannot answer for (564-01).
    fn __torajs_fn_print_anonymous();
}

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
        // 564-01 — a COMPUTED member has a `.name` (its runtime key,
        // §10.2.9) but no name in the SOURCE, and bun's inspect
        // reads the source: it prints the anonymous form.
        if unsafe {
            crate::method_value_class::class_method_runtime_name(closure as *mut c_void).is_some()
        } {
            unsafe { __torajs_fn_print_anonymous() };
            return;
        }
        // B6c — every other class-method face prints its adapter's
        // registry row (the user-visible method name).
        let fn_addr = unsafe { crate::method_value_class::registry_addr(closure as *mut c_void) };
        unsafe { __torajs_fn_print_inline(fn_addr) };
    }
}
