//! The shared mid-dispatch chain entry for iterator-protocol cells —
//! split out of the parent when the §27.1.4.1 @@dispose arm pushed it
//! past the 500-line cap (rotation 342). A child module rather than a
//! sibling so the parent's private constants, externs and
//! `iter_helper_do_return` stay reachable with zero visibility
//! changes (the `gen_step.rs` convention).

use core::ffi::c_void;

use super::{
    __torajs_throw_type_error, ITER_HELPER_DROP, ITER_HELPER_FILTER, ITER_HELPER_FLAT_MAP,
    ITER_HELPER_MAP, ITER_HELPER_TAKE, RUNNING_OFF, iter_helper_do_return, iter_helper_mint,
};
use crate::iter_helper_eager::{iter_eager, iter_to_array};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};

/// Chained lazy-helper construction on any iterator-protocol cell
/// (`m.map(f).map(g)`, `[].values().map(f)`, …) — shared by the
/// IterHelper / MapIter / ArrIter dispatch arms. `None` = not a
/// helper mid (caller falls through to its own face).
///
/// # Safety
/// `ptr` is a live heap cell of the caller's tag; `argv` holds
/// `argc` boxed values.
pub(crate) unsafe fn try_helper_chain(
    ptr: *mut c_void,
    mid: i64,
    argv: *const AnyValue,
    argc: i64,
) -> Option<AnyValue> {
    let kind = match mid {
        torajs_rc::ANY_METHOD_MAP => ITER_HELPER_MAP,
        torajs_rc::ANY_METHOD_FILTER => ITER_HELPER_FILTER,
        torajs_rc::ANY_METHOD_FLAT_MAP => ITER_HELPER_FLAT_MAP,
        torajs_rc::any_method_iter::ANY_METHOD_TAKE => ITER_HELPER_TAKE,
        torajs_rc::any_method_iter::ANY_METHOD_DROP => ITER_HELPER_DROP,
        torajs_rc::any_method_iter::ANY_METHOD_TO_ARRAY => {
            return Some(unsafe { iter_to_array(ptr) });
        }
        // §27.1.2.1 %Iterator.prototype%[@@iterator] — return this
        // (the receiver box takes its own stake; borrow in, owned
        // out like every dispatch return).
        torajs_rc::any_method_iter::ANY_METHOD_ITER_SELF => {
            unsafe { torajs_rc::__torajs_rc_inc(ptr) };
            return Some(unsafe { crate::nanbox_encode::__torajs_anyv_box_pointer(ptr) });
        }
        // §27.1.4.1 %Iterator.prototype%[@@dispose] (RFC 20260809
        // B6) — GetMethod(this, "return"): the Map/Set/Array
        // iterator prototypes define no return (no-op); an Iterator
        // Helper's own return() closes the underlying, behind the
        // same executing gate its named spelling holds. The
        // return()'s iter-result is dropped — the spec ignores it
        // and answers undefined (a thrown close still propagates
        // through the pending-throw channel).
        torajs_rc::any_method_iter::ANY_METHOD_ITER_DISPOSE => {
            let tag = unsafe { (*(ptr as *const torajs_rc::HeapHeader)).type_tag };
            if tag == torajs_rc::Tag::IterHelper as u16 {
                if unsafe { (ptr.cast::<u8>().add(RUNNING_OFF)).read() } != 0 {
                    unsafe {
                        __torajs_throw_type_error(c"Iterator Helper is already running".as_ptr())
                    };
                    return Some(VALUE_UNDEFINED);
                }
                let res = unsafe { iter_helper_do_return(ptr) };
                unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(res) };
            }
            return Some(VALUE_UNDEFINED);
        }
        // 刀 3 — eager consumers (§27.1.4.5/.7/.8/.2/.12): drive to
        // exhaustion / short-circuit, closing the underlying on an
        // early exit.
        torajs_rc::ANY_METHOD_FOR_EACH
        | torajs_rc::ANY_METHOD_SOME
        | torajs_rc::ANY_METHOD_EVERY
        | torajs_rc::ANY_METHOD_FIND
        | torajs_rc::ANY_METHOD_REDUCE => {
            return Some(unsafe { iter_eager(ptr, mid, argv, argc) });
        }
        _ => return None,
    };
    let fn_av = if argc >= 1 {
        unsafe { argv.read() }
    } else {
        VALUE_UNDEFINED
    };
    // The receiver box is a borrow of the dispatcher's cell; mint
    // takes its own stake.
    Some(unsafe { iter_helper_mint(ptr as u64, kind, fn_av) })
}
