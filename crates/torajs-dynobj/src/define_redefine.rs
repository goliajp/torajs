//! Existing-entry arm of [`crate::define::define_apply`] — split from
//! `define.rs` (file-size hard limit; rotation 267 刀 R5a added the
//! `throw_on_refusal` parameterization + soft shell and pushed the
//! shared file over 500).

use core::ffi::c_void;

use crate::define::refuse;
use crate::layout::{
    ANY_HEAP, BUCKET_FLAG_CONFIGURABLE, BUCKET_FLAG_ENUMERABLE, BUCKET_FLAG_WRITABLE,
    BUCKET_KEY_PTR_MASK, BUCKET_TAG_MASK, DEFINE_FLAG_CONFIGURABLE, DEFINE_FLAG_ENUMERABLE,
    DEFINE_FLAG_WRITABLE, DEFINE_PRESENT_CONFIGURABLE, DEFINE_PRESENT_ENUMERABLE,
    DEFINE_PRESENT_VALUE, DEFINE_PRESENT_WRITABLE,
};
use crate::probe::{Entry, bucket_flags, bucket_make_key_tagged};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const u8);
    fn __torajs_value_drop_heap(child: *mut c_void);
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// torajs-anyvalue — §7.2.10 SameValue over two boxed values.
    fn __torajs_anyv_same_value(l: u64, r: u64) -> bool;
}

/// Existing-entry arm of [`crate::define::define_apply`] — validate
/// the transition against the current flags (§10.1.6.3
/// non-configurable rules) and apply the per-flag fold + value swap
/// in place. Answers 1 on success, 0 on a refusal (recorded as a
/// TypeError only for the throwing flavor).
///
/// # Safety
/// `e` points at a live entry of the probed dynobj; same `tag` /
/// `value` ownership contract as `define_apply`.
/// §10.4.6.6 — a module namespace's [[DefineOwnProperty]] on an
/// EXISTING key. The ordinary path below accepts several descriptors
/// that the exotic one must reject: an export is
/// `{ writable: true, enumerable: true, configurable: false }`, and an
/// ordinary non-configurable-but-writable entry legitimately takes a
/// value change. Through a namespace nothing may change — the
/// descriptor is allowed only to restate what is already there.
///
/// Steps 4-8 of the spec text, in order. Step 3 (`current` is
/// undefined) never reaches here: a fresh key is already refused by
/// the non-extensible bit `__torajs_module_ns_finalize` sets.
///
/// # Safety
/// `e` is a live entry in the receiver's table.
pub(crate) unsafe fn module_ns_refuses(
    e: *mut Entry,
    tag: u64,
    value: u64,
    flags_byte: u64,
) -> bool {
    // 4. Desc has [[Configurable]] and it is true.
    if flags_byte & DEFINE_PRESENT_CONFIGURABLE != 0 && flags_byte & DEFINE_FLAG_CONFIGURABLE != 0 {
        return true;
    }
    // 5. Desc has [[Enumerable]] and it is false.
    if flags_byte & DEFINE_PRESENT_ENUMERABLE != 0 && flags_byte & DEFINE_FLAG_ENUMERABLE == 0 {
        return true;
    }
    // 6. IsAccessorDescriptor(Desc).
    if flags_byte & (crate::layout::DEFINE_PRESENT_GET | crate::layout::DEFINE_PRESENT_SET) != 0 {
        return true;
    }
    // 7. Desc has [[Writable]] and it is false.
    if flags_byte & DEFINE_PRESENT_WRITABLE != 0 && flags_byte & DEFINE_FLAG_WRITABLE == 0 {
        return true;
    }
    // 8. Desc has [[Value]] — SameValue against the current one.
    //    Bit-pattern equality IS SameValue on this lane: NaN matches
    //    NaN (same bits) and +0 does not match -0 (different bits),
    //    which is precisely what SameValue asks for and what `===`
    //    would get wrong in both directions.
    if flags_byte & DEFINE_PRESENT_VALUE != 0 {
        let cur_anyv = unsafe { (*e).value_anyv };
        let cur_tag = unsafe { __torajs_anyv_unbox_tag(cur_anyv) } as u64;
        let cur_val = unsafe { __torajs_anyv_unbox_value(cur_anyv) } as u64;
        return cur_tag != (tag & BUCKET_TAG_MASK) || cur_val != value;
    }
    // 9. Nothing was asked to change.
    false
}

