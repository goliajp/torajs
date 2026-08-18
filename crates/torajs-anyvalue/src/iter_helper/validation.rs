//! §27.1.4 helper-argument validation faces — the IfAbruptCloseIterator
//! close and the %Iterator.prototype% ownership predicate. A child
//! module of `iter_helper` under the 500-line file rule (the parent
//! sat at 536 with both inline); it reaches the parent's private
//! items directly, zero visibility changes on that side.

use core::ffi::c_void;

use crate::nanbox::AnyValue;

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_take() -> i64;
    fn __torajs_throw_take_tag() -> i64;
    fn __torajs_throw_set(tag: i64, value: i64);
}

/// §7.4.9 IteratorClose on a validation abrupt (IfAbruptCloseIterator)
/// — GetMethod(recv, "return"), call it, and make the ORIGINAL abrupt
/// win: an abrupt already in flight (a ToNumber poison) is preserved
/// across the close, and any throw the close itself raises is
/// discarded (step 5). The caller lands its own error after this
/// returns when nothing was in flight.
pub(super) unsafe fn close_on_validation_abrupt(recv: AnyValue) {
    unsafe {
        let saved = if __torajs_throw_check() != 0 {
            let t = __torajs_throw_take_tag();
            let v = __torajs_throw_take();
            Some((t, v))
        } else {
            None
        };
        // GetMethod step 3 — absent (or present-but-undefined)
        // answers undefined with no call, same probe-first order as
        // the @@dispose entry.
        let name = crate::closure_proto::iter_proto::return_name_cell();
        let tag = crate::member_get::__torajs_any_member_get_tag(recv, name.cast());
        if __torajs_throw_check() == 0 && tag != 5 {
            let r = crate::method_call::any_method_call_inner(
                recv,
                torajs_rc::any_method::ANY_METHOD_ITER_RETURN,
                name,
                core::ptr::null_mut(),
                core::ptr::null(),
                0,
            );
            if r != crate::method_call::ANY_METHOD_NO_SUCH {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(r);
            }
        }
        // Discard the close's own throw — the original abrupt wins.
        if __torajs_throw_check() != 0 {
            let ct = __torajs_throw_take_tag();
            let cv = __torajs_throw_take();
            if ct == 4 && cv != 0 {
                torajs_rc::__torajs_rc_dec(cv as usize as *mut c_void);
            }
        }
        if let Some((t, v)) = saved {
            __torajs_throw_set(t, v);
        }
    }
}

/// The method ids %Iterator.prototype% owns (§27.1.4 — the lazy
/// helpers and the eager consumers). One predicate feeds both the
/// proto-tag ownership table (`method_support_proto/owns.rs`, so the
/// reflection surface hands out cells) and the dynobj chain
/// re-dispatch (`method_call_dynobj_chain.rs`, so a resolved tag-15
/// cell routes the CHILD receiver into `try_helper_chain` instead of
/// looping back through the generic dispatch).
pub(crate) fn iter_proto_owns_mid(mid: i64) -> bool {
    matches!(
        mid,
        torajs_rc::ANY_METHOD_MAP
            | torajs_rc::ANY_METHOD_FILTER
            | torajs_rc::ANY_METHOD_FLAT_MAP
            | torajs_rc::ANY_METHOD_FOR_EACH
            | torajs_rc::ANY_METHOD_SOME
            | torajs_rc::ANY_METHOD_EVERY
            | torajs_rc::ANY_METHOD_FIND
            | torajs_rc::ANY_METHOD_REDUCE
    ) || matches!(
        mid,
        torajs_rc::any_method_iter::ANY_METHOD_TAKE
            | torajs_rc::any_method_iter::ANY_METHOD_DROP
            | torajs_rc::any_method_iter::ANY_METHOD_TO_ARRAY
    )
}
