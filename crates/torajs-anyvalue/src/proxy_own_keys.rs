//! §10.5.11 [[OwnPropertyKeys]] on a Proxy.
//!
//! One kernel produces the whole key list — strings AND symbols, in
//! the order the trap answered — and runs the invariant checks over
//! it; the three reflection faces (`getOwnPropertyNames` /
//! `Object.keys`, `getOwnPropertySymbols`, `Reflect.ownKeys`) filter
//! that one list. The checks have to see the full list: a handler
//! that drops a non-configurable SYMBOL key is caught by
//! `Object.keys` too, which only ever reports strings.
//!
//! Keys are carried as owned cell pointers (Str or Symbol), the repr
//! the ordinary reflection kernels answer in.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, as_void_ptr, box_void_ptr, is_cell, is_short_str};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use crate::proxy::{live_slots, trap};
use crate::proxy_invariant::{target_desc, target_extensible, violated};

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    fn __torajs_str_eq(a: *const u8, b: *const u8) -> i64;
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-meta — OrdinaryOwnPropertyKeys (strings, then symbols),
    /// and the Proxy arm that routes back here.
    fn __torajs_anyv_own_keys_all(obj_any: u64) -> *mut c_void;
}

/// `Arr` header mirrors (torajs-arr layout): `len` u64 at +8,
/// element storage pointer at +32.
const ARR_LEN_OFF: usize = 8;
const ARR_DATA_PTR_OFF: usize = 32;

/// A list of owned key cells.
#[derive(Default)]
pub(crate) struct KeyList(Vec<*mut c_void>);

impl Drop for KeyList {
    fn drop(&mut self) {
        for &k in &self.0 {
            // SAFETY: every entry is an owned Str or Symbol cell.
            unsafe { __torajs_anyv_rc_dec(box_void_ptr(k)) };
        }
    }
}

impl KeyList {
    fn contains(&self, key: *const c_void) -> bool {
        self.0.iter().any(|&k| unsafe { key_eq(k, key) })
    }

    /// Take the list's entries out as an owned `Arr` of cell
    /// pointers, keeping the ones `keep` accepts; the rest are
    /// released.
    unsafe fn into_arr(mut self, mut keep: impl FnMut(*mut c_void) -> bool) -> *mut u8 {
        unsafe {
            let mut out = __torajs_arr_alloc(0);
            for k in core::mem::take(&mut self.0) {
                if keep(k) {
                    out = __torajs_arr_push(out, k as i64);
                } else {
                    __torajs_anyv_rc_dec(box_void_ptr(k));
                }
            }
            out
        }
    }
}

fn is_symbol_cell(k: *const c_void) -> bool {
    unsafe { k.cast::<u8>().add(4).cast::<u16>().read() == torajs_rc::Tag::Symbol as u16 }
}

/// Property-key identity: symbols by cell, strings by content.
unsafe fn key_eq(a: *const c_void, b: *const c_void) -> bool {
    match (is_symbol_cell(a), is_symbol_cell(b)) {
        (true, true) => a == b,
        (false, false) => unsafe { __torajs_str_eq(a.cast(), b.cast()) != 0 },
        _ => false,
    }
}

/// target.[[OwnPropertyKeys]]() — the ordinary kernel, which answers
/// a Proxy target through this file again.
unsafe fn target_keys(target: AnyValue) -> Result<KeyList, ()> {
    unsafe {
        let arr = __torajs_anyv_own_keys_all(target);
        let n = arr.cast::<u8>().add(ARR_LEN_OFF).cast::<u64>().read() as usize;
        let data = arr
            .cast::<u8>()
            .add(ARR_DATA_PTR_OFF)
            .cast::<*const u64>()
            .read();
        // The Arr is unmarked, so dropping it releases only the block;
        // the element stakes move into the list.
        let keys = KeyList((0..n).map(|i| data.add(i).read() as *mut c_void).collect());
        __torajs_value_drop_heap(arr);
        if __torajs_throw_check() != 0 {
            return Err(());
        }
        Ok(keys)
    }
}

/// §7.3.18 CreateListFromArrayLike(obj, « String, Symbol »), plus the
/// §10.5.11 step 9 duplicate check.
unsafe fn trap_key_list(listish: AnyValue) -> Result<KeyList, ()> {
    unsafe {
        if !crate::to_primitive::is_object_value(listish) {
            return violated(c"proxy ownKeys trap must return an array-like object");
        }
        let len_av = crate::len_get::__torajs_any_length_get(listish);
        let len = crate::nanbox_ffi::__torajs_anyv_to_number(len_av);
        __torajs_anyv_rc_dec(len_av);
        if __torajs_throw_check() != 0 {
            return Err(());
        }
        let n = if len.is_finite() && len > 0.0 {
            len as i64
        } else {
            0
        };
        let mut keys = KeyList::default();
        for i in 0..n {
            let k = crate::index_any::__torajs_any_index_get(listish, i);
            if __torajs_throw_check() != 0 {
                __torajs_anyv_rc_dec(k);
                return Err(());
            }
            let cell = if is_short_str(k) || (is_cell(k) && is_str_cell(k)) {
                let s = crate::nanbox_ffi::__torajs_anyv_to_str(k);
                __torajs_anyv_rc_dec(k);
                s
            } else if is_cell(k) && is_symbol_cell(as_void_ptr(k)) {
                as_void_ptr(k)
            } else {
                __torajs_anyv_rc_dec(k);
                return violated(c"proxy ownKeys trap result may only contain strings and symbols");
            };
            if keys.contains(cell) {
                __torajs_anyv_rc_dec(box_void_ptr(cell));
                return violated(c"proxy ownKeys trap result cannot contain duplicate keys");
            }
            keys.0.push(cell);
        }
        Ok(keys)
    }
}

