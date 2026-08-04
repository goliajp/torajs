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
use crate::state::__torajs_promise_reject;

/// Bit 8 of the then/catch kernels' `ret_repr` parameter — set when
/// the handler's first parameter is `any` (dispatch boxes the settled
/// value per the source's repr stamp before the call). Low byte keeps
/// the return-repr code. Mirror:
/// `torajs-core/src/ssa_lower_promise_repr_mark.rs::PARAM_ANY_FLAG` —
/// must move in lockstep.
pub const PARAM_ANY_FLAG: i64 = 256;

/// Bits 16-23 of the same word — the handler's FIRST-PARAMETER repr,
/// for the mirror direction: a cell settled with an `any` handed to a
/// handler that declared a typed lane. Zero when the parameter is
/// `any` (see [`PARAM_ANY_FLAG`]) or a form with no repr code.
/// Mirror: `torajs-core/src/ssa_lower_promise_repr_mark.rs::
/// PARAM_REPR_SHIFT` — must move in lockstep.
pub const PARAM_REPR_SHIFT: i64 = 16;

/// AnySlotTag indices, as `__torajs_anyv_unbox_tag` reports them
/// (`torajs-anyvalue/src/nanbox_encode.rs`).
const TAG_BOOL: i64 = 1;
const TAG_I64: i64 = 2;
const TAG_F64: i64 = 3;

unsafe extern "C" {
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_box_str_slot(s: *mut c_void) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_take() -> i64;
    fn __torajs_throw_take_tag() -> i64;
}

/// §27.2.2.1 PromiseReactionJob steps 8-9 — a handler that completes
/// ABRUPTLY rejects the derived promise with the thrown value; it does
/// not fulfill it with the (absent) return. Answers `true` when it
/// settled `result`, so the caller skips its resolve leg and goes
/// straight to teardown.
///
/// Until this existed the typed kernels read the return register
/// regardless and resolved with whatever garbage it held, so the throw
/// stayed pending in the TLS with no live frame to check it: a
/// `.then(() => { throw e })` chain silently turned into a fulfillment
/// and every downstream `.catch` was dead code. The any-lane bridge
/// (torajs-anyvalue `method_call_promise.rs::then_any_dispatch`) has
/// always done this — the two lanes disagreeing is what let async
/// test262 cases whose assertion throws inside a `.then` handler report
/// success.
///
/// Reason encoding mirrors that bridge exactly: box the (tag, value)
/// pair off the throw TLS and stamp the cell `REPR_ANY`, so a
/// downstream handler's [`settle_param`] converts it into whatever lane
/// that handler declared. `value_is_heap` marks the boxed stake as the
/// cell's to release; the take composition (tag peeked before value —
/// `take` clears `active` but leaves the tag slot) transfers the
/// throw's stake into the box.
pub(crate) unsafe fn reject_on_pending_throw(result: *mut c_void) -> bool {
    unsafe {
        if __torajs_throw_check() == 0 {
            return false;
        }
        let tag = __torajs_throw_take_tag();
        let value = __torajs_throw_take();
        let reason = __torajs_anyv_box_from_pair(tag, value);
        let pp = as_promise(result);
        (*pp).value_is_heap = 1;
        (*pp).value_repr = REPR_ANY;
        __torajs_promise_reject(result, reason as i64);
        true
    }
}

/// A reaction handler's complete outcome protocol, in spec order: an
/// abrupt completion rejects (§27.2.2.1 steps 8-9), a returned promise
/// is ADOPTED rather than stored (§27.2.1.3.2), and anything else
/// fulfills the derived promise verbatim under the call site's static
/// return repr.
pub(crate) unsafe fn settle_handler_return(result: *mut c_void, ret_repr: u8, value: i64) {
    unsafe {
        if reject_on_pending_throw(result) {
            return;
        }
        if let Some(inner) = crate::then_adopt::returned_promise(ret_repr, value) {
            crate::then_adopt::adopt_into(result, inner);
            return;
        }
        (*as_promise(result)).value_repr = ret_repr;
        crate::state::__torajs_promise_resolve(result, value);
    }
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

/// Unbox a settled AnyValue into the raw form a TYPED handler
/// parameter expects — the mirror of [`box_settled`], and rc-neutral
/// for the same reason: the argument slot borrows the cell's stake,
/// exactly as a same-repr cell's value does today.
///
/// Without this the typed kernels handed the box POINTER straight to
/// the handler, so `Promise.resolve(x as any)` read back through
/// `(v: number)` bitcast a pointer into an f64 (NaN) and through
/// `(v: string)` dereferenced it as a Str (SIGSEGV).
///
/// A tag that disagrees with the declared lane means the annotation
/// lied — TS `any` type-checks that and hands the value over
/// unconverted, which a typed lane cannot represent. The nearest
/// answer it can hold is used (ToNumber-shaped for the number lane,
/// truthiness for the boolean one).
pub(crate) unsafe fn unbox_settled(param_repr: u8, av: i64) -> i64 {
    unsafe {
        let tag = __torajs_anyv_unbox_tag(av as u64);
        let raw = __torajs_anyv_unbox_value(av as u64);
        match param_repr {
            REPR_F64 => match tag {
                TAG_F64 => raw,
                TAG_I64 | TAG_BOOL => (raw as f64).to_bits() as i64,
                _ => f64::NAN.to_bits() as i64,
            },
            REPR_I64 => match tag {
                TAG_I64 | TAG_BOOL => raw,
                TAG_F64 => f64::from_bits(raw as u64) as i64,
                _ => 0,
            },
            REPR_BOOL => match tag {
                TAG_BOOL => raw,
                TAG_F64 => i64::from(f64::from_bits(raw as u64) != 0.0),
                _ => i64::from(raw != 0),
            },
            // The target lane IS `any` — hand the box back whole. The
            // callers all route any-parameters elsewhere, but a lane
            // that unwraps what it was asked to preserve is the exact
            // defect this function exists to fix.
            REPR_ANY => av,
            // A cell pointer is already the slot form both the STR and
            // HEAP lanes move values in.
            _ => raw,
        }
    }
}

/// The value a handler's first parameter receives: boxed when it
/// declared `any`, unboxed when the cell holds one but the handler
/// declared a typed lane, and untouched when the two already agree.
pub(crate) unsafe fn settle_param(
    src_repr: u8,
    param_any: bool,
    param_repr: u8,
    value: i64,
) -> i64 {
    unsafe {
        if param_any {
            box_settled(src_repr, value)
        } else if src_repr == REPR_ANY && param_repr != REPR_UNSTAMPED {
            unbox_settled(param_repr, value)
        } else {
            value
        }
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
