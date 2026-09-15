//! §10.5.5 [[GetOwnProperty]] on a Proxy, and the value-producing
//! reflection faces composed over it (RFC 20260823-proxy-substrate 刀 4).
//!
//! `getOwnPropertyDescriptor` and every enumerability question read
//! through [[GetOwnProperty]]; `Object.values` / `Object.entries`
//! compose it with [[OwnPropertyKeys]] (`proxy_own_keys.rs`) and
//! [[Get]], per §7.3.24 EnumerableOwnProperties.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, box_void_ptr};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use crate::proxy::{live_slots, trap};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    /// torajs-meta — the ordinary reflection surface, used for the
    /// trap-less forward.
    fn __torajs_anyv_get_property_descriptor(obj_any: u64, key: *const c_void) -> u64;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// `Arr` header mirrors (torajs-arr layout): `len` u64 at +8,
/// element storage pointer at +32.
const ARR_LEN_OFF: usize = 8;
const ARR_DATA_PTR_OFF: usize = 32;
/// A one-level heap element-kind chain (`obj_own_values` twin).
const KIND_CHAIN_HEAP: u64 = 4;
/// dynobj / arr bucket tag for a heap payload.
const ANY_HEAP_TAG: u64 = 4;

/// §10.5.5 [[GetOwnProperty]] — the `getOwnPropertyDescriptor` trap
/// or the target's own descriptor. Answers an OWNED descriptor
/// object, or `undefined` for an absent property.
///
/// # Safety
/// `recv` is a live Proxy AnyValue; `key` a live Str or Symbol cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proxy_get_own_descriptor(
    recv: AnyValue,
    key: *const c_void,
) -> AnyValue {
    unsafe {
        let Ok(__s) = live_slots(as_void_ptr(recv)) else {
            return VALUE_UNDEFINED;
        };
        let (target, handler) = (__s.target, __s.handler);
        let t = match trap(handler, b"getOwnPropertyDescriptor") {
            Err(()) => return VALUE_UNDEFINED,
            Ok(None) => return __torajs_anyv_get_property_descriptor(target, key),
            Ok(Some(t)) => t,
        };
        let key_av = crate::proxy_key::key_to_any(key);
        let argv = [target, key_av];
        let out = crate::method_call_closure_dispatch::__torajs_any_call_with_this(
            t,
            handler,
            argv.as_ptr(),
            2,
        );
        __torajs_anyv_rc_dec(t);
        __torajs_anyv_rc_dec(key_av);
        if __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(out);
            return VALUE_UNDEFINED;
        }
        // §10.5.5 step 7 — the trap answers an object or undefined;
        // anything else is a TypeError.
        if !crate::nanbox::is_undefined(out) && !crate::to_primitive::is_object_value(out) {
            __torajs_anyv_rc_dec(out);
            __torajs_throw_type_error(
                c"proxy 'getOwnPropertyDescriptor' trap must return an object or undefined"
                    .as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        // Steps 8-16 — the invariants, and the answer is the
        // COMPLETED record rebuilt as a fresh object (§6.2.6.4), not
        // the handler's own object: a partial `{ value: 1 }` reads
        // with all three attributes present, and the handler's
        // object is never written.
        let checked = crate::proxy_invariant::check_get_own(target, key, out);
        __torajs_anyv_rc_dec(out);
        match checked {
            Ok(Some(rd)) => rd.to_object(),
            Ok(None) | Err(()) => VALUE_UNDEFINED,
        }
    }
}

/// §7.3.24 EnumerableOwnProperties for a Proxy —
/// `Object.values(p)` (`want_entries == 0`) and
/// `Object.entries(p)` (non-zero). Both compose the same two traps:
/// ownKeys names the keys, [[GetOwnProperty]] decides which are
/// enumerable, and [[Get]] produces each value.
///
/// Answers an owned `Array<Any>` (values) or an `Array<Array<Any>>`
/// with its element-kind stamped (entries), the shapes the ordinary
/// reflection lane builds.
///
/// # Safety
/// `recv` is a live Proxy AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proxy_own_values(recv: AnyValue, want_entries: i64) -> *mut u8 {
    unsafe {
        let keys = crate::proxy_own_keys::__torajs_proxy_own_keys(recv, 0);
        let n = (keys.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read();
        let mut out = if want_entries == 0 {
            __torajs_arr_alloc_any(n)
        } else {
            __torajs_arr_alloc(n)
        };
        for i in 0..n {
            let data = (keys.cast::<u8>().add(ARR_DATA_PTR_OFF) as *const *const u64).read();
            let key = data.add(i as usize).read() as *const c_void;
            if key.is_null() {
                continue;
            }
            let v = crate::proxy::get(as_void_ptr(recv), key, recv);
            if __torajs_throw_check() != 0 {
                __torajs_anyv_rc_dec(v);
                break;
            }
            let (tag, payload) = (
                crate::__torajs_anyv_unbox_tag(v),
                crate::__torajs_anyv_unbox_value(v),
            );
            if want_entries == 0 {
                out = __torajs_arr_push_any(out as *mut c_void, tag as u64, payload as u64);
                continue;
            }
            torajs_rc::__torajs_rc_inc(key as *mut c_void);
            let inner = __torajs_arr_alloc_any(2);
            let inner = __torajs_arr_push_any(inner as *mut c_void, ANY_HEAP_TAG, key as u64);
            let inner = __torajs_arr_push_any(inner as *mut c_void, tag as u64, payload as u64);
            out = __torajs_arr_push(out, inner as i64);
        }
        // The key Arr is unmarked: its drop frees the block only, so
        // each key's stake is released here.
        let data = (keys.cast::<u8>().add(ARR_DATA_PTR_OFF) as *const *const u64).read();
        for i in 0..n as usize {
            __torajs_anyv_rc_dec(box_void_ptr(data.add(i).read() as *mut c_void));
        }
        __torajs_value_drop_heap(keys as *mut c_void);
        if want_entries != 0 {
            __torajs_arr_mark_kind(out as *mut c_void, KIND_CHAIN_HEAP);
        }
        out
    }
}