fn is_str_cell(k: AnyValue) -> bool {
    unsafe { as_void_ptr(k).cast::<u8>().add(4).cast::<u16>().read() == torajs_rc::Tag::Str as u16 }
}

/// §10.5.11 — the validated key list. `Err` = a pending throw.
///
/// # Safety
/// `recv` is a live Proxy AnyValue.
pub(crate) unsafe fn own_property_keys(recv: AnyValue) -> Result<KeyList, ()> {
    unsafe {
        let s = live_slots(as_void_ptr(recv))?;
        let t = match trap(s.handler, b"ownKeys")? {
            None => return target_keys(s.target),
            Some(t) => t,
        };
        let argv = [s.target];
        let listish = crate::method_call_closure_dispatch::__torajs_any_call_with_this(
            t,
            s.handler,
            argv.as_ptr(),
            1,
        );
        __torajs_anyv_rc_dec(t);
        if __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(listish);
            return Err(());
        }
        let keys = trap_key_list(listish);
        __torajs_anyv_rc_dec(listish);
        let keys = keys?;
        check_against_target(s.target, keys)
    }
}

/// §10.5.11 steps 10-23.
unsafe fn check_against_target(target: AnyValue, keys: KeyList) -> Result<KeyList, ()> {
    unsafe {
        let extensible = target_extensible(target)?;
        let target_keys = target_keys(target)?;
        let mut nonconfigurable = Vec::new();
        let mut configurable = Vec::new();
        for &k in &target_keys.0 {
            match target_desc(target, k)? {
                Some(d) if d.configurable == Some(false) => nonconfigurable.push(k),
                _ => configurable.push(k),
            }
        }
        if extensible && nonconfigurable.is_empty() {
            return Ok(keys);
        }
        let mut unchecked: Vec<bool> = vec![true; keys.0.len()];
        for k in nonconfigurable {
            if !take(&keys, &mut unchecked, k) {
                return violated(
                    c"proxy ownKeys trap result must include every non-configurable key of the target",
                );
            }
        }
        if extensible {
            return Ok(keys);
        }
        for k in configurable {
            if !take(&keys, &mut unchecked, k) {
                return violated(
                    c"proxy ownKeys trap result must include every key of a non-extensible target",
                );
            }
        }
        if unchecked.contains(&true) {
            return violated(
                c"proxy ownKeys trap cannot report new keys for a non-extensible target",
            );
        }
        Ok(keys)
    }
}

/// Mark the first unchecked occurrence of `k`; false when there is none.
unsafe fn take(keys: &KeyList, unchecked: &mut [bool], k: *const c_void) -> bool {
    let hit = (0..keys.0.len()).find(|&i| unchecked[i] && unsafe { key_eq(keys.0[i], k) });
    hit.map(|i| unchecked[i] = false).is_some()
}

/// `getOwnPropertyNames` (`include_nonenum != 0`) and `Object.keys`
/// (`0`, §7.3.24: each string key filtered through the proxy's own
/// [[GetOwnProperty]]). Answers an owned `Arr` of Str cells.
///
/// # Safety
/// `recv` is a live Proxy AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proxy_own_keys(recv: AnyValue, include_nonenum: i64) -> *mut u8 {
    unsafe {
        let Ok(keys) = own_property_keys(recv) else {
            return __torajs_arr_alloc(0);
        };
        keys.into_arr(|k| {
            !is_symbol_cell(k)
                && (include_nonenum != 0 || (__torajs_throw_check() == 0 && enumerable_on(recv, k)))
        })
    }
}

/// `getOwnPropertySymbols` (`include_nonenum != 0`) and the
/// enumerable symbol keys `Object.assign` copies (`0`).
///
/// # Safety
/// `recv` is a live Proxy AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proxy_own_symbols(
    recv: AnyValue,
    include_nonenum: i64,
) -> *mut u8 {
    unsafe {
        let Ok(keys) = own_property_keys(recv) else {
            return __torajs_arr_alloc(0);
        };
        keys.into_arr(|k| {
            is_symbol_cell(k)
                && (include_nonenum != 0 || (__torajs_throw_check() == 0 && enumerable_on(recv, k)))
        })
    }
}

/// `Reflect.ownKeys` (`include_nonenum != 0`) and the enumerable
/// keys of both kinds `Object.assign` copies (`0`) — one trap call
/// for the whole list.
///
/// # Safety
/// `recv` is a live Proxy AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proxy_own_keys_all(
    recv: AnyValue,
    include_nonenum: i64,
) -> *mut u8 {
    unsafe {
        let Ok(keys) = own_property_keys(recv) else {
            return __torajs_arr_alloc(0);
        };
        keys.into_arr(|k| {
            include_nonenum != 0 || (__torajs_throw_check() == 0 && enumerable_on(recv, k))
        })
    }
}

/// §7.3.24 step 2.a.i — is this key's own descriptor enumerable, as
/// the PROXY reports it (its gOPD trap, not the target's)?
unsafe fn enumerable_on(recv: AnyValue, key: *const c_void) -> bool {
    unsafe {
        let d = crate::proxy_own::__torajs_proxy_get_own_descriptor(recv, key);
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
