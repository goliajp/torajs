//! ES §7.4.9 IteratorClose — the `return()` a consumer owes an
//! iterator it stops stepping early. Split from [`crate::iter_any`]
//! at the size cap; the body is that module's close family verbatim.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::iter_any::{MethodOutcome, SYM_ITERATOR_METHOD, call_obj_method_0};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_short_str};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;

unsafe extern "C" {
    /// torajs-throw — record a pending catchable TypeError; returns
    /// normally (caller's throw-check propagates).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_take() -> i64;
    fn __torajs_throw_take_tag() -> i64;
    fn __torajs_throw_set(tag: i64, value: i64);
}

/// §7.4.9 IteratorClose driven by an ABRUPT completion
/// (IfAbruptCloseIterator — the callback-threw arms of the lazy /
/// eager helpers). The pending throw must be STASHED before the
/// close: a live pending throw bounces every callee straight out of
/// its prologue check, so the underlying's own `return()` body never
/// ran and test262's closed-on-call-throws family observed
/// `closed === false`. Per step 6 the original completion wins — a
/// throw the close itself raises is swallowed (its heap value
/// released) and the original is restored.
///
/// # Safety
/// `iter` is `undefined` or a live AnyValue; a throw is pending.
pub(crate) unsafe fn iter_close_under_pending_throw(iter: AnyValue) {
    unsafe {
        let tag = __torajs_throw_take_tag();
        let val = __torajs_throw_take();
        __torajs_iter_close_value(iter);
        if __torajs_throw_check() != 0 {
            let ctag = __torajs_throw_take_tag();
            let cval = __torajs_throw_take();
            // box_from_pair re-encodes without a new stake, so this
            // dec releases the swallowed throw's own reference.
            __torajs_anyv_rc_dec(crate::__torajs_anyv_box_from_pair(ctag, cval));
        }
        __torajs_throw_set(tag, val);
    }
}

/// `__torajs_any_iter_close(recv, iter_slot)` — ES §7.4.9
/// IteratorClose. A consumer that stops before the iterator reports
/// done owes it a `return()` call: that is what runs a generator's
/// `finally` blocks and lets a custom iterator release what it holds.
/// Array / string / Map / Set iterators have no `return` method, so
/// closing one is a no-op — as it is for any iterator whose prototype
/// omits it (the spec returns early on an undefined `return`).
///
/// An `iter_slot` still at `undefined` means the consumer never took a
/// step (an empty pattern, `const [] = src`). ES §13.15.5.3 calls
/// GetIterator regardless, so this still rejects a non-iterable source
/// — otherwise `const [] = 5` would bind nothing and pass.
///
/// The slot's reference stays the caller's to release.
///
/// # Safety
/// `recv` is an AnyValue whose cells are live heap pointers;
/// `iter_slot` is a valid writable pointer holding `undefined` or a
/// derived iterator box.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_iter_close(recv: AnyValue, iter_slot: *mut AnyValue) {
    unsafe {
        if *iter_slot == VALUE_UNDEFINED && !derive_for_close(recv, iter_slot) {
            // An IterHelper is itself the iterator — a zero-derive
            // close still owes it the §27.1.5.2 return() (close the
            // underlying); borrow shape, the recv ref stays the
            // caller's (RFC 20260730-iterator-global 刀 2).
            if is_cell(recv)
                && (as_void_ptr(recv).cast::<u8>().add(4) as *const u16).read()
                    == Tag::IterHelper as u16
            {
                __torajs_iter_close_value(recv);
            }
            return;
        }
        __torajs_iter_close_value(*iter_slot);
    }
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
        let iter_ptr = as_void_ptr(iter) as *mut c_void;
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
            crate::iter_any_get_method::generic_iter_close(iter);
            return;
        }
        // A `return()` that throws propagates on a normal completion
        // (§7.4.9 step 6) — leave the throw in flight and touch nothing.
        match call_obj_method_0(iter_ptr, b"return") {
            MethodOutcome::Ok(result) => {
                __torajs_anyv_rc_dec(result);
            }
            MethodOutcome::Missing => crate::iter_any_get_method::generic_iter_close(iter),
            MethodOutcome::Threw => {}
        }
    }
}

/// The zero-step case of [`__torajs_any_iter_close`] — assert the
/// receiver is iterable, and derive the iterator only when it is a
/// class instance (the one shape with something to close). `false`
/// means either a pending TypeError or a lane with no closable
/// iterator; both leave the slot alone.
///
/// # Safety
/// Same as the caller's.
unsafe fn derive_for_close(recv: AnyValue, iter_slot: *mut AnyValue) -> bool {
    unsafe {
        // Same GetIterator the stepping path runs, and for the same
        // reason it runs first there: a user `@@iterator` outranks
        // every builtin lane, so a zero-step close over one must
        // derive (and close) THAT iterator.
        match crate::iter_any_get_method::get_iterator(recv) {
            crate::iter_any_get_method::GetIterator::Iterator(iter) => {
                *iter_slot = iter;
                return true;
            }
            crate::iter_any_get_method::GetIterator::Threw => return false,
            crate::iter_any_get_method::GetIterator::NoUserMethod => {}
        }
        if is_short_str(recv) {
            return false;
        }
        if !is_cell(recv) {
            __torajs_throw_type_error(c"value is not iterable".as_ptr());
            return false;
        }
        let tag = (as_void_ptr(recv).cast::<u8>().add(4) as *const u16).read();
        if tag == Tag::Str as u16
            || tag == Tag::Arr as u16
            || tag == Tag::Map as u16
            || tag == Tag::Set as u16
            || tag == Tag::MapIter as u16
            || tag == Tag::ArrIter as u16
            || tag == Tag::IterHelper as u16
        {
            return false;
        }
        if tag != Tag::Obj as u16 {
            __torajs_throw_type_error(c"value is not iterable".as_ptr());
            return false;
        }
        match call_obj_method_0(as_void_ptr(recv) as *mut c_void, SYM_ITERATOR_METHOD) {
            MethodOutcome::Ok(iter) => {
                *iter_slot = iter;
                true
            }
            MethodOutcome::Missing => {
                __torajs_throw_type_error(c"value is not iterable".as_ptr());
                false
            }
            // The user's `[Symbol.iterator]()` threw — forward it.
            MethodOutcome::Threw => false,
        }
    }
}
