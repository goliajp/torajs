//! DynObj key lookups — `get_tag` / `get_value` / `get_flags`.
//!
//! Port of `runtime_str.c::__torajs_dynobj_get_{tag,value,flags}`
//! (P4.2-b, 2026-05-23). All three share [`crate::probe::probe`] +
//! the `Bucket` field layout; the only per-fn variation is which field
//! returns and the default for an absent / non-dynobj input.
//!
//! Defensive type-tag check: callers occasionally pass an Any-box that
//! does not wrap a DynObj (e.g. typed Struct via `obj?.x.y` chained
//! optional access). Without the `type_tag == DYNOBJ` guard, the probe
//! would index into a wrong layout and return garbage tag values.

use core::ffi::c_void;

use crate::layout::{
    ANY_UNDEF, BUCKET_FLAG_CONFIGURABLE, BUCKET_FLAG_ENUMERABLE, BUCKET_FLAG_WRITABLE,
    BUCKET_TAG_MASK, TAG_DYNOBJ,
};
use crate::probe::{buckets, probe};

/// Read the `type_tag: u16` at offset 4 of the heap header.
///
/// # Safety
/// `obj` must point at a live heap block with the universal header.
#[inline]
unsafe fn type_tag(obj: *const c_void) -> u16 {
    unsafe { *((obj as *const u8).add(4) as *const u16) }
}

/// `__torajs_dynobj_get_tag(obj, key)` — return the slot's ANY_TAG
/// (low 8 bits of `Bucket::tag`). Returns `ANY_UNDEF` (5) when `obj`
/// is NULL, not a DynObj, or the key is absent.
///
/// # Safety
/// `obj` is null or a live heap pointer with a universal header.
/// `key` (if reached) is a live Str heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64 {
    if obj.is_null() {
        return ANY_UNDEF;
    }
    if unsafe { type_tag(obj) } != TAG_DYNOBJ {
        return ANY_UNDEF;
    }
    let p = unsafe { probe(obj, key) };
    if !p.found {
        return ANY_UNDEF;
    }
    let bk = unsafe { buckets(obj) };
    unsafe { (*bk.add(p.idx as usize)).tag & BUCKET_TAG_MASK }
}

/// `__torajs_dynobj_get_value(obj, key)` — return the slot's
/// per-tag payload (`Bucket::value`). Returns 0 when `obj` is NULL,
/// not a DynObj, or the key is absent.
///
/// # Safety
/// Same contract as [`__torajs_dynobj_get_tag`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64 {
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
    let bk = unsafe { buckets(obj) };
    unsafe { (*bk.add(p.idx as usize)).value }
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
    let bk = unsafe { buckets(obj) };
    let t = unsafe { (*bk.add(p.idx as usize)).tag };
    let mut flags: u64 = 0;
    if t & BUCKET_FLAG_WRITABLE != 0 {
        flags |= 1 << 0;
    }
    if t & BUCKET_FLAG_ENUMERABLE != 0 {
        flags |= 1 << 1;
    }
    if t & BUCKET_FLAG_CONFIGURABLE != 0 {
        flags |= 1 << 2;
    }
    flags
}
