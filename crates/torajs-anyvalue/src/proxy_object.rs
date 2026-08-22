//! The four object-level internal methods on a Proxy —
//! §10.5.1 [[GetPrototypeOf]], §10.5.2 [[SetPrototypeOf]],
//! §10.5.3 [[IsExtensible]], §10.5.4 [[PreventExtensions]]
//! (RFC 20260823-proxy-substrate 刀 5).
//!
//! These ask about the object itself rather than one of its
//! properties, so their traps take only the target (plus the
//! prototype, for the setter). Otherwise the shape is the one every
//! other internal method has: trap or forward, ToBoolean the answer
//! where the spec says boolean.
//!
//! §10.5.3 carries the one invariant that does NOT need the target's
//! own descriptors, so it is here rather than deferred: the
//! `isExtensible` trap must agree with the target
//! (`IsExtensible(target)`), and a disagreement is a TypeError. It
//! is the cheapest of the §10.5.x invariants and the one a handler
//! is most likely to get wrong.

use crate::nanbox::{AnyValue, VALUE_NULL, as_void_ptr, is_null};
use crate::nanbox_ffi::{__torajs_anyv_rc_dec, __torajs_anyv_to_bool};
use crate::proxy::{live_slots, trap};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    /// torajs-meta — the ordinary forms these forward to.
    fn __torajs_anyv_get_proto_of_any(v: u64) -> u64;
    fn __torajs_reflect_set_prototype_of(obj: u64, proto: u64) -> i64;
    fn __torajs_anyv_is_extensible(obj_any: u64) -> bool;
    fn __torajs_anyv_prevent_extensions(obj_any: u64) -> u64;
}

/// Call a trap that takes only `(target)` and answer its result
/// OWNED. Err = pending throw.
///
/// # Safety
/// `t` is an owned callable; `handler` / `target` are live.
unsafe fn call_target_trap(
    t: AnyValue,
    handler: AnyValue,
    target: AnyValue,
) -> Result<AnyValue, ()> {
    unsafe {
        let argv = [target];
        let out = crate::method_call_closure_dispatch::__torajs_any_call_with_this(
            t,
            handler,
            argv.as_ptr(),
            1,
        );
        __torajs_anyv_rc_dec(t);
        if __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(out);
            return Err(());
        }
        Ok(out)
    }
}

/// §10.5.1. Answers an OWNED prototype value (`null` for none).
///
/// # Safety
/// `recv` is a live Proxy AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proxy_get_prototype_of(recv: AnyValue) -> AnyValue {
    unsafe {
        let Ok(__s) = live_slots(as_void_ptr(recv)) else {
            return VALUE_NULL;
        };
        let (target, handler) = (__s.target, __s.handler);
        let Ok(t) = trap(handler, b"getPrototypeOf") else {
            return VALUE_NULL;
        };
        let Some(t) = t else {
            return __torajs_anyv_get_proto_of_any(target);
        };
        let Ok(out) = call_target_trap(t, handler, target) else {
            return VALUE_NULL;
        };
        // §10.5.1 step 7 — the trap answers an Object or null.
        if is_null(out) || crate::to_primitive::is_object_value(out) {
            return out;
        }
        __torajs_anyv_rc_dec(out);
        __torajs_throw_type_error(
            c"proxy 'getPrototypeOf' trap must return an object or null".as_ptr(),
        );
        VALUE_NULL
    }
}

/// §10.5.2. Answers 1 when the re-parent was accepted, 0 on a
/// refusal the caller turns into its own flavor of "no".
///
/// # Safety
/// `recv` is a live Proxy AnyValue; `proto` a valid AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proxy_set_prototype_of(recv: AnyValue, proto: AnyValue) -> i64 {
    unsafe {
        let Ok(__s) = live_slots(as_void_ptr(recv)) else {
            return 0;
        };
        let (target, handler) = (__s.target, __s.handler);
        let Ok(t) = trap(handler, b"setPrototypeOf") else {
            return 0;
        };
        let Some(t) = t else {
            return __torajs_reflect_set_prototype_of(target, proto);
        };
        let argv = [target, proto];
        let out = crate::method_call_closure_dispatch::__torajs_any_call_with_this(
            t,
            handler,
            argv.as_ptr(),
            2,
        );
        __torajs_anyv_rc_dec(t);
        if __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(out);
            return 0;
        }
        let b = __torajs_anyv_to_bool(out);
        __torajs_anyv_rc_dec(out);
        b as i64
    }
}

/// §10.5.3. The trap's answer must agree with the target's real
/// extensibility (step 8) — a disagreement is a TypeError, not a
/// silently honored lie.
///
/// # Safety
/// `recv` is a live Proxy AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proxy_is_extensible(recv: AnyValue) -> bool {
    unsafe {
        let Ok(__s) = live_slots(as_void_ptr(recv)) else {
            return false;
        };
        let (target, handler) = (__s.target, __s.handler);
        let Ok(t) = trap(handler, b"isExtensible") else {
            return false;
        };
        let Some(t) = t else {
            return __torajs_anyv_is_extensible(target);
        };
        let Ok(out) = call_target_trap(t, handler, target) else {
            return false;
        };
        let b = __torajs_anyv_to_bool(out);
        __torajs_anyv_rc_dec(out);
        let real = __torajs_anyv_is_extensible(target);
        if __torajs_throw_check() != 0 {
            return false;
        }
        if b != real {
            __torajs_throw_type_error(
                c"proxy 'isExtensible' trap disagrees with the target".as_ptr(),
            );
            return false;
        }
        b
    }
}

/// §10.5.4. Answers 1 on acceptance, 0 on refusal.
///
/// # Safety
/// `recv` is a live Proxy AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proxy_prevent_extensions(recv: AnyValue) -> i64 {
    unsafe {
        let Ok(__s) = live_slots(as_void_ptr(recv)) else {
            return 0;
        };
        let (target, handler) = (__s.target, __s.handler);
        let Ok(t) = trap(handler, b"preventExtensions") else {
            return 0;
        };
        let Some(t) = t else {
            __torajs_anyv_prevent_extensions(target);
            return 1;
        };
        let Ok(out) = call_target_trap(t, handler, target) else {
            return 0;
        };
        let b = __torajs_anyv_to_bool(out);
        __torajs_anyv_rc_dec(out);
        if !b {
            return 0;
        }
        // §10.5.4 step 8 — a trap that reports success while the
        // target is still extensible is a TypeError.
        if __torajs_anyv_is_extensible(target) {
            __torajs_throw_type_error(
                c"proxy 'preventExtensions' trap returned true for an extensible target".as_ptr(),
            );
            return 0;
        }
        1
    }
}
