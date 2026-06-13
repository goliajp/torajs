//! `Object.defineProperty(obj, key, descriptor)` — full attribute-flag
//! tracking path.
//!
//! Implements spec §10.1.6.3 ValidateAndApplyPropertyDescriptor for the
//! data-property subset (accessor descriptors not yet supported by tr).
//!
//! ## flags_byte layout
//! Low 3 bits = flag VALUE (writable / enumerable / configurable);
//! bits 3-5 = flag PRESENT in descriptor (distinguishes "absent" from
//! "present-false"); bit 6 = `[[Value]]` present.
//!
//! ## Validation rules (current.configurable=false branch)
//! - Reject upgrading configurable: false → true.
//! - Reject changing enumerable to a different value.
//! - With current.writable=false: reject upgrading writable false → true,
//!   AND reject a [[Value]] change unless SameValue (approximated via
//!   exact (tag, value) match — same heuristic as Any===Any).
//!
//! Each rejection records pending TypeError via TLS + returns — the
//! caller's ssa-lower-side `emit_throw_check` propagates. Matches
//! `feedback_throw_extern_returns_void`: throw extern is `()` not `-> !`.

use core::ffi::c_void;

use crate::layout::{
    ANY_HEAP, ANY_UNDEF, BUCKET_FLAG_CONFIGURABLE, BUCKET_FLAG_ENUMERABLE, BUCKET_FLAG_WRITABLE,
    BUCKET_KEY_PTR_MASK, BUCKET_TAG_MASK, DEFINE_FLAG_CONFIGURABLE, DEFINE_FLAG_ENUMERABLE,
    DEFINE_FLAG_WRITABLE, DEFINE_PRESENT_CONFIGURABLE, DEFINE_PRESENT_ENUMERABLE,
    DEFINE_PRESENT_VALUE, DEFINE_PRESENT_WRITABLE, DYNOBJ_HDR_FLAG_NON_EXTENSIBLE,
};
use crate::probe::{
    Entry, bucket_flags, bucket_make_key_tagged, count, entries, entries_cap, entries_len,
    index_ptr, probe, set_count, set_entries_len,
};
use crate::resize::resize;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const u8);
    fn __torajs_value_drop_heap(child: *mut c_void);
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_anyv_to_bool(v: u64) -> bool;
}

/// `__torajs_dynobj_define(obj_slot, key, tag, value, flags_byte)`.
///
/// Thin extern wrapper over [`define_apply`] for the compile-time
/// literal-descriptor path (ssa_lower extracts the flags + value at
/// compile time). The runtime-descriptor path
/// ([`__torajs_dynobj_define_from_desc`]) shares the same apply core.
///
/// # Safety
/// `obj_slot` is non-NULL and points at a live `*mut c_void` holding
/// a dynobj or NULL. `key` is a live Str heap pointer. `tag` / `value`
/// honored only when bit 6 (`DEFINE_PRESENT_VALUE`) of `flags_byte`
/// is set. Caller must check for pending throw after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_define(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
) {
    unsafe { define_apply(obj_slot, key, tag, value, flags_byte) }
}

