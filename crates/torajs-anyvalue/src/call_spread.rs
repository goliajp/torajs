//! Spread-call kernels — §13.3.8.1 ArgumentListEvaluation for a
//! call whose argument list carries a dynamic `...spread` (rotation
//! 372). The lowering has already materialized the FULL argument
//! list into one `Array<Any>` (literal args pushed, spread sources
//! walked through the unified iteration protocol), so argc is a
//! runtime fact these kernels read off the array; the existing
//! `__torajs_any_call` / `__torajs_any_method_call` take a
//! compile-time argc and cannot express it.
//!
//! Each kernel converts the array's 16-byte tag/value slots into
//! the 8-byte NaN-box argv the dispatch layer speaks
//! (`__torajs_arr_get_any_boxed` — a BORROW per slot, the array
//! keeps every element's stake alive across the call) and enters
//! the same dispatch tail as its fixed-argc twin: the closure boxed
//! dual entry for a bare call, `any_method_call_inner` + the
//! builtin-proto patch consult for a method call. argv slots stay
//! borrowed; the caller drops the args array after the call, which
//! releases the elements.

use core::ffi::c_void;

use crate::method_call::{ANY_METHOD_NO_SUCH, any_method_call_inner, not_callable};
use crate::method_call_closure_dispatch::{MAX_BOXED_ARGS, closure_boxed_entry, invoke_with_this};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};

unsafe extern "C" {
    /// torajs-arr — element `i` of any array shape as a BORROWED
    /// AnyValue (typed slots bridge-encode; out-of-range answers
    /// undefined).
    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
}

/// Read the args array into an argv buffer and hand it to `f`.
/// Stack buffer up to [`MAX_BOXED_ARGS`], heap beyond (the
/// `invoke_boxed_recv_first` shape).
unsafe fn with_argv<F: FnOnce(*const u64, i64) -> AnyValue>(
    args_arr: *const c_void,
    f: F,
) -> AnyValue {
    unsafe {
        let argc = (args_arr
            .cast::<u8>()
            .add(crate::index_any::MIRROR_ARR_LEN_OFF)
            .cast::<u64>())
        .read() as i64;
        let n = argc as usize;
        if n > MAX_BOXED_ARGS {
            let mut big = vec![VALUE_UNDEFINED; n];
            for (i, slot) in big.iter_mut().enumerate() {
                *slot = __torajs_arr_get_any_boxed(args_arr, i as u64);
            }
            return f(big.as_ptr(), argc);
        }
        let mut buf = [VALUE_UNDEFINED; MAX_BOXED_ARGS];
        for (i, slot) in buf.iter_mut().enumerate().take(n) {
            *slot = __torajs_arr_get_any_boxed(args_arr, i as u64);
        }
        f(buf.as_ptr(), argc)
    }
}

/// `f(a, ...xs)` — the bare-callee flavor. Same verdict split as
/// [`crate::method_call_closure_dispatch::__torajs_any_call`]: a
/// closure cell with a boxed dual entry invokes (thisArg undefined
/// per §10.2.1.2 strict OrdinaryCallBindThis), anything else is a
/// catchable TypeError.
///
/// # Safety
/// `recv` is a valid AnyValue; `args_arr` is a live `Array<Any>`
/// the caller keeps alive across the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_call_spread(
    recv: AnyValue,
    args_arr: *const c_void,
) -> AnyValue {
    unsafe {
        if let Some((env, entry)) = closure_boxed_entry(recv) {
            return with_argv(args_arr, |argv, argc| {
                invoke_with_this(env, entry, VALUE_UNDEFINED, argv, argc)
            });
        }
        not_callable()
    }
}

/// `new callee(a, ...xs)` where `callee` is a runtime value — the
/// spread flavor of [`crate::construct::__torajs_anyv_construct`]
/// (§13.3.5.1 EvaluateNew with a spread-carrying Arguments). Same
/// verdict split: the §7.2.4 IsConstructor gate and the base-kind /
/// bound-function construct paths all live in the fixed-argc twin
/// this enters.
///
/// # Safety
/// `callee` is a valid AnyValue; `args_arr` is a live `Array<Any>`
/// the caller keeps alive across the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_construct_spread(
    callee: AnyValue,
    args_arr: *const c_void,
) -> AnyValue {
    unsafe {
        with_argv(args_arr, |argv, argc| {
            crate::construct::__torajs_anyv_construct(callee, argv, argc)
        })
    }
}

/// `super.m(a, ...xs)` in a builtin-heritage subclass method — the
/// spread flavor of
/// [`crate::method_call_subclass::__torajs_super_builtin_method`]
/// (rotation 372; `super.has(...rest)` is the t262
/// subclass-receiver-methods forwarding idiom). Same §13.3.7.3
/// contract: parent-prototype resolution, the receiver's own
/// override is never consulted.
///
/// # Safety
/// `recv` is a live AnyValue; `args_arr` is a live `Array<Any>`
/// the caller keeps alive across the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_super_builtin_method_spread(
    recv: AnyValue,
    mid: i64,
    args_arr: *const c_void,
) -> AnyValue {
    unsafe {
        with_argv(args_arr, |argv, argc| {
            crate::method_call_subclass::__torajs_super_builtin_method(recv, mid, argv, argc)
        })
    }
}

/// `recv.name(a, ...xs)` — the method flavor. Same dispatch tail as
/// [`crate::method_call::__torajs_any_method_call`]: the shared
/// inner, then the builtin-proto patch consult on a mid-miss, then
/// the TypeError.
///
/// # Safety
/// Same contract as `__torajs_any_method_call`; `args_arr` is a
/// live `Array<Any>` the caller keeps alive across the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_method_call_spread(
    recv: AnyValue,
    mid: i64,
    name_str: *const u8,
    _name_len: i64,
    recv_slot: *mut u64,
    args_arr: *const c_void,
) -> AnyValue {
    unsafe {
        with_argv(args_arr, |argv, argc| {
            let r = any_method_call_inner(recv, mid, name_str, recv_slot, argv, argc);
            if r == ANY_METHOD_NO_SUCH {
                if let Some(out) = crate::method_call_proto_patch::builtin_proto_patch_method(
                    recv, mid, name_str, argv, argc,
                ) {
                    return out;
                }
                return not_callable();
            }
            r
        })
    }
}
