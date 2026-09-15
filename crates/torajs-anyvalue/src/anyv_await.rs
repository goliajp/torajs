//! `await <any>` — §27.7.5.1 by-VALUE dispatch for the erased lane
//! (rotation 233), moved out of `method_call_promise` whole (rotation
//! 654, file-size): the then / catch / finally arm stays there, the
//! await kernel lives here on the same promise-cell layout.

use crate::method_call_promise::{
    P_REPR_OFF, P_STATE_OFF, REPR_UNSTAMPED, STATE_FULFILLED, box_settled,
};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};

unsafe extern "C" {
    fn __torajs_promise_get_value(p: *const c_void) -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}
use core::ffi::c_void;

/// `await <any>` — §27.7.5.1 by-VALUE dispatch for the erased lane
/// (rotation 233). The static tiers dispatch await by TYPE
/// (Promise(T) unwraps, everything else identity); an `any` operand
/// only knows its form at runtime. A heap Promise cell unwraps to
/// its settled value boxed per the cell's repr stamp, +1 stake — the
/// result is an independent binding, mirroring the typed lane's
/// rc_inc after `get_value` (box_settled is a borrow). A REJECTED
/// cell routes through `get_value`'s throw slot (the call site's
/// throw-check propagates); a still-PENDING cell answers undefined
/// (the sync-resolve model's guard, same as the typed read); an
/// UNSTAMPED fulfilled cell refuses loudly — never a silent mis-box.
/// Every other value — including a thenable struct (L3b) — passes
/// through identity WITH a fresh +1: the contract is "+1 stake on
/// every return path", and the lowering call site
/// (`lower_any_await`) releases an owned operand after this call on
/// that assumption. Pre-fix the identity arm returned the same cell
/// without the bump, so `await <owned non-promise cell>` freed the
/// operand's only stake and handed back a dangling pointer (async-gen
/// `yield*` step objects — rotation 288 heisenbug root cause). The
/// call site drains microtasks first, same as the typed await route.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_await(av: AnyValue) -> AnyValue {
    unsafe {
        if !crate::nanbox::is_cell(av) {
            return av;
        }
        let ptr = crate::nanbox::as_void_ptr(av);
        let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        if tag != torajs_rc::Tag::Promise as u16 {
            crate::nanbox_ffi::__torajs_anyv_rc_inc(av);
            return av;
        }
        let state = ptr.cast::<u8>().add(P_STATE_OFF).read();
        let repr = ptr.cast::<u8>().add(P_REPR_OFF).read();
        let value = __torajs_promise_get_value(ptr);
        if state != STATE_FULFILLED {
            // rejected: get_value set the throw slot; pending: the
            // sync-model undefined guard.
            return VALUE_UNDEFINED;
        }
        if repr == REPR_UNSTAMPED {
            __torajs_throw_type_error(
                c"promise value form unknown to `await` on this receiver".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let out = box_settled(repr, value);
        crate::nanbox_ffi::__torajs_anyv_rc_inc(out);
        out
    }
}
