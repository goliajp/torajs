//! Any-param callback support for the typed `.then` / `.catch`
//! kernels (RFC 20260720-promise-any-cb knife 1).
//!
//! A `(v: any) => R` handler's body expects its argument as NaN-box
//! AnyValue bits, but the typed kernels move the settled value as raw
//! i64. The call site cannot box statically — the SSA layer's
//! `Type::Promise` is inner-T erased — so the kernel boxes at
//! dispatch time from the cell's `value_repr` stamp (the same truth
//! source the any-lane bridge reads). The lowering marks such
//! handlers by setting [`PARAM_ANY_FLAG`] on the kernels' `ret_repr`
//! parameter; low byte stays the return-repr code.
//!
//! An UNSTAMPED source (a mint family without repr wiring) refuses
//! LOUDLY at attach time — never a silent mis-box.

use core::ffi::c_void;

use crate::layout::{
    REPR_ANY, REPR_BOOL, REPR_F64, REPR_HEAP, REPR_I64, REPR_NULL, REPR_STR, REPR_UNSTAMPED,
    REPR_VOID, as_promise,
};

/// Bit 8 of the then/catch kernels' `ret_repr` parameter — set when
/// the handler's first parameter is `any` (dispatch boxes the settled
/// value per the source's repr stamp before the call). Low byte keeps
/// the return-repr code. Mirror:
/// `torajs-core/src/ssa_lower_promise_repr_mark.rs::PARAM_ANY_FLAG` —
/// must move in lockstep.
pub const PARAM_ANY_FLAG: i64 = 256;

unsafe extern "C" {
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_box_str_slot(s: *mut c_void) -> u64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Box the source's settled value per its repr stamp — mirror of the
/// any-lane bridge's `box_settled` (`torajs-anyvalue/src/
/// method_call_promise.rs`, must move in lockstep). Rc-neutral: the
/// box is a borrow of the cell's stake for the callback's argument
/// slot (the STR arm's helper materializes/incs internally, same
/// contract as the any-lane caller).
pub(crate) unsafe fn box_settled(repr: u8, value: i64) -> i64 {
    unsafe {
        (match repr {
            REPR_I64 => __torajs_anyv_box_from_pair(2, value),
            REPR_F64 => __torajs_anyv_box_from_pair(3, value),
            REPR_BOOL => __torajs_anyv_box_from_pair(1, value),
            REPR_STR => __torajs_anyv_box_str_slot(value as *mut c_void),
            REPR_HEAP => __torajs_anyv_box_from_pair(4, value),
            REPR_ANY => value as u64,
            REPR_VOID => __torajs_anyv_box_from_pair(5, 0),
            REPR_NULL => __torajs_anyv_box_from_pair(0, 0),
            // unreachable — the attach-time gate refused UNSTAMPED.
            _ => __torajs_anyv_box_from_pair(5, 0),
        }) as i64
    }
}

/// Attach-time gate for an any-param handler: an UNSTAMPED source has
/// no repr wiring, so dispatch could only mis-box. Throw before any
/// callback runs; the call site's throw-check propagates it. Returns
/// `true` when the attach must be refused.
pub(crate) unsafe fn refuse_unstamped(source: *mut c_void) -> bool {
    unsafe {
        if (*as_promise(source)).value_repr == REPR_UNSTAMPED {
            __torajs_throw_type_error(
                c"promise value form unknown to the any-param handler on this receiver".as_ptr(),
            );
            return true;
        }
    }
    false
}
