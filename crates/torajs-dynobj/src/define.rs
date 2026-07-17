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
    DEFINE_PRESENT_VALUE, DEFINE_PRESENT_WRITABLE, DYNOBJ_HDR_FLAG_NON_EXTENSIBLE, TAG_ARR_HDR,
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
    /// torajs-arr — Array DefineOwnProperty kernel (RFC
    /// 20260712-arr-exotic-define chunk B receiver dispatch).
    fn __torajs_arr_define(
        arr: *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
}

/// `__torajs_dynobj_define(obj_slot, key, tag, value, flags_byte)`.
///
/// Thin extern wrapper over [`define_apply`] for the compile-time
/// literal-descriptor path (ssa_lower extracts the flags + value at
/// compile time). The runtime-descriptor path
/// (`define_from_desc.rs`) shares the same apply core.
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

/// Release the caller-transferred `[[Value]]` stake on a rejection
/// path. Every throw+return leg in [`define_apply`] /
/// [`redefine_entry`] leaves before the store that would consume the
/// value — without this the desc value stranded one rc per rejected
/// define (`value` is honored only under `DEFINE_PRESENT_VALUE`, so
/// legs gate on `has_value`).
pub(crate) unsafe fn drop_rejected_value(tag: u64, value: u64) {
    if (tag & BUCKET_TAG_MASK) == ANY_HEAP && value != 0 {
        unsafe { __torajs_value_drop_heap(value as *mut c_void) };
    }
}

