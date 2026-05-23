//! `Object.defineProperty(obj, key, descriptor)` — full attribute-flag
//! tracking path.
//!
//! Port of `runtime_str.c::__torajs_dynobj_define` (P4.2-d, 2026-05-23).
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
    BUCKET_TAG_MASK, DEFINE_FLAG_CONFIGURABLE, DEFINE_FLAG_ENUMERABLE, DEFINE_FLAG_WRITABLE,
    DEFINE_PRESENT_CONFIGURABLE, DEFINE_PRESENT_ENUMERABLE, DEFINE_PRESENT_VALUE,
    DEFINE_PRESENT_WRITABLE, DYNOBJ_TOMBSTONE,
};
use crate::probe::{buckets, probe};
use crate::resize::resize;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const u8);
    fn __torajs_value_drop_heap(child: *mut c_void);
}

/// `__torajs_dynobj_define(obj_slot, key, tag, value, flags_byte)`.
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
    let mut obj = unsafe { *obj_slot };
    if obj.is_null() {
        return;
    }
    let cap = unsafe { *((obj as *const u8).add(12) as *const u32) };
    let count = unsafe { *((obj as *const u8).add(8) as *const u32) };
    let tomb = unsafe { *((obj as *const u8).add(16) as *const u32) };
    if (count + tomb + 1) * 8 > cap * 7 {
        unsafe {
            resize(obj_slot, cap * 2);
            obj = *obj_slot;
        }
    }

    let pr = unsafe { probe(obj, key as *const c_void) };
    let bk = unsafe { buckets(obj) };

    let has_writable = flags_byte & DEFINE_PRESENT_WRITABLE != 0;
    let has_enumerable = flags_byte & DEFINE_PRESENT_ENUMERABLE != 0;
    let has_configurable = flags_byte & DEFINE_PRESENT_CONFIGURABLE != 0;
    let has_value = flags_byte & DEFINE_PRESENT_VALUE != 0;
    let desc_writable = flags_byte & DEFINE_FLAG_WRITABLE != 0;
    let desc_enumerable = flags_byte & DEFINE_FLAG_ENUMERABLE != 0;
    let desc_configurable = flags_byte & DEFINE_FLAG_CONFIGURABLE != 0;

    if pr.found {
        let cur_tag = unsafe { (*bk.add(pr.idx as usize)).tag };
        let cur_writable = cur_tag & BUCKET_FLAG_WRITABLE != 0;
        let cur_enumerable = cur_tag & BUCKET_FLAG_ENUMERABLE != 0;
        let cur_configurable = cur_tag & BUCKET_FLAG_CONFIGURABLE != 0;
        let cur_value_tag = cur_tag & BUCKET_TAG_MASK;

        if !cur_configurable {
            // Spec §10.1.6.3 — non-configurable bucket; reject diverging
            // present-flag changes.
            if has_configurable && desc_configurable && !cur_configurable {
                unsafe {
                    __torajs_throw_type_error(
                        c"TypeError: Cannot redefine property: configurable was false".as_ptr()
                            as *const u8,
                    );
                }
                return;
            }
            if has_enumerable && desc_enumerable != cur_enumerable {
                unsafe {
                    __torajs_throw_type_error(
                        c"TypeError: Cannot redefine property: enumerable mismatch".as_ptr()
                            as *const u8,
                    );
                }
                return;
            }
            if !cur_writable {
                if has_writable && desc_writable {
                    unsafe {
                        __torajs_throw_type_error(
                            c"TypeError: Cannot redefine property: writable was false".as_ptr()
                                as *const u8,
                        );
                    }
                    return;
                }
                if has_value {
                    // SameValue approximated by exact (tag, value) match.
                    let same = (tag & BUCKET_TAG_MASK) == cur_value_tag
                        && value == unsafe { (*bk.add(pr.idx as usize)).value };
                    if !same {
                        unsafe {
                            __torajs_throw_type_error(
                                c"TypeError: Cannot redefine property: writable was false, value mismatch".as_ptr() as *const u8,
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
            let old_val = unsafe { (*bk.add(pr.idx as usize)).value as *mut c_void };
            unsafe {
                __torajs_value_drop_heap(old_val);
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
            unsafe { (*bk.add(pr.idx as usize)).value }
        };

        unsafe {
            (*bk.add(pr.idx as usize)).tag = new_value_tag | new_flags;
            (*bk.add(pr.idx as usize)).value = new_value;
        }
    } else {
        // Fresh define. Absent flags default to false (spec §10.1.6.2).
        if unsafe { (*bk.add(pr.idx as usize)).key_ptr } == DYNOBJ_TOMBSTONE {
            unsafe {
                *((obj as *mut u8).add(16) as *mut u32) = tomb - 1;
            }
        }
        unsafe {
            __torajs_rc_inc(key);
        }
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
        unsafe {
            (*bk.add(pr.idx as usize)).key_ptr = key;
            if has_value {
                (*bk.add(pr.idx as usize)).tag = (tag & BUCKET_TAG_MASK) | new_flags;
                (*bk.add(pr.idx as usize)).value = value;
            } else {
                // No .value present — default [[Value]] to undefined.
                (*bk.add(pr.idx as usize)).tag = ANY_UNDEF | new_flags;
                (*bk.add(pr.idx as usize)).value = 0;
            }
            *((obj as *mut u8).add(8) as *mut u32) = count + 1;
        }
    }
}
