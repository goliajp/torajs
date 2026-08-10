//! DynObj key lookups — `get_tag` / `get_value` / `get_flags`.
//!
//! All three share [`crate::probe::probe`] + the dense `Entry` layout;
//! the only per-fn variation is which field returns and the default
//! for an absent / non-dynobj input.
//!
//! Defensive type-tag check: callers occasionally pass an Any-box that
//! does not wrap a DynObj (e.g. typed Struct via `obj?.x.y` chained
//! optional access). Without the `type_tag == DYNOBJ` guard, the probe
//! would index into a wrong layout and return garbage tag values.

use core::ffi::c_void;

use crate::accessor::TAG_ACCESSOR_PAIR;
use crate::layout::{
    ANY_ACCESSOR, ANY_HEAP, ANY_UNDEF, BUCKET_FLAG_CONFIGURABLE, BUCKET_FLAG_ENUMERABLE,
    BUCKET_FLAG_WRITABLE, TAG_DYNOBJ,
};
use crate::probe::{bucket_flags, entries, probe};

unsafe extern "C" {
    /// torajs-anyvalue — NaN-box AnyValue tag decoder.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    /// torajs-anyvalue — NaN-box AnyValue value decoder.
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
}

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
    let ent = unsafe { entries(obj) };
    let v = unsafe { (*ent.add(p.entry as usize)).value_anyv };
    // A hole tombstone (deleted shadow-domain key — arguments
    // length / callee) is ABSENT to a get: the sentinel must never
    // surface as a value.
    if v == crate::layout::DYNOBJ_HOLE_SENTINEL {
        return ANY_UNDEF;
    }
    let tag = unsafe { __torajs_anyv_unbox_tag(v) } as u64;
    // Accessor entries store an `AccessorPair` cell — surface the
    // synthetic ANY_ACCESSOR sentinel so the SSA GET path dispatches
    // the getter instead of yielding the pair pointer. Only a Heap-tag
    // value can be an accessor, so the pointee type_tag read is gated.
    if tag == ANY_HEAP {
        let ptr = unsafe { __torajs_anyv_unbox_value(v) } as *const c_void;
        if !ptr.is_null() && unsafe { type_tag(ptr) } == TAG_ACCESSOR_PAIR {
            return ANY_ACCESSOR;
        }
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
    let v = unsafe { (*ent.add(p.entry as usize)).value_anyv };
    // Hole tombstone — absent (see the tag twin above).
    if v == crate::layout::DYNOBJ_HOLE_SENTINEL {
        return 0;
    }
    unsafe { __torajs_anyv_unbox_value(v) as u64 }
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
