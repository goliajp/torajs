//! DynObj key lookups — `get_own` / `get_tag` / `get_value` /
//! `get_flags`.
//!
//! `get_tag` and `get_value` are the two halves of `get_own`'s
//! one-probe `(tag, value)` read (one body, kept out of line so the
//! three entries share it); `get_flags` reads the entry's attribute
//! bits off the same [`crate::probe::probe`].
//!
//! Defensive type-tag check: callers occasionally pass an Any-box that
//! does not wrap a DynObj (e.g. typed Struct via `obj?.x.y` chained
//! optional access). Without the `type_tag == DYNOBJ` guard, the probe
//! would index into a wrong layout and return garbage tag values.

use core::ffi::c_void;

use crate::nanbox;

use crate::accessor::{TAG_ACCESSOR_PAIR, VALUE_UNDEFINED};
use crate::define::ANY_I64;
use crate::global_cell::deref_global_cell;
use crate::layout::{
    ANY_ACCESSOR, ANY_HEAP, ANY_UNDEF, BUCKET_FLAG_CONFIGURABLE, BUCKET_FLAG_ENUMERABLE,
    BUCKET_FLAG_WRITABLE, DYNOBJ_HOLE_SENTINEL, TAG_DYNOBJ,
};
use crate::probe::{bucket_flags, entries, probe};

/// The `(tag, value)` pair tag of a double.
const ANY_F64: u64 = 3;

/// Read the `type_tag: u16` at offset 4 of the heap header.
///
/// # Safety
/// `obj` must point at a live heap block with the universal header.
#[inline]
pub(crate) unsafe fn type_tag(obj: *const c_void) -> u16 {
    unsafe { *((obj as *const u8).add(4) as *const u16) }
}

/// `__torajs_dynobj_get_tag(obj, key)` — return the slot's ANY_TAG
/// (decoded from the NaN-box `value_anyv`). Returns `ANY_UNDEF` (5)
/// when `obj` is NULL, not a DynObj, or the key is absent.
///
/// # Safety
/// `obj` is null or a live heap pointer with a universal header.
/// `key` (if reached) is a live Str heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64 {
    match unsafe { __torajs_dynobj_get_own(obj, key, core::ptr::null_mut()) } {
        DYNOBJ_GET_OWN_MISS => ANY_UNDEF,
        tag => tag,
    }
}

/// [`__torajs_dynobj_get_own`]'s answer when `obj` has no entry for
/// `key` (no `(tag, value)` pair uses it: tags stop at
/// [`ANY_ACCESSOR`]).
pub const DYNOBJ_GET_OWN_MISS: u64 = u64::MAX;

/// `__torajs_dynobj_get_own(obj, key, value_out)` — the own entry's
/// whole `(tag, value)` pair from one probe: the tag is returned (the
/// same answer [`__torajs_dynobj_get_tag`] gives, accessor sentinel
/// included) and the value is written to `value_out` (the same answer
/// [`__torajs_dynobj_get_value`] gives). An entry storing `undefined`
/// is a hit `(ANY_UNDEF, 0)`, and so is a hole tombstone (the get
/// pair reads it as undefined and `has` as present); only a key with
/// no entry answers [`DYNOBJ_GET_OWN_MISS`], leaving `value_out`
/// untouched; a null `obj` or a cell that is not a DynObj owns no
/// entry. A null `value_out` asks for the tag alone: a ShortStr
/// entry then materializes no `Str` (nobody would release it).
///
/// # Safety
/// `obj` is null or a live heap pointer with a universal header;
/// `key` is a live Str or Symbol cell; `value_out` is null or
/// writable.
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe extern "C" fn __torajs_dynobj_get_own(
    obj: *const c_void,
    key: *const c_void,
    value_out: *mut u64,
) -> u64 {
    if obj.is_null() || unsafe { type_tag(obj) } != TAG_DYNOBJ {
        return DYNOBJ_GET_OWN_MISS;
    }
    let p = unsafe { probe(obj, key) };
    if !p.found {
        return DYNOBJ_GET_OWN_MISS;
    }
    let v = unsafe { (*entries(obj).add(p.entry as usize)).value_anyv };
    // A hole tombstone reads as an entry storing undefined.
    let v = if v == DYNOBJ_HOLE_SENTINEL {
        VALUE_UNDEFINED
    } else {
        unsafe { deref_global_cell(v, key as *mut c_void) }
    };
    let (tag, value) = if nanbox::is_int32(v) {
        (ANY_I64, v as i32 as i64 as u64)
    } else if nanbox::is_cell(v) {
        // only a heap cell can be an accessor pair
        let pair = unsafe { type_tag(v as *const c_void) } == TAG_ACCESSOR_PAIR;
        (if pair { ANY_ACCESSOR } else { ANY_HEAP }, v)
    } else if nanbox::is_number(v) {
        (ANY_F64, nanbox::double_bits(v))
    } else {
        // A ShortStr's value is a fresh `Str`: it is materialized only
        // for a caller that takes the value.
        let value = if value_out.is_null() {
            0
        } else {
            unsafe { nanbox::unbox_value(v) as u64 }
        };
        (nanbox::unbox_tag(v) as u64, value)
    };
    if !value_out.is_null() {
        unsafe { value_out.write(value) };
    }
    tag
}

/// `__torajs_dynobj_get_value(obj, key)` — return the slot's
/// per-tag payload (decoded from the NaN-box `value_anyv`). Returns
/// 0 when `obj` is NULL, not a DynObj, or the key is absent.
///
/// # Safety
/// Same contract as [`__torajs_dynobj_get_tag`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64 {
    // a miss leaves the slot at 0
    let mut value = 0;
    unsafe { __torajs_dynobj_get_own(obj, key, &mut value) };
    value
}

/// `__torajs_dynobj_get_flags(obj, key)` — return the slot's
/// PropertyDescriptor data-attribute flags packed as
/// `bit 0 = writable, bit 1 = enumerable, bit 2 = configurable`.
/// Returns 0 when `obj` is NULL, not a DynObj, or the key is absent.
///
/// Used by `getOwnPropertyDescriptor` to populate the descriptor
/// object's boolean fields.
///
/// # Safety
/// Same contract as [`__torajs_dynobj_get_tag`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_get_flags(obj: *const c_void, key: *const c_void) -> u64 {
    if obj.is_null() {
        return 0;
    }
    if unsafe { type_tag(obj) } != TAG_DYNOBJ {
        return 0;
    }
    let p = unsafe { probe(obj, key) };
    if !p.found {
        return 0;
    }
    let ent = unsafe { entries(obj) };
    let kp_tagged = unsafe { (*ent.add(p.entry as usize)).key_ptr_tagged };
    let f = bucket_flags(kp_tagged);
    // The output ABI for get_flags is bit 0/1/2 = W/E/C, matching the
    // internal entry flag-bit layout exactly — return verbatim.
    let mut flags: u64 = 0;
    if f & BUCKET_FLAG_WRITABLE != 0 {
        flags |= 1 << 0;
    }
    if f & BUCKET_FLAG_ENUMERABLE != 0 {
        flags |= 1 << 1;
    }
    if f & BUCKET_FLAG_CONFIGURABLE != 0 {
        flags |= 1 << 2;
    }
    flags
}