pub(crate) unsafe fn redefine_entry(
    e: *mut Entry,
    tag: u64,
    value: u64,
    flags_byte: u64,
    throw_on_refusal: bool,
) -> i64 {
    let has_writable = flags_byte & DEFINE_PRESENT_WRITABLE != 0;
    let has_enumerable = flags_byte & DEFINE_PRESENT_ENUMERABLE != 0;
    let has_configurable = flags_byte & DEFINE_PRESENT_CONFIGURABLE != 0;
    let has_value = flags_byte & DEFINE_PRESENT_VALUE != 0;
    let desc_writable = flags_byte & DEFINE_FLAG_WRITABLE != 0;
    let desc_enumerable = flags_byte & DEFINE_FLAG_ENUMERABLE != 0;
    let desc_configurable = flags_byte & DEFINE_FLAG_CONFIGURABLE != 0;
    let cur_kp_tagged = unsafe { (*e).key_ptr_tagged };
    let cur_value_anyv = unsafe { (*e).value_anyv };
    let cur_flags = bucket_flags(cur_kp_tagged);
    let cur_writable = cur_flags & BUCKET_FLAG_WRITABLE != 0;
    let cur_enumerable = cur_flags & BUCKET_FLAG_ENUMERABLE != 0;
    let cur_configurable = cur_flags & BUCKET_FLAG_CONFIGURABLE != 0;
    let cur_value_tag = unsafe { __torajs_anyv_unbox_tag(cur_value_anyv) } as u64;
    // Descriptor kind split (RFC 20260713 residual fix-up) — the
    // data-only writable/value rules must not fire on accessor pairs
    // (a fresh pair never SameValue-matches the current one, so a
    // same-faces redefine of a non-configurable accessor wrongly
    // threw "readonly").
    let incoming_accessor = has_value
        && (tag & BUCKET_TAG_MASK) == ANY_HEAP
        && value != 0
        && unsafe { (value as *const u8).add(4).cast::<u16>().read() }
            == crate::accessor::TAG_ACCESSOR_PAIR;
    let cur_accessor = unsafe { crate::accessor::value_is_accessor(cur_value_anyv) };

    if !cur_configurable {
        // Spec §10.1.6.3 — non-configurable entry; reject diverging
        // present-flag changes.
        if has_configurable && desc_configurable && !cur_configurable {
            return unsafe {
                refuse(
                    throw_on_refusal,
                    c"Attempting to change configurable attribute of unconfigurable property."
                        .as_ptr() as *const u8,
                    has_value,
                    tag,
                    value,
                )
            };
        }
        if has_enumerable && desc_enumerable != cur_enumerable {
            return unsafe {
                refuse(
                    throw_on_refusal,
                    c"Attempting to change enumerable attribute of unconfigurable property."
                        .as_ptr() as *const u8,
                    has_value,
                    tag,
                    value,
                )
            };
        }
        // §10.1.6.3 step 4 — a data↔accessor kind switch on a
        // non-configurable property refuses (either direction).
        if has_value && incoming_accessor != cur_accessor {
            return unsafe {
                refuse(
                    throw_on_refusal,
                    c"Attempting to change access mechanism for an unconfigurable property."
                        .as_ptr() as *const u8,
                    has_value,
                    tag,
                    value,
                )
            };
        }
        if !incoming_accessor && !cur_accessor && !cur_writable {
            if has_writable && desc_writable {
                return unsafe {
                    refuse(
                        throw_on_refusal,
                        c"Attempting to change writable attribute of unconfigurable property."
                            .as_ptr() as *const u8,
                        has_value,
                        tag,
                        value,
                    )
                };
            }
            if has_value {
                // §10.1.6.3 step 6.b — true SameValue (a fresh Str
                // cell with equal bytes IS the same value; the old
                // exact-pointer approximation rejected `{value:
                // "abcd"}` redefined with another "abcd").
                let incoming = unsafe {
                    __torajs_anyv_box_from_pair((tag & BUCKET_TAG_MASK) as i64, value as i64)
                };
                let same = unsafe { __torajs_anyv_same_value(incoming, cur_value_anyv) };
                if !same {
                    return unsafe {
                        refuse(
                            throw_on_refusal,
                            c"Attempting to change value of a readonly property.".as_ptr()
                                as *const u8,
                            has_value,
                            tag,
                            value,
                        )
                    };
                }
            }
        }
    }

    // Accessor-over-accessor redefine (RFC 20260713 chunk D) —
    // §10.1.6.3 partial update; a rejection inside drops the fresh
    // pair and answers per flavor (R5b).
    if incoming_accessor
        && cur_accessor
        && !unsafe {
            merge_accessor_redefine(
                cur_value_anyv,
                value as *mut u8,
                flags_byte,
                cur_configurable,
                throw_on_refusal,
            )
        }
    {
        return 0;
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
    1
}

/// Accessor-over-accessor redefine merge (RFC 20260713 chunk D) —
/// §10.1.6.3 partial update: a face absent from the descriptor
/// (`DEFINE_PRESENT_GET` / `_SET` clear) inherits the current closure
/// and its kind byte (an explicit `undefined` face is present + NULL
/// and clears it); a non-configurable accessor rejects any face
/// change. Answers `false` on rejection (the fresh pair is dropped;
/// the TypeError records only for the throwing flavor — the caller
/// returns 0), `true` to proceed with the ordinary apply.
///
/// # Safety
/// `cur_value_anyv` wraps a live `AccessorPair`; `new_pair` is a live
/// fresh `AccessorPair` whose ref the caller owns.
unsafe fn merge_accessor_redefine(
    cur_value_anyv: u64,
    new_pair: *mut u8,
    flags_byte: u64,
    cur_configurable: bool,
    throw_on_refusal: bool,
) -> bool {
    use crate::accessor::{ACC_GET_OFF, ACC_KINDS_OFF, ACC_SET_OFF};
    let cur_pair = unsafe { __torajs_anyv_unbox_value(cur_value_anyv) } as *const u8;
    let has_get = flags_byte & crate::layout::DEFINE_PRESENT_GET != 0;
    let has_set = flags_byte & crate::layout::DEFINE_PRESENT_SET != 0;
    let cur_get = unsafe { cur_pair.add(ACC_GET_OFF).cast::<u64>().read() };
    let cur_set = unsafe { cur_pair.add(ACC_SET_OFF).cast::<u64>().read() };
    let cur_kinds = unsafe { cur_pair.add(ACC_KINDS_OFF).cast::<u64>().read() };
    if !cur_configurable {
        let new_get = unsafe { new_pair.add(ACC_GET_OFF).cast::<u64>().read() };
        let new_set = unsafe { new_pair.add(ACC_SET_OFF).cast::<u64>().read() };
        if (has_get && new_get != cur_get) || (has_set && new_set != cur_set) {
            unsafe { __torajs_value_drop_heap(new_pair as *mut c_void) };
            if throw_on_refusal {
                unsafe {
                    __torajs_throw_type_error(
                        c"Attempting to change access mechanism for an unconfigurable property."
                            .as_ptr() as *const u8,
                    );
                }
            }
            return false;
        }
    }
    unsafe {
        let kinds_p = new_pair.add(ACC_KINDS_OFF).cast::<u64>();
        if !has_get && cur_get != 0 {
            __torajs_rc_inc(cur_get as *mut c_void);
            new_pair.add(ACC_GET_OFF).cast::<u64>().write(cur_get);
            kinds_p.write((kinds_p.read() & !0xFF) | (cur_kinds & 0xFF));
        }
        if !has_set && cur_set != 0 {
            __torajs_rc_inc(cur_set as *mut c_void);
            new_pair.add(ACC_SET_OFF).cast::<u64>().write(cur_set);
            kinds_p.write((kinds_p.read() & !0xFF00) | (cur_kinds & 0xFF00));
        }
    }
    true
}