/// Spec §10.1.6.3 ValidateAndApplyPropertyDescriptor — data-property
/// subset. Shared core for both the literal
/// ([`__torajs_dynobj_define`]) and runtime-descriptor
/// ([`__torajs_dynobj_define_from_desc`]) entries.
///
/// # Safety
/// Same contract as [`__torajs_dynobj_define`]. When `flags_byte` sets
/// `DEFINE_PRESENT_VALUE` and `tag == ANY_HEAP`, the caller transfers
/// one rc of `value` (consumed on store / dropped on redefine).
unsafe fn define_apply(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
) {
    let mut obj = unsafe { *obj_slot };
    if obj.is_null() {
        return;
    }
    // Dense-array-full guard — same shape as set.rs.
    if unsafe { entries_len(obj) } == unsafe { entries_cap(obj) } {
        unsafe {
            resize(obj_slot);
            obj = *obj_slot;
        }
    }

    let pr = unsafe { probe(obj, key as *const c_void) };
    let ent = unsafe { entries(obj) };

    let has_writable = flags_byte & DEFINE_PRESENT_WRITABLE != 0;
    let has_enumerable = flags_byte & DEFINE_PRESENT_ENUMERABLE != 0;
    let has_configurable = flags_byte & DEFINE_PRESENT_CONFIGURABLE != 0;
    let has_value = flags_byte & DEFINE_PRESENT_VALUE != 0;
    let desc_writable = flags_byte & DEFINE_FLAG_WRITABLE != 0;
    let desc_enumerable = flags_byte & DEFINE_FLAG_ENUMERABLE != 0;
    let desc_configurable = flags_byte & DEFINE_FLAG_CONFIGURABLE != 0;

    if pr.found {
        let e = unsafe { ent.add(pr.entry as usize) };
        let cur_kp_tagged = unsafe { (*e).key_ptr_tagged };
        let cur_value_anyv = unsafe { (*e).value_anyv };
        let cur_flags = bucket_flags(cur_kp_tagged);
        let cur_writable = cur_flags & BUCKET_FLAG_WRITABLE != 0;
        let cur_enumerable = cur_flags & BUCKET_FLAG_ENUMERABLE != 0;
        let cur_configurable = cur_flags & BUCKET_FLAG_CONFIGURABLE != 0;
        let cur_value_tag = unsafe { __torajs_anyv_unbox_tag(cur_value_anyv) } as u64;

        if !cur_configurable {
            // Spec §10.1.6.3 — non-configurable entry; reject diverging
            // present-flag changes.
            if has_configurable && desc_configurable && !cur_configurable {
                unsafe {
                    __torajs_throw_type_error(
                        c"Attempting to change configurable attribute of unconfigurable property."
                            .as_ptr() as *const u8,
                    );
                }
                return;
            }
            if has_enumerable && desc_enumerable != cur_enumerable {
                unsafe {
                    __torajs_throw_type_error(
                        c"Attempting to change enumerable attribute of unconfigurable property."
                            .as_ptr() as *const u8,
                    );
                }
                return;
            }
            if !cur_writable {
                if has_writable && desc_writable {
                    unsafe {
                        __torajs_throw_type_error(
                            c"Attempting to change writable attribute of unconfigurable property."
                                .as_ptr() as *const u8,
                        );
                    }
                    return;
                }
                if has_value {
                    // SameValue approximated by exact (tag, value) match.
                    let cur_unboxed_value =
                        unsafe { __torajs_anyv_unbox_value(cur_value_anyv) } as u64;
                    let same =
                        (tag & BUCKET_TAG_MASK) == cur_value_tag && value == cur_unboxed_value;
                    if !same {
                        unsafe {
                            __torajs_throw_type_error(
                                c"Attempting to change value of a readonly property.".as_ptr()
                                    as *const u8,
                            );
                        }
                        return;
                    }
                }
            }
        }

        // Validation passed — apply. Drop the old heap value first if
        // the new descriptor brings a fresh [[Value]] over an ANY_HEAP slot.
        if has_value && cur_value_tag == ANY_HEAP {
            unsafe {
                __torajs_value_drop_heap(cur_value_anyv as *mut c_void);
            }
        }

        // Per-flag fold: present → take desc value; absent → preserve current.
        let mut new_flags: u64 = 0;
        new_flags |= if has_writable {
            if desc_writable {
                BUCKET_FLAG_WRITABLE
            } else {
                0
            }
        } else if cur_writable {
            BUCKET_FLAG_WRITABLE
        } else {
            0
        };
        new_flags |= if has_enumerable {
            if desc_enumerable {
                BUCKET_FLAG_ENUMERABLE
            } else {
                0
            }
        } else if cur_enumerable {
            BUCKET_FLAG_ENUMERABLE
        } else {
            0
        };
        new_flags |= if has_configurable {
            if desc_configurable {
                BUCKET_FLAG_CONFIGURABLE
            } else {
                0
            }
        } else if cur_configurable {
            BUCKET_FLAG_CONFIGURABLE
        } else {
            0
        };

        let new_value_tag = if has_value {
            tag & BUCKET_TAG_MASK
        } else {
            cur_value_tag
        };
        let new_value = if has_value {
            value
        } else {
            unsafe { __torajs_anyv_unbox_value(cur_value_anyv) as u64 }
        };

        // Preserve the existing key pointer (re-pack with new flags);
        // rebox the (tag, value) pair into a fresh NaN-box AnyValue.
        let cur_key_ptr = (cur_kp_tagged & BUCKET_KEY_PTR_MASK) as *mut c_void;
        unsafe {
            (*e).key_ptr_tagged = bucket_make_key_tagged(cur_key_ptr, new_flags);
            (*e).value_anyv = __torajs_anyv_box_from_pair(new_value_tag as i64, new_value as i64);
        }
    } else {
        // RFC C5b-4 — Object.defineProperty(O, "newKey", desc) on a
        // sealed / non-extensible dict must throw TypeError. Existing-
        // key redefine is gated by the `cur_configurable` branch above;
        // this catches the "add a new own property" path.
        let header_flags = unsafe { *(obj.cast::<u8>().add(6) as *const u16) };
        if header_flags & DYNOBJ_HDR_FLAG_NON_EXTENSIBLE != 0 {
            // Matches bun's exact wording for cross-runtime parity.
            unsafe {
                __torajs_throw_type_error(
                    c"Attempting to define property on object that is not extensible.".as_ptr()
                        as *const u8,
                );
            }
            return;
        }
        // Fresh define: append to the dense array (insertion order).
        // Absent flags default to false (spec §10.1.6.2).
        let mut new_flags: u64 = 0;
        if desc_writable {
            new_flags |= BUCKET_FLAG_WRITABLE;
        }
        if desc_enumerable {
            new_flags |= BUCKET_FLAG_ENUMERABLE;
        }
        if desc_configurable {
            new_flags |= BUCKET_FLAG_CONFIGURABLE;
        }
        let (init_tag, init_value) = if has_value {
            (tag & BUCKET_TAG_MASK, value)
        } else {
            // No .value present — default [[Value]] to undefined.
            (ANY_UNDEF, 0)
        };
        let e_idx = unsafe { entries_len(obj) };
        unsafe {
            __torajs_rc_inc(key);
            *ent.add(e_idx as usize) = Entry {
                key_ptr_tagged: bucket_make_key_tagged(key, new_flags),
                value_anyv: __torajs_anyv_box_from_pair(init_tag as i64, init_value as i64),
            };
            *index_ptr(obj).add(pr.slot as usize) = e_idx;
            set_entries_len(obj, e_idx + 1);
            set_count(obj, count(obj) + 1);
        }
    }
}

