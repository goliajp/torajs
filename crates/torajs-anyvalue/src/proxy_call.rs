//! `proxy.m(args…)` — the §7.3.2 GetV + §7.3.14 Call composition a
//! method call on a Proxy receiver actually is
//! (RFC 20260823-proxy-substrate 刀 1).
//!
//! A method call is not its own internal method. `p.m()` is
//! `Call(GetV(p, "m"), p, args)`, so on a Proxy receiver the NAME
//! lookup goes through [[Get]] — the `get` trap, or the target's
//! own read when the handler has no trap — and whatever comes back
//! is invoked with the **proxy** as `this`.
//!
//! This is why it cannot ride the ordinary tag dispatch: that
//! resolves a receiver's method by the receiver's SHAPE, and a
//! proxy's shape says nothing about what its handler will answer.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
}

/// The whole call. `name_str` is the method-name Str cell (the
/// dispatcher's key face); `argv` is borrowed.
///
/// **Trap-less proxies forward the whole dispatch to the target.**
/// Not for convenience: the read a trap-less proxy performs answers
/// the target's property verbatim, and for a builtin that property
/// is a REIFIED cell whose body is tag-dispatched on a real receiver
/// — `Array.prototype.join` invoked with a proxy `this` has nothing
/// to read. Forwarding preserves every builtin, at the cost of one
/// observable: `this` inside the callee is the target rather than
/// the proxy, which only an identity comparison can see. Making it
/// exact needs array-like-generic builtin bodies (they would go
/// through [[Get]] like §23.1.3.16 says) — a knife of its own.
///
/// # Safety
/// `recv` is a live Proxy AnyValue; `name_str` a live Str cell;
/// `argv` points at `argc` slots alive across the call.
pub(crate) unsafe fn method_call(
    recv: AnyValue,
    mid: i64,
    name_str: *const u8,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        // `p.call(t, …)` / `p.apply(t, list)` on a CALLABLE proxy is
        // still a [[Call]] on the proxy — §20.2.3.3 passes its own
        // receiver through as the thisArgument, so forwarding the
        // dispatch to the target would skip the `apply` trap and
        // hand the target itself the call.
        if let Some(v) = callable_forwarder(recv, mid, argv, argc) {
            return v;
        }
        let Ok(__s) = crate::proxy::live_slots(as_void_ptr(recv)) else {
            return VALUE_UNDEFINED;
        };
        let (target, handler) = (__s.target, __s.handler);
        match crate::proxy::trap(handler, b"get") {
            Err(()) => return VALUE_UNDEFINED,
            Ok(None) => {
                return crate::method_call::any_method_call_inner(
                    target, mid, name_str, recv_slot, argv, argc,
                );
            }
            Ok(Some(t)) => crate::nanbox_ffi::__torajs_anyv_rc_dec(t),
        }
        let f = crate::proxy::get(as_void_ptr(recv), name_str as *const c_void, recv);
        if __torajs_throw_check() != 0 {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(f);
            return VALUE_UNDEFINED;
        }
        if crate::method_call_closure_dispatch::closure_boxed_entry(f).is_none() {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(f);
            __torajs_throw_type_error(c"proxy property is not a function".as_ptr());
            return VALUE_UNDEFINED;
        }
        let out =
            crate::method_call_closure_dispatch::__torajs_any_call_with_this(f, recv, argv, argc);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(f);
        out
    }
}

/// §20.2.3.1 / §20.2.3.3 over a callable Proxy receiver — the two
/// spellings that re-enter [[Call]] with an explicit thisArgument.
/// `None` = not one of them (or the proxy is not callable), and the
/// ordinary name lookup runs.
///
/// # Safety
/// `recv` is a live Proxy AnyValue; `argv` points at `argc` slots
/// alive across the call.
unsafe fn callable_forwarder(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    unsafe {
        if !crate::proxy_callable::proxy_is_callable(recv) {
            return None;
        }
        let this_arg = if argc >= 1 { *argv } else { VALUE_UNDEFINED };
        if mid == torajs_rc::ANY_METHOD_CALL {
            let (rest, n) = if argc <= 1 {
                (argv, 0)
            } else {
                (argv.add(1), argc - 1)
            };
            return Some(crate::proxy_callable::__torajs_proxy_apply(
                recv, this_arg, rest, n,
            ));
        }
        if mid == torajs_rc::ANY_METHOD_APPLY {
            let list = if argc >= 2 {
                *argv.add(1)
            } else {
                VALUE_UNDEFINED
            };
            return Some(crate::proxy_callable::apply_with_list(recv, this_arg, list));
        }
        None
    }
}