/// Spec §10.1.6.3 ValidateAndApplyPropertyDescriptor — data-property
/// subset. Shared core for both the literal
/// ([`__torajs_dynobj_define`]) and runtime-descriptor
/// (`define_from_desc.rs`) entries.
///
/// # Safety
/// Same contract as [`__torajs_dynobj_define`]. When `flags_byte` sets
/// `DEFINE_PRESENT_VALUE` and `tag == ANY_HEAP`, the caller transfers
/// one rc of `value` (consumed on store, dropped on redefine, or
/// dropped on every rejection leg via [`drop_rejected_value`]).
pub(crate) unsafe fn define_apply(
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
    // RFC 20260712-arr-exotic-define chunk B — an Any receiver that
    // holds an Array cell routes to the Array DefineOwnProperty
    // kernel (§10.4.2.1). Pre-fix the Arr header (`len` @8) was read
    // as a dynobj header (count/cap @8) — silent corruption. The Arr
    // cell never relocates (grow swaps the data pointer), so no
    // slot writeback is needed.
    let htag = unsafe { (obj.cast::<u8>().add(4) as *const u16).read() };
    if htag == TAG_ARR_HDR {
        unsafe { __torajs_arr_define(obj, key, tag, value, flags_byte) };
        return;
    }
    // Primitive-wrapper receiver — §10.4.3.2 inherent-slot validation
    // / expando define (pre-fix the wrapper layout was walked as a
    // dynobj header: silent corruption, SIGSEGV on the first define).
    // The wrapper cell never relocates, so no slot writeback.
    if htag == crate::define_wrapper::TAG_NUMBER_WRAPPER
        || htag == crate::define_wrapper::TAG_STRING_WRAPPER
        || htag == crate::define_wrapper::TAG_BOOLEAN_WRAPPER
    {
        unsafe { crate::define_wrapper::wrapper_define(obj, key, tag, value, flags_byte) };
        return;
    }
    // Closure receiver (T-27 Function-as-Object) — its own
    // properties live in the lazy expando dynobj at +24; recurse
    // with the expando slot so the full §10.1.6.3 validate/apply
    // (non-configurable redefine rejection included) runs against
    // the entry table. Pre-fix the closure layout was walked as a
    // dynobj header (fn_addr read as count/cap) — SIGSEGV on the
    // first defineProperty against an any-typed function.
    if htag == crate::layout::TAG_CLOSURE_HDR {
        let props_slot =
            unsafe { obj.cast::<u8>().add(crate::layout::CELL_PROPS_OFF) } as *mut *mut c_void;
        unsafe {
            if (*props_slot).is_null() {
                *props_slot = crate::alloc::__torajs_dynobj_alloc();
            }
            define_apply(props_slot, key, tag, value, flags_byte);
        }
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

    let has_value = flags_byte & DEFINE_PRESENT_VALUE != 0;
    let desc_writable = flags_byte & DEFINE_FLAG_WRITABLE != 0;
    let desc_enumerable = flags_byte & DEFINE_FLAG_ENUMERABLE != 0;
    let desc_configurable = flags_byte & DEFINE_FLAG_CONFIGURABLE != 0;

    if pr.found {
        // Existing entry — §10.1.6.3 validate + apply, split out to
        // keep this dispatcher under the 200-line fn hard limit.
        unsafe { redefine_entry(ent.add(pr.entry as usize), tag, value, flags_byte) };
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
            if has_value {
                unsafe { drop_rejected_value(tag, value) };
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

/// Existing-entry arm of [`define_apply`] — validate the transition
/// against the current flags (§10.1.6.3 non-configurable rules) and
/// apply the per-flag fold + value swap in place.
///
/// # Safety
/// `e` points at a live entry of the probed dynobj; same `tag` /
/// `value` ownership contract as [`define_apply`].
unsafe fn redefine_entry(e: *mut Entry, tag: u64, value: u64, flags_byte: u64) {
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
            unsafe {
                __torajs_throw_type_error(
                    c"Attempting to change configurable attribute of unconfigurable property."
                        .as_ptr() as *const u8,
                );
            }
            if has_value {
                unsafe { drop_rejected_value(tag, value) };
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
            if has_value {
                unsafe { drop_rejected_value(tag, value) };
            }
            return;
        }
        // §10.1.6.3 step 4 — a data↔accessor kind switch on a
        // non-configurable property throws (either direction).
        if has_value && incoming_accessor != cur_accessor {
            unsafe {
                __torajs_throw_type_error(
                    c"Attempting to change access mechanism for an unconfigurable property."
                        .as_ptr() as *const u8,
                );
                drop_rejected_value(tag, value);
            }
            return;
        }
        if !incoming_accessor && !cur_accessor && !cur_writable {
            if has_writable && desc_writable {
                unsafe {
                    __torajs_throw_type_error(
                        c"Attempting to change writable attribute of unconfigurable property."
                            .as_ptr() as *const u8,
                    );
                }
                if has_value {
                    unsafe { drop_rejected_value(tag, value) };
                }
                return;
            }
            if has_value {
                // SameValue approximated by exact (tag, value) match.
                let cur_unboxed_value = unsafe { __torajs_anyv_unbox_value(cur_value_anyv) } as u64;
                let same = (tag & BUCKET_TAG_MASK) == cur_value_tag && value == cur_unboxed_value;
                if !same {
                    unsafe {
                        __torajs_throw_type_error(
                            c"Attempting to change value of a readonly property.".as_ptr()
                                as *const u8,
                        );
                        drop_rejected_value(tag, value);
                    }
                    return;
                }
            }
        }
    }

    // Accessor-over-accessor redefine (RFC 20260713 chunk D) —
    // §10.1.6.3 partial update; a rejection inside drops the fresh
    // pair and records the pending throw.
    if incoming_accessor
        && cur_accessor
        && !unsafe {
            merge_accessor_redefine(
                cur_value_anyv,
                value as *mut u8,
                flags_byte,
                cur_configurable,
            )
        }
    {
        return;
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
}

/// Accessor-over-accessor redefine merge (RFC 20260713 chunk D) —
/// §10.1.6.3 partial update: a face absent from the descriptor
/// (`DEFINE_PRESENT_GET` / `_SET` clear) inherits the current closure
/// and its kind byte (an explicit `undefined` face is present + NULL
/// and clears it); a non-configurable accessor rejects any face
/// change. Answers `false` on rejection (the fresh pair is dropped
/// and a TypeError is pending — the caller returns), `true` to
/// proceed with the ordinary apply.
///
/// # Safety
/// `cur_value_anyv` wraps a live `AccessorPair`; `new_pair` is a live
/// fresh `AccessorPair` whose ref the caller owns.
unsafe fn merge_accessor_redefine(
    cur_value_anyv: u64,
    new_pair: *mut u8,
    flags_byte: u64,
    cur_configurable: bool,
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
            unsafe {
                __torajs_throw_type_error(
                    c"Attempting to change access mechanism for an unconfigurable property."
                        .as_ptr() as *const u8,
                );
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
