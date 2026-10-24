//! §7.4.9 IteratorClose over an iterator the caller already holds:
//! the `return()` call, through the vtable tier a class instance
//! answers or the generic member read every other cell takes. Split
//! from [`crate::iter_any_close`] and [`crate::iter_any_get_method`]
//! at the size cap; the bodies are theirs verbatim.

use torajs_rc::Tag;

use crate::iter_any_call::{MethodOutcome, call_obj_method_0};
use crate::iter_any_close::reject_non_object_close_result;
use crate::method_call_closure_dispatch::invoke_with_this;
use crate::nanbox::{AnyValue, VALUE_NULL, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
}

/// `__torajs_iter_close_value(iter)` — §7.4.9 over an iterator the
/// caller already holds, without the slot / GetIterator bookkeeping.
///
/// The statically-typed class-iterator lowering keeps its iterator in
/// a typed local rather than an `Any` park slot, so it closes through
/// this face; the slot-shaped extern above resolves its slot and then
/// lands here, which is what keeps one copy of the cascade.
///
/// The reference stays the caller's — this only borrows it.
///
/// # Safety
/// `iter` is `undefined` or a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_iter_close_value(iter: AnyValue) {
    unsafe {
        if !is_cell(iter) {
            return;
        }
        let iter_ptr = as_void_ptr(iter);
        let tag = (iter_ptr.cast::<u8>().add(4) as *const u16).read();
        // An IterHelper closes through its own §27.1.5.2 return()
        // arm directly — the generic tier below resolves `return`
        // through the member-VALUE face, which the helper cell does
        // not carry (RFC 20260730-iterator-global 刀 2; the reified
        // method-value face is the 刀 2b supported-table work).
        if tag == Tag::IterHelper as u16 {
            let r = crate::iter_helper::iter_helper_method(
                iter_ptr,
                torajs_rc::any_method::ANY_METHOD_ITER_RETURN,
                core::ptr::null(),
                0,
            );
            __torajs_anyv_rc_dec(r);
            return;
        }
        // Same tier cascade the step takes, for the same reason: an
        // anon-stamped literal wears `Tag::Obj` but keeps `return` as
        // a field, so a vtable miss is a tier hand-off, not "no
        // return method".
        if tag != Tag::Obj as u16 {
            generic_iter_close(iter);
            return;
        }
        // A `return()` that throws propagates on a normal completion
        // (§7.4.9 step 6) — leave the throw in flight and touch nothing.
        match call_obj_method_0(iter_ptr, b"return") {
            MethodOutcome::Ok(result) => reject_non_object_close_result(result),
            MethodOutcome::Missing => generic_iter_close(iter),
            MethodOutcome::Threw => {}
        }
    }
}

/// §7.4.9 IteratorClose for an iterator whose `return` is an ordinary
/// property rather than a vtable method.
///
/// An iterator with no `return` closes silently (step 4), while one
/// whose `return` is not callable throws. A `return`
/// that throws leaves its throw in flight — on a normal completion
/// §7.4.9 step 6 propagates it, and touching the result would be
/// reading an aborted call's sentinel.
///
/// # Safety
/// `iter` is a live cell.
pub(crate) unsafe fn generic_iter_close(iter: AnyValue) {
    unsafe {
        // GetMethod is a [[Get]]: an accessor `return` (a class's `get
        // return()`) runs its getter, whose throw is the close's
        // completion (§7.4.9 step 5); the read answers an owned value
        let Some(method) =
            crate::iter_any_result::iter_result_get(iter, as_void_ptr(iter), b"return")
        else {
            return;
        };
        // §7.4.9 step 4 — GetMethod answering undefined ends the
        // close; an iterator without a `return` is closed already.
        if method == VALUE_UNDEFINED || method == VALUE_NULL {
            return;
        }
        let result = {
            let Some(m) = crate::callback_resolve::resolve(method, iter) else {
                __torajs_anyv_rc_dec(method);
                __torajs_throw_type_error(c"return is not a function".as_ptr());
                return;
            };
            let argv: [u64; 0] = [];
            invoke_with_this(m.env(), m.entry(), iter, argv.as_ptr(), 0)
        };
        __torajs_anyv_rc_dec(method);
        if __torajs_throw_check() == 0 {
            // §7.4.6 step 9 — non-Object answer → TypeError (the
            // shared reject; original-throw precedence is the abrupt
            // wrapper's job, same as the Tag::Obj arm).
            crate::iter_any_close::reject_non_object_close_result(result);
        }
    }
}