/// Stack-allocated Str-shaped probe key. [`probe`] / `hash_str` /
/// `str_eq` only read `len` (offset 8) and the inline payload (offset
/// 16) — never the heap header — so a non-heap buffer with those two
/// fields suffices to look a property name up in a dynobj without
/// allocating (or interning) a real Str. Field names are short; a
/// 16-byte inline payload covers every descriptor key.
#[repr(C, align(8))]
struct FakeStrKey {
    _header: u64,
    len: u64,
    data: [u8; 16],
}

impl FakeStrKey {
    #[inline]
    fn new(name: &str) -> FakeStrKey {
        let mut k = FakeStrKey {
            _header: 0,
            len: name.len() as u64,
            data: [0u8; 16],
        };
        k.data[..name.len()].copy_from_slice(name.as_bytes());
        k
    }
}

/// Look a property name up in `desc` and return its NaN-box
/// `value_anyv` if present (the property's stored AnyValue).
///
/// # Safety
/// `desc` points at a live dynobj heap block.
#[inline]
unsafe fn desc_field(desc: *const c_void, name: &str) -> Option<u64> {
    let probe_key = FakeStrKey::new(name);
    let pr = unsafe { probe(desc, &probe_key as *const FakeStrKey as *const c_void) };
    if !pr.found {
        return None;
    }
    let ent = unsafe { entries(desc) };
    Some(unsafe { (*ent.add(pr.entry as usize)).value_anyv })
}

/// `__torajs_dynobj_define_from_desc(obj_slot, key, desc)` — the
/// runtime-descriptor path for `Object.defineProperty`. Reads the
/// data-descriptor fields (`value` / `writable` / `enumerable` /
/// `configurable`) off the `desc` dynobj at runtime, builds the
/// `flags_byte` + `(tag, value)` the compile-time literal path
/// produces, and applies via [`define_apply`].
///
/// Accessor fields (`get` / `set`) are a follow-up substrate piece
/// (RFC C3) — a descriptor carrying only accessors currently defines a
/// generic property with `undefined` value.
///
/// # Safety
/// `obj_slot` points at a live `*mut c_void` (dynobj or NULL). `key`
/// is a live Str. `desc` is a dynobj heap pointer or NULL. Caller must
/// check for pending throw after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_define_from_desc(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    desc: *const c_void,
) {
    if desc.is_null() {
        return;
    }

    let mut flags_byte: u64 = 0;
    let mut out_tag: u64 = 0;
    let mut out_value: u64 = 0;

    if let Some(v_anyv) = unsafe { desc_field(desc, "value") } {
        let v_tag = unsafe { __torajs_anyv_unbox_tag(v_anyv) } as u64;
        let v_val = unsafe { __torajs_anyv_unbox_value(v_anyv) } as u64;
        // define_apply consumes one rc of a Heap value (it is stored
        // into `obj` while still owned by `desc`) — mirror the literal
        // path's `pack` rc_inc.
        if v_tag == ANY_HEAP && v_val != 0 {
            unsafe { __torajs_rc_inc(v_val as *mut c_void) };
        }
        out_tag = v_tag;
        out_value = v_val;
        flags_byte |= DEFINE_PRESENT_VALUE;
    }

    if let Some(w) = unsafe { desc_field(desc, "writable") } {
        flags_byte |= DEFINE_PRESENT_WRITABLE;
        if unsafe { __torajs_anyv_to_bool(w) } {
            flags_byte |= DEFINE_FLAG_WRITABLE;
        }
    }
    if let Some(e) = unsafe { desc_field(desc, "enumerable") } {
        flags_byte |= DEFINE_PRESENT_ENUMERABLE;
        if unsafe { __torajs_anyv_to_bool(e) } {
            flags_byte |= DEFINE_FLAG_ENUMERABLE;
        }
    }
    if let Some(c) = unsafe { desc_field(desc, "configurable") } {
        flags_byte |= DEFINE_PRESENT_CONFIGURABLE;
        if unsafe { __torajs_anyv_to_bool(c) } {
            flags_byte |= DEFINE_FLAG_CONFIGURABLE;
        }
    }

    unsafe { define_apply(obj_slot, key, out_tag, out_value, flags_byte) }
}
