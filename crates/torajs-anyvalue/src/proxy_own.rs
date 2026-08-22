//! §10.5.11 [[OwnPropertyKeys]] and §10.5.5 [[GetOwnProperty]] on a
//! Proxy (RFC 20260823-proxy-substrate 刀 4).
//!
//! These are the two internal methods the *reflection* surface reads
//! through — `Object.keys` / `getOwnPropertyNames` / `Reflect.ownKeys`
//! for the first, `getOwnPropertyDescriptor` and every enumerability
//! question for the second — so they are exported as C entries and
//! the reflection kernels in `torajs-meta` grow one arm each.
//!
//! `Object.keys` is not just the key list: §7.3.24
//! EnumerableOwnProperties calls [[GetOwnProperty]] on EVERY key the
//! ownKeys trap answered and keeps the enumerable ones. So the two
//! traps compose here, and a handler that answers keys but no
//! descriptors correctly yields nothing.
//!
//! §10.5.x invariant checks (the non-extensible / non-configurable
//! consistency rules) are still deferred — they need the target's own
//! descriptor for comparison, which this file now has, so they are
//! the next knife rather than a missing foundation.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use crate::proxy::{live_slots, trap};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    /// torajs-meta — the ordinary reflection surface, used for the
    /// trap-less forward.
    fn __torajs_anyv_own_keys(v: u64, include_nonenum: i64) -> *mut c_void;
    fn __torajs_anyv_get_property_descriptor(obj_any: u64, key: *const c_void) -> u64;
    /// torajs-str — release a key cell the enumerable filter dropped.
    fn __torajs_str_drop(s: *mut c_void);
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

/// §10.5.11. `include_nonenum == 0` is `Object.keys`' surface, which
/// §7.3.24 filters through [[GetOwnProperty]]; anything else keeps
/// every key the trap answered.
///
/// Answers an owned `Array<Any>` cell of Str keys.
///
/// # Safety
/// `recv` is a live Proxy AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proxy_own_keys(recv: AnyValue, include_nonenum: i64) -> *mut u8 {
    unsafe {
        let Ok(__s) = live_slots(as_void_ptr(recv)) else {
            return __torajs_arr_alloc(0);
        };
        let (target, handler) = (__s.target, __s.handler);
        let t = match trap(handler, b"ownKeys") {
            Err(()) => return __torajs_arr_alloc(0),
            Ok(None) => return __torajs_anyv_own_keys(target, include_nonenum) as *mut u8,
            Ok(Some(t)) => t,
        };
        let argv = [target];
        let listish = crate::method_call_closure_dispatch::__torajs_any_call_with_this(
            t,
            handler,
            argv.as_ptr(),
            1,
        );
        __torajs_anyv_rc_dec(t);
        if __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(listish);
            return __torajs_arr_alloc(0);
        }
        // §7.3.18 CreateListFromArrayLike — the trap MUST answer an
        // object; a primitive is the step-6 TypeError.
        if !is_cell(listish) {
            __torajs_anyv_rc_dec(listish);
            __torajs_throw_type_error(
                c"proxy 'ownKeys' trap must return an array-like object".as_ptr(),
            );
            return __torajs_arr_alloc(0);
        }
        let out = collect_key_list(listish, recv, include_nonenum);
        __torajs_anyv_rc_dec(listish);
        out
    }
}

/// Walk the trap's array-like, keeping the entries that are
/// property keys and — for the `Object.keys` surface — enumerable
/// ones only.
///
/// The result is an `Array<Str>` of CELL pointers, not of boxed
/// values: that is the repr the reflection lane's callers read
/// (`__torajs_anyv_own_keys`' other arms build the same thing). A
/// trap element that is a ShortStr immediate has no cell of its own,
/// so every key is materialized through ToString — which is also
/// what turns a numeric element into the `"0"` a property key is.
///
/// # Safety
/// `listish` is a live cell AnyValue; `recv` a live Proxy AnyValue.
unsafe fn collect_key_list(listish: AnyValue, recv: AnyValue, include_nonenum: i64) -> *mut u8 {
    unsafe {
        let len_av = crate::len_get::__torajs_any_length_get(listish);
        let len = crate::nanbox_ffi::__torajs_anyv_to_number(len_av);
        __torajs_anyv_rc_dec(len_av);
        let n = if len.is_finite() && len > 0.0 {
            len as i64
        } else {
            0
        };
        let mut out = __torajs_arr_alloc(0);
        for i in 0..n {
            let k = crate::index_any::__torajs_any_index_get(listish, i);
            if __torajs_throw_check() != 0 {
                __torajs_anyv_rc_dec(k);
                return out;
            }
            // A Symbol key belongs to the symbol lane, which this
            // string-keyed surface does not enumerate.
            if is_symbol_value(k) {
                __torajs_anyv_rc_dec(k);
                continue;
            }
            let key = crate::nanbox_ffi::__torajs_anyv_to_str(k);
            __torajs_anyv_rc_dec(k);
            if key.is_null() {
                return out;
            }
            if include_nonenum == 0 && !enumerable_on(recv, key as *const c_void) {
                __torajs_str_drop(key);
                if __torajs_throw_check() != 0 {
                    return out;
                }
                continue;
            }
            // The Arr takes the stake the ToString mint carries.
            out = __torajs_arr_push(out, key as i64);
        }
        out
    }
}

/// Is this value a Symbol cell?
fn is_symbol_value(k: AnyValue) -> bool {
    if !is_cell(k) {
        return false;
    }
    unsafe {
        as_void_ptr(k).cast::<u8>().add(4).cast::<u16>().read() == torajs_rc::Tag::Symbol as u16
    }
}

/// §7.3.24 step 2.a.i — is this key's own descriptor enumerable, as
/// the PROXY reports it (its gOPD trap, not the target's)?
unsafe fn enumerable_on(recv: AnyValue, key: *const c_void) -> bool {
    unsafe {
        let d = __torajs_proxy_get_own_descriptor(recv, key);
        if crate::nanbox::is_undefined(d) {
            return false;
        }
        let Some(e) = crate::proxy_get_prop::get_by_name(d, b"enumerable") else {
            __torajs_anyv_rc_dec(d);
            return false;
        };
        let b = crate::nanbox_ffi::__torajs_anyv_to_bool(e);
        __torajs_anyv_rc_dec(e);
        __torajs_anyv_rc_dec(d);
        b
    }
}

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
        if crate::nanbox::is_undefined(out) {
            return VALUE_UNDEFINED;
        }
        if !crate::to_primitive::is_object_value(out) {
            __torajs_anyv_rc_dec(out);
            __torajs_throw_type_error(
                c"proxy 'getOwnPropertyDescriptor' trap must return an object or undefined"
                    .as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        // §6.2.6.4 FromPropertyDescriptor — the spec COMPLETES the
        // trap's descriptor before handing it back, so a partial one
        // ({value: 1}) reads with the three attributes present and
        // false. The completion is the reflection surface's own
        // (`__torajs_anyv_complete_descriptor`), shared with the
        // ordinary lane.
        crate::proxy_desc::complete_descriptor(out)
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
        let keys = __torajs_proxy_own_keys(recv, 0);
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
        __torajs_value_drop_heap(keys as *mut c_void);
        if want_entries != 0 {
            __torajs_arr_mark_kind(out as *mut c_void, KIND_CHAIN_HEAP);
        }
        out
    }
}
