//! `Object.getOwnPropertyDescriptor` arm for a `Tag::Closure` cell
//! (RFC 20260711-closure-reflection chunk B).
//!
//! ES §20.2.4: every function object carries own `name` and `length`
//! data properties `{ writable: false, enumerable: false,
//! configurable: true }`. tr carries the pair virtually — the value
//! sides live in torajs-anyvalue's metadata chain
//! (`__torajs_closure_name_str`: method-cell interned name / bound
//! `"bound "` prefix / fn-addr registry; `__torajs_closure_length`:
//! method-cell arity / bound subtract / registry arity, `-1` = miss).
//!
//! Own-property order: a live expando entry (monkey-patch, or a
//! post-delete recreate once chunk C lands the tombstones) wins over
//! the virtual pair — the probe delegates to the DynObj descriptor
//! path so accessor entries and attribute flags come out right.
//! `name` always answers (registry misses answer the ES
//! anonymous-function `""`); `length` answers only when the metadata
//! chain has an arity, mirroring the `.length` member read.

use core::ffi::c_void;

use crate::reflect::{VALUE_UNDEFINED_IMM, build_data_descriptor};

unsafe extern "C" {
    /// torajs-anyvalue — owned name Str (interned immortal for
    /// method cells, fresh rc=1 otherwise; drop no-ops on statics).
    fn __torajs_closure_name_str(ptr: *mut c_void) -> *mut u8;
    /// torajs-anyvalue — metadata-chain arity, `-1` = miss.
    fn __torajs_closure_length(ptr: *mut c_void) -> i64;
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const u8) -> bool;
}

const ANY_I64: u64 = 2;
const ANY_HEAP: u64 = 4;

/// `torajs_rc::FLAG_FN_NAME_DELETED` / `FLAG_FN_LENGTH_DELETED`
/// mirrors (chunk C tombstones; this crate keeps its dep tree
/// narrow — the u16 bit positions are part of the header ABI).
const FLAG_FN_NAME_DELETED: u16 = 1 << 13;
const FLAG_FN_LENGTH_DELETED: u16 = 1 << 14;

/// Closure-env layout mirror (torajs-core `ssa_lower.rs` constants):
/// expando props-dynobj slot at +24.
const CLOSURE_PROPS_OFF: usize = 24;

/// Key Str layout mirror — len u32 at +8, payload at +16.
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;

/// `true` iff the key Str spells exactly `name`.
unsafe fn key_is(key: *const c_void, name: &[u8]) -> bool {
    let len = unsafe { key.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() };
    len as usize == name.len()
        && unsafe { core::slice::from_raw_parts(key.cast::<u8>().add(STR_DATA_OFF), len as usize) }
            == name
}

/// See module doc.
///
/// # Safety
/// `cell` is a live `Tag::Closure` heap pointer (caller checked the
/// header tag); `key` is a live `Str` pointer (caller checked
/// non-NULL).
pub(crate) unsafe fn closure_cell_descriptor(cell: *const c_void, key: *const c_void) -> u64 {
    // 1. expando entry wins — delegate to the DynObj descriptor path
    //    (accessor entries / attribute flags handled there).
    let props = unsafe {
        cell.cast::<u8>()
            .add(CLOSURE_PROPS_OFF)
            .cast::<*const c_void>()
            .read()
    };
    if !props.is_null() && unsafe { __torajs_dynobj_has(props, key as *const u8) } {
        return unsafe { crate::reflect::__torajs_anyv_get_property_descriptor(props as u64, key) };
    }
    // 2. the virtual §20.2.4 pair — `{ writable: false, enumerable:
    //    false, configurable: true }`. The owned name Str transfers
    //    into the descriptor (no extra inc). A chunk-C tombstone
    //    (delete fn.name / fn.length) skips the virtual answer.
    let flags = unsafe { (cell.cast::<u8>().add(6) as *const u16).read() };
    if unsafe { key_is(key, b"name") } && flags & FLAG_FN_NAME_DELETED == 0 {
        let s = unsafe { __torajs_closure_name_str(cell as *mut c_void) };
        return unsafe { build_data_descriptor(ANY_HEAP, s as u64, 0, 0, 1) };
    }
    if unsafe { key_is(key, b"length") } && flags & FLAG_FN_LENGTH_DELETED == 0 {
        let l = unsafe { __torajs_closure_length(cell as *mut c_void) };
        if l >= 0 {
            return unsafe { build_data_descriptor(ANY_I64, l as u64, 0, 0, 1) };
        }
    }
    VALUE_UNDEFINED_IMM
}
