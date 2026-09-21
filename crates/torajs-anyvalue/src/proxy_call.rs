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
        // §7.3.31 step 1 — `p.#m()` finds nothing: a Proxy's own
        // private elements are always empty, and the target's must
        // never be reached through a trap. Private names only ever
        // arrive compiler-mangled under this prefix.
        if torajs_rc::str_wtf8::StrWtf8::of(name_str as *const c_void)
            .as_bytes()
            .starts_with(b"__priv_")
        {
            // Unless the proxy itself was adopted by a class that added
            // its brand to it (slice 2c): the method is then that
            // class's, called with the proxy as `this`.
            if let Some(v) =
                crate::private_foreign::foreign_private_call(recv, name_str, argv, argc)
            {
                return v;
            }
            __torajs_throw_type_error(c"cannot call private method on a Proxy".as_ptr());
            return VALUE_UNDEFINED;
        }
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
                if let Some(v) = user_method_on_proxy(recv, name_str, argv, argc) {
                    return v;
                }
                // §20.2.3.5 step 4 — the one method whose answer
                // turns on the RECEIVER's identity rather than the
                // resolved function's. Forwarding to the target (what
                // every other method does here, and what makes a
                // trap-less proxy cheap) would hand back the target's
                // source text; the receiver is the proxy, and a proxy
                // has no [[SourceText]]. `toLocaleString` arrives too
                // — §20.1.3.5 is `Invoke(O, "toString")`, so it asks
                // the same question about the same receiver.
                if (mid == torajs_rc::ANY_METHOD_TO_STRING
                    || mid == torajs_rc::ANY_METHOD_TO_LOCALE_STRING)
                    && let Some(v) = crate::proxy_callable::proxy_to_string_form(recv)
                {
                    return v;
                }
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

/// §7.3.14 Call(GetV(p, name), p) on a trap-less proxy when the value
/// is a USER function: its body must see the proxy as `this` (a class
/// method reading `this.#x` then fails the brand, as it must). `None`
/// keeps the whole-dispatch forward for everything else — builtin
/// method cells tag-dispatch on a real receiver, a class method face
/// without a receiver-polymorphic twin reads `this` at baked layout
/// offsets, and a non-callable value keeps the forward's miss shape.
///
/// # Safety
/// `recv` is a live Proxy AnyValue; `name_str` a live Str cell; `argv`
/// points at `argc` slots alive across the call.
unsafe fn user_method_on_proxy(
    recv: AnyValue,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    unsafe {
        let f = crate::proxy::get(as_void_ptr(recv), name_str as *const c_void, recv);
        if __torajs_throw_check() != 0 {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(f);
            return Some(VALUE_UNDEFINED);
        }
        let user = crate::method_call_closure_dispatch::closure_boxed_entry(f).is_some() && {
            let cell = as_void_ptr(f);
            crate::method_value::builtin_method_mid(cell).is_none()
                && (crate::method_value_class::class_method_face_adapter(cell).is_none()
                    || crate::method_value_class::class_method_face_guard(cell).1 != 0)
        };
        if !user {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(f);
            return None;
        }
        let out =
            crate::method_call_closure_dispatch::__torajs_any_call_with_this(f, recv, argv, argc);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(f);
        Some(out)
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
        // §20.2.3.2 step 4 — [[BoundTargetFunction]] is the value
        // `bind` was called ON, and here that value is the proxy.
        // Forwarding the dispatch to the target (the trap-less
        // default below) bound the target instead, and the bound
        // function then called it directly: every trap was gone, so
        // `p.bind(null, 5)()` on a proxy with an `apply` trap
        // answered undefined.
        if mid == torajs_rc::ANY_METHOD_BIND {
            return Some(crate::method_bind::bind_cell(as_void_ptr(recv), argv, argc));
        }
        None
    }
}
