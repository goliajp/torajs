//! §10.5.12 [[Call]] and §10.5.13 [[Construct]] on a Proxy
//! (RFC 20260823-proxy-substrate 刀 6).
//!
//! A Proxy is callable only when its target is, and constructible
//! only when its target is — the exotic object gets those internal
//! methods at creation time, from the target's shape (§10.5.14 step
//! 3). So `typeof` on a proxy is `typeof` on its target, and
//! `is_constructor` likewise: the handler never gets to make a
//! non-function look like one.
//!
//! The `apply` trap takes `(target, thisArgument, argumentsList)`
//! and `construct` takes `(target, argumentsList, newTarget)` — both
//! receive the arguments as an ARRAY, which is why they are the two
//! traps that have to materialize one.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use crate::proxy::{live_slots, slots, trap};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// The target a Proxy's callability is read from — following a chain
/// of proxies down to the first non-proxy, since a proxy over a
/// proxy over a function is callable too.
///
/// # Safety
/// `v` is a valid AnyValue.
pub(crate) unsafe fn callable_target(v: AnyValue) -> AnyValue {
    let mut cur = v;
    for _ in 0..64 {
        if !crate::proxy::is_proxy(cur) {
            return cur;
        }
        // A revoked proxy has null slots — `null` is neither
        // callable nor a constructor, which is the right answer.
        let (target, _) = unsafe { slots(as_void_ptr(cur)) };
        cur = target;
    }
    VALUE_UNDEFINED
}

/// Materialize `argv` as the `Array<Any>` both traps take.
///
/// # Safety
/// `argv` points at `argc` live AnyValue slots.
unsafe fn args_array(argv: *const u64, argc: i64) -> *mut u8 {
    unsafe {
        let n = argc.max(0) as u64;
        let mut arr = __torajs_arr_alloc_any(n);
        for i in 0..n {
            let v = *argv.add(i as usize);
            let (tag, payload) = (
                crate::__torajs_anyv_unbox_tag(v),
                crate::__torajs_anyv_unbox_value(v),
            );
            crate::payload_rc_inc(tag, payload);
            arr = __torajs_arr_push_any(arr as *mut c_void, tag as u64, payload as u64);
        }
        arr
    }
}

/// §10.5.12 — `p(args…)` / `p.call(this, args…)`. Answers OWNED.
///
/// # Safety
/// `recv` is a live Proxy AnyValue; `argv` points at `argc` slots
/// alive across the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proxy_apply(
    recv: AnyValue,
    this_arg: AnyValue,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        let Ok((target, handler)) = live_slots(as_void_ptr(recv)) else {
            return VALUE_UNDEFINED;
        };
        let t = match trap(handler, b"apply") {
            Err(()) => return VALUE_UNDEFINED,
            Ok(None) => {
                return crate::method_call_closure_dispatch::__torajs_any_call_with_this(
                    target, this_arg, argv, argc,
                );
            }
            Ok(Some(t)) => t,
        };
        let list = args_array(argv, argc);
        let list_av = crate::nanbox::box_void_ptr(list as *mut c_void);
        let trap_argv = [target, this_arg, list_av];
        let out = crate::method_call_closure_dispatch::__torajs_any_call_with_this(
            t,
            handler,
            trap_argv.as_ptr(),
            3,
        );
        __torajs_anyv_rc_dec(t);
        __torajs_value_drop_heap(list as *mut c_void);
        if __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(out);
            return VALUE_UNDEFINED;
        }
        out
    }
}

/// §10.5.13 — `new p(args…)`. Answers OWNED; the trap must answer an
/// object (step 9).
///
/// # Safety
/// `recv` is a live Proxy AnyValue; `argv` points at `argc` slots
/// alive across the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proxy_construct(
    recv: AnyValue,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        let Ok((target, handler)) = live_slots(as_void_ptr(recv)) else {
            return VALUE_UNDEFINED;
        };
        let t = match trap(handler, b"construct") {
            Err(()) => return VALUE_UNDEFINED,
            Ok(None) => return crate::construct::__torajs_anyv_construct(target, argv, argc),
            Ok(Some(t)) => t,
        };
        let list = args_array(argv, argc);
        let list_av = crate::nanbox::box_void_ptr(list as *mut c_void);
        // newTarget is the proxy itself for a direct `new p(...)`.
        let trap_argv = [target, list_av, recv];
        let out = crate::method_call_closure_dispatch::__torajs_any_call_with_this(
            t,
            handler,
            trap_argv.as_ptr(),
            3,
        );
        __torajs_anyv_rc_dec(t);
        __torajs_value_drop_heap(list as *mut c_void);
        if __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(out);
            return VALUE_UNDEFINED;
        }
        if !crate::to_primitive::is_object_value(out) {
            __torajs_anyv_rc_dec(out);
            __torajs_throw_type_error(c"proxy 'construct' trap must return an object".as_ptr());
            return VALUE_UNDEFINED;
        }
        out
    }
}

/// Is this proxy callable? A proxy is callable exactly when the
/// target it wraps is (§10.5.14 step 3) — the handler has no say.
///
/// # Safety
/// `v` is a valid AnyValue.
pub(crate) unsafe fn proxy_is_callable(v: AnyValue) -> bool {
    let t = unsafe { callable_target(v) };
    if !is_cell(t) {
        return false;
    }
    unsafe { crate::method_call_closure_dispatch::closure_boxed_entry(t) }.is_some()
        || unsafe { as_void_ptr(t).cast::<u8>().add(4).cast::<u16>().read() }
            == torajs_rc::Tag::Closure as u16
}

/// §20.2.3.3 CreateListFromArrayLike then [[Call]] — the `apply`
/// spelling's argument list arrives as a value, not as a window.
///
/// # Safety
/// `recv` is a live Proxy AnyValue; `list` a valid AnyValue.
pub(crate) unsafe fn apply_with_list(
    recv: AnyValue,
    this_arg: AnyValue,
    list: AnyValue,
) -> AnyValue {
    unsafe {
        if crate::nanbox::is_undefined(list) || crate::nanbox::is_null(list) {
            return __torajs_proxy_apply(recv, this_arg, core::ptr::null(), 0);
        }
        let len_av = crate::len_get::__torajs_any_length_get(list);
        let len = crate::nanbox_ffi::__torajs_anyv_to_number(len_av);
        __torajs_anyv_rc_dec(len_av);
        let n = if len.is_finite() && len > 0.0 {
            len as usize
        } else {
            0
        };
        let mut buf: Vec<u64> = Vec::with_capacity(n);
        for i in 0..n {
            buf.push(crate::index_any::__torajs_any_index_get(list, i as i64));
        }
        let out = __torajs_proxy_apply(recv, this_arg, buf.as_ptr(), n as i64);
        for v in buf {
            __torajs_anyv_rc_dec(v);
        }
        out
    }
}
