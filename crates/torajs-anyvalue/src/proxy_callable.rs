//! §10.5.12 [[Call]] and §10.5.13 [[Construct]] on a Proxy
//! (RFC 20260823-proxy-substrate 刀 6).
//!
//! A Proxy is callable only when its target is, and constructible
//! only when its target is — the exotic object gets those internal
//! methods at creation time, from the target's shape (§10.5.14 step
//! 7), and the header flags record them there. So `typeof` on a proxy
//! is `typeof` on its target at creation — revoked or not — and
//! `is_constructor` likewise: the handler never gets to make a
//! non-function look like one.
//!
//! The `apply` trap takes `(target, thisArgument, argumentsList)`
//! and `construct` takes `(target, argumentsList, newTarget)` — both
//! receive the arguments as an ARRAY, which is why they are the two
//! traps that have to materialize one.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use crate::proxy::{live_slots, trap};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_fn_native_form_str(name_ptr: *const u8, name_len: u32) -> *mut u8;
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
            // AnyValue-shaped inc (546-02): the pair-shaped
            // payload_rc_inc double-staked a ShortStr's materialized
            // Str (unbox_tag lies Heap); no-op inc + the push's
            // transfer balance the materialization.
            crate::nanbox_ffi::__torajs_anyv_rc_inc(v);
            let (tag, payload) = (
                crate::__torajs_anyv_unbox_tag(v),
                crate::__torajs_anyv_unbox_value(v),
            );
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
        let Ok(__s) = live_slots(as_void_ptr(recv)) else {
            return VALUE_UNDEFINED;
        };
        let (target, handler) = (__s.target, __s.handler);
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
    // newTarget is the proxy itself for a direct `new p(...)`.
    unsafe { construct_proxy(recv, argv, argc, recv) }
}

/// §10.5.13 with an explicit newTarget — the trap receives it, and a
/// trap-less forward hands it to `target.[[Construct]]`.
///
/// # Safety
/// `recv` is a live Proxy AnyValue; `new_target` a live constructor;
/// `argv` holds `argc` live AnyValues.
pub(crate) unsafe fn construct_proxy(
    recv: AnyValue,
    argv: *const u64,
    argc: i64,
    new_target: AnyValue,
) -> AnyValue {
    unsafe {
        let Ok(__s) = live_slots(as_void_ptr(recv)) else {
            return VALUE_UNDEFINED;
        };
        let (target, handler) = (__s.target, __s.handler);
        let t = match trap(handler, b"construct") {
            Err(()) => return VALUE_UNDEFINED,
            Ok(None) => {
                return crate::reflect_construct::construct_with_new_target(
                    target, argv, argc, new_target,
                );
            }
            Ok(Some(t)) => t,
        };
        let list = args_array(argv, argc);
        let list_av = crate::nanbox::box_void_ptr(list as *mut c_void);
        let trap_argv = [target, list_av, new_target];
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

/// Is this proxy callable? Decided at creation (§10.5.14 step 7) from
/// the target — the handler has no say, and a revoke does not undo it.
///
/// # Safety
/// `v` is a live Proxy AnyValue.
pub(crate) unsafe fn proxy_is_callable(v: AnyValue) -> bool {
    (unsafe { header_flags(v) } & torajs_rc::FLAG_PROXY_CALLABLE) != 0
}

/// Does `proxy` have [[Construct]]? Fixed at creation, like
/// [`proxy_is_callable`].
///
/// # Safety
/// `v` is a live Proxy AnyValue.
pub(crate) unsafe fn proxy_is_constructor(v: AnyValue) -> bool {
    (unsafe { header_flags(v) } & torajs_rc::FLAG_PROXY_CONSTRUCTOR) != 0
}

/// §20.2.3.5 step 4 — what `Function.prototype.toString` answers for
/// a Proxy receiver. A Proxy has no [[SourceText]] slot (step 2) and
/// is not a built-in function object (step 3), so a CALLABLE one
/// lands on step 4's "implementation-defined representation with the
/// syntax of a NativeFunction". JSC — so bun — spells it
/// `function ProxyObject() { [native code] }`.
///
/// `None` when `v` is not a callable proxy: a non-callable one is
/// step 5's TypeError through `Function.prototype.toString`, and
/// plain `String(p)` on it resolves `Object.prototype.toString`
/// instead. Either way the caller keeps the path it had.
///
/// The target's own source is the one answer that is WRONG here —
/// forwarding the dispatch to the target (which is what a trap-less
/// proxy does for every other method) hands back `function add(a,b)
/// {…}` / `class C{}`, and test262's `assertNativeFunction` reads
/// exactly that string against the NativeFunction grammar.
///
/// # Safety
/// `v` is a live AnyValue.
pub(crate) unsafe fn proxy_to_string_form(v: AnyValue) -> Option<AnyValue> {
    if !crate::proxy::is_proxy(v) || !unsafe { proxy_is_callable(v) } {
        return None;
    }
    const NAME: &[u8] = b"ProxyObject";
    let s = unsafe { __torajs_fn_native_form_str(NAME.as_ptr(), NAME.len() as u32) };
    Some(crate::nanbox::box_void_ptr(s as *mut c_void))
}

/// The proxy cell's header flags word.
unsafe fn header_flags(v: AnyValue) -> u16 {
    unsafe { as_void_ptr(v).cast::<u8>().add(6).cast::<u16>().read() }
}

// IsCallable over a proxy's would-be target is §7.2.3 like every
// other asking of it — `construct::is_callable`. A copy lived here
// and listed the closure shapes only, so `new Proxy(class C {}, {})`
// recorded itself non-callable and `typeof` answered "object".

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
