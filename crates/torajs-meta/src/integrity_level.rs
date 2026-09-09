//! §7.3.16 TestIntegrityLevel — `Object.isSealed` / `Object.isFrozen`.
//!
//! The spec asks three things of a receiver: is it non-extensible, is
//! every own property non-configurable, and (frozen) is every own
//! data property non-writable. The first is the header bit; the other
//! two are read off the same own-keys walk and descriptor probe every
//! reflection surface uses, so a receiver's answer is whatever
//! `Reflect.ownKeys` + `getOwnPropertyDescriptor` would say about it —
//! an array's elements and `length`, a function's virtual pair and
//! `prototype`, a struct's declared fields, a wrapper's index domain,
//! and every cell's expando bag, each through its own arm and none of
//! it spelled a second time here.
//!
//! Before this `isFrozen` read only the header bit and `isSealed`
//! answered false for every non-dynobj cell that was merely made
//! non-extensible: `Object.preventExtensions({})` read as not frozen,
//! a non-extensible Map (no own properties at all) as neither sealed
//! nor frozen, a non-extensible object whose every property had been
//! defined `{configurable: false, writable: false}` as not frozen.
//!
//! The header markers stay as the fast path: `Object.seal` /
//! `Object.freeze` walked every own property when they ran, so a
//! marked cell needs no second walk. A Proxy answers through its
//! traps and skips the markers.

use core::ffi::c_void;

use crate::obj_own_keys::{ARR_DATA_PTR_OFF, ARR_LEN_OFF};
use crate::reflect::{ANY_BOOL, TAG_PROXY, VALUE_UNDEFINED_IMM, heap_type_tag, is_cell_imm};

unsafe extern "C" {
    /// torajs-rc — the header markers `Object.seal` / `Object.freeze`
    /// set, and the [[Extensible]] bit.
    fn __torajs_obj_is_sealed_marked(p: *const c_void) -> bool;
    fn __torajs_obj_is_frozen(p: *const c_void) -> bool;
    /// torajs-dynobj — one descriptor field.
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const u8) -> u64;
    /// torajs-value-drop — release the key array / a descriptor.
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-str — release a minted key.
    fn __torajs_str_drop(s: *mut u8);
}

/// `Object.isSealed(O)` — §20.1.2.20: `TestIntegrityLevel(O, sealed)`;
/// a non-Object answers `true`.
///
/// # Safety
/// `obj_any` carries a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_is_sealed(obj_any: u64) -> bool {
    unsafe { test_integrity_level(obj_any, Level::Sealed) }
}

/// `Object.isFrozen(O)` — §20.1.2.14: `TestIntegrityLevel(O, frozen)`;
/// a non-Object answers `true`.
///
/// # Safety
/// `obj_any` carries a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_is_frozen(obj_any: u64) -> bool {
    unsafe { test_integrity_level(obj_any, Level::Frozen) }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Sealed,
    Frozen,
}

/// See module doc.
///
/// # Safety
/// `obj_any` carries a valid AnyValue bit pattern.
unsafe fn test_integrity_level(obj_any: u64, level: Level) -> bool {
    if !is_cell_imm(obj_any) {
        return true;
    }
    let p = obj_any as *const c_void;
    // SAFETY: is_cell_imm guarantees a live heap pointer.
    if unsafe { crate::extensible_reflect::is_primitive_heap_tag(p) } {
        return true;
    }
    // Step 1 — `[[IsExtensible]]`; the anyv face also answers a Proxy.
    if unsafe { crate::extensible_reflect::__torajs_anyv_is_extensible(obj_any) } {
        return false;
    }
    // SAFETY: cell pointer.
    let is_proxy = unsafe { heap_type_tag(p) } == TAG_PROXY;
    if !is_proxy {
        let marked = match level {
            // SAFETY: cell pointer with the universal header.
            Level::Sealed => unsafe { __torajs_obj_is_sealed_marked(p) },
            Level::Frozen => unsafe { __torajs_obj_is_frozen(p) },
        };
        if marked {
            return true;
        }
    }
    // Steps 3-4 — every own key's descriptor.
    let keys = unsafe { crate::own_names::__torajs_anyv_own_keys_all(obj_any) };
    let verdict = unsafe { every_own_property_satisfies(obj_any, keys, level) };
    unsafe { __torajs_value_drop_heap(keys) };
    verdict
}

/// Walk `keys` (an `Arr` of live key cells, borrowed — the caller
/// releases it) and ask each descriptor: configurable is a `false` for
/// either level; a writable data property is a `false` for frozen.
///
/// # Safety
/// `keys` is NULL or a live `Arr` of key cells; `obj_any` is the
/// receiver they were listed on.
unsafe fn every_own_property_satisfies(obj_any: u64, keys: *mut c_void, level: Level) -> bool {
    if keys.is_null() {
        return true;
    }
    let len = unsafe { keys.cast::<u8>().add(ARR_LEN_OFF).cast::<u64>().read() };
    let data = unsafe {
        keys.cast::<u8>()
            .add(ARR_DATA_PTR_OFF)
            .cast::<*mut u64>()
            .read()
    };
    if data.is_null() {
        return true;
    }
    let k_conf = unsafe { crate::reflect::alloc_str_key(b"configurable") };
    let k_writ = unsafe { crate::reflect::alloc_str_key(b"writable") };
    let mut ok = true;
    for i in 0..len {
        let k = unsafe { data.add(i as usize).read() } as *const c_void;
        if k.is_null() {
            continue;
        }
        let d = unsafe {
            crate::reflect_get_property_descriptor::__torajs_anyv_get_property_descriptor(
                obj_any, k,
            )
        };
        if d == VALUE_UNDEFINED_IMM {
            continue;
        }
        let desc = d as *const c_void;
        // SAFETY: a descriptor is a fresh dynobj this walk owns.
        let configurable = unsafe { desc_bool(desc, k_conf) };
        let writable = unsafe { desc_bool(desc, k_writ) };
        unsafe { __torajs_value_drop_heap(desc as *mut c_void) };
        if configurable || (level == Level::Frozen && writable) {
            ok = false;
            break;
        }
    }
    unsafe { __torajs_str_drop(k_conf) };
    unsafe { __torajs_str_drop(k_writ) };
    ok
}

/// A descriptor's boolean field; an absent field (an accessor has no
/// `writable`) reads `false`.
///
/// # Safety
/// `desc` is a live dynobj; `key` a live Str.
unsafe fn desc_bool(desc: *const c_void, key: *const u8) -> bool {
    let tag = unsafe { __torajs_dynobj_get_tag(desc, key) } as i64;
    tag == ANY_BOOL && unsafe { __torajs_dynobj_get_value(desc, key) } != 0
}
