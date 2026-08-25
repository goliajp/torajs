//! What a define WRITES once it has decided to — split out of
//! `define.rs` (file size: the §10.5.6 Proxy arm tipped the host
//! past the 500-line cap).
//!
//! The parent answers whether a define happens at all (§10.1.6.3
//! ValidateAndApplyPropertyDescriptor, the receiver dispatch, the
//! refusals). This file answers the other half: what a function
//! cell's §20.2.4 virtual `name` / `length` / `prototype` become
//! when a define is the first thing to materialize one, and what a
//! fresh entry looks like when the key had none.

use core::ffi::c_void;

use crate::define::{
    __torajs_closure_length, __torajs_closure_name_str, ANY_I64, FN_LENGTH_DELETED,
    FN_NAME_DELETED, define_apply, refuse,
};
use crate::layout::{
    ANY_HEAP, ANY_UNDEF, BUCKET_FLAG_CONFIGURABLE, BUCKET_FLAG_ENUMERABLE, BUCKET_FLAG_WRITABLE,
    BUCKET_TAG_MASK, DEFINE_FLAG_CONFIGURABLE, DEFINE_FLAG_ENUMERABLE, DEFINE_FLAG_WRITABLE,
    DEFINE_PRESENT_VALUE, DYNOBJ_HDR_FLAG_NON_EXTENSIBLE,
};
use crate::probe::{
    Entry, bucket_make_key_tagged, count, entries, entries_len, index_ptr, key_str_bytes, probe,
    set_count, set_entries_len,
};
unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    /// torajs-rc — builtin-prototype-singleton own-write note (same
    /// hook the front door's dynobj tail records; see
    /// [`__torajs_dynobj_define_plain`]).
    fn __torajs_builtin_proto_note_own_write(obj: *const c_void, name: *const u8, len: i64);
}

/// Lazily materialize a function's virtual own `length` / `name`
/// into the expando ahead of a defineProperty against it, so
/// §10.1.6.3 validates the user descriptor against the REAL current
/// attributes — §20.2.3: non-writable, non-enumerable,
/// CONFIGURABLE — instead of treating the define as a brand-new
/// property, whose absent fields default false (a partial-descriptor
/// define then bricked the slot: a later redefine met
/// configurable=false and was refused where bun accepts it).
///
/// Skips symbol keys / other names, an existing expando entry (the
/// current attributes already live there), a tombstoned slot
/// (`delete f.length` removed the current), and a metadata miss.
pub(crate) unsafe fn seed_virtual_fn_prop(
    fn_cell: *mut c_void,
    props_slot: *mut *mut c_void,
    key: *mut c_void,
) {
    unsafe {
        let Some((data, len)) = key_str_bytes(key as *const c_void) else {
            return;
        };
        let name = core::slice::from_raw_parts(data, len as usize);
        let is_length = name == b"length";
        if !is_length && name != b"name" {
            return;
        }
        if probe(*props_slot, key as *const c_void).found {
            return;
        }
        let hflags = *(fn_cell.cast::<u8>().add(6) as *const u16);
        let tomb = if is_length {
            FN_LENGTH_DELETED
        } else {
            FN_NAME_DELETED
        };
        if hflags & tomb != 0 {
            return;
        }
        let (tag, value) = if is_length {
            let l = __torajs_closure_length(fn_cell);
            if l < 0 {
                return;
            }
            (ANY_I64, l as u64)
        } else {
            let s = __torajs_closure_name_str(fn_cell);
            if s.is_null() {
                return;
            }
            // Owned Str transfers into the seeded entry's value.
            (ANY_HEAP, s as u64)
        };
        let flags = DEFINE_PRESENT_VALUE
            | crate::layout::DEFINE_PRESENT_WRITABLE
            | crate::layout::DEFINE_PRESENT_ENUMERABLE
            | crate::layout::DEFINE_PRESENT_CONFIGURABLE
            | DEFINE_FLAG_CONFIGURABLE;
        define_apply(props_slot, key, tag, value, flags, false);
    }
}

/// The absent-key half of [`define_apply`]'s dynobj tail — the
/// non-extensible refusal + fresh append (insertion order, absent
/// flags defaulting false per §10.1.6.2). Split out at the 200-line
/// fn cap; body verbatim.
pub(crate) unsafe fn define_fresh_entry(
    obj: *mut c_void,
    key: *mut c_void,
    slot: u32,
    tag: u64,
    value: u64,
    flags_byte: u64,
    throw_on_refusal: bool,
) -> i64 {
    let has_value = flags_byte & DEFINE_PRESENT_VALUE != 0;
    // RFC C5b-4 — Object.defineProperty(O, "newKey", desc) on a
    // sealed / non-extensible dict must throw TypeError (wording
    // matches bun for cross-runtime parity). Existing-key redefine
    // is gated by the `cur_configurable` branch above; this
    // catches the "add a new own property" path.
    let header_flags = unsafe { *(obj.cast::<u8>().add(6) as *const u16) };
    if header_flags & DYNOBJ_HDR_FLAG_NON_EXTENSIBLE != 0 {
        return unsafe {
            refuse(
                throw_on_refusal,
                c"Attempting to define property on object that is not extensible.".as_ptr()
                    as *const u8,
                has_value,
                tag,
                value,
            )
        };
    }
    // Fresh define: append to the dense array (insertion order).
    // Absent flags default to false (spec §10.1.6.2).
    let mut new_flags: u64 = 0;
    if flags_byte & DEFINE_FLAG_WRITABLE != 0 {
        new_flags |= BUCKET_FLAG_WRITABLE;
    }
    if flags_byte & DEFINE_FLAG_ENUMERABLE != 0 {
        new_flags |= BUCKET_FLAG_ENUMERABLE;
    }
    if flags_byte & DEFINE_FLAG_CONFIGURABLE != 0 {
        new_flags |= BUCKET_FLAG_CONFIGURABLE;
    }
    let (init_tag, init_value) = if has_value {
        (tag & BUCKET_TAG_MASK, value)
    } else {
        // No .value present — default [[Value]] to undefined.
        (ANY_UNDEF, 0)
    };
    unsafe {
        let ent = entries(obj);
        let e_idx = entries_len(obj);
        __torajs_rc_inc(key);
        *ent.add(e_idx as usize) = Entry {
            key_ptr_tagged: bucket_make_key_tagged(key, new_flags),
            value_anyv: __torajs_anyv_box_from_pair(init_tag as i64, init_value as i64),
        };
        *index_ptr(obj).add(slot as usize) = e_idx;
        set_entries_len(obj, e_idx + 1);
        set_count(obj, count(obj) + 1);
    }
    1
}

/// Assembly-path define — the narrow kernel the INSTALL world calls
/// (RFC 20260825-inject-narrow-define 刀 1). The general
/// `__torajs_dynobj_define` front door dispatches on receiver shape
/// (Proxy / Array / wrapper / closure / promise / buffer / struct)
/// before reaching the dynobj entry table, and each of those arms
/// statically roots a whole runtime universe the reachability
/// closure cannot sever — an empty program paid for the proxy,
/// accessor-invoke and to_primitive worlds because the builtin-class
/// injection sequence installed its methods through the front door.
/// The injection mint sites know every arm's premise is false at
/// compile time: the receiver is a plain dynobj they just minted (or
/// the class/proto singleton the register sequence filled), so this
/// kernel goes straight to the entry table.
///
/// Caller contract (NOT validated beyond the debug assert): the
/// receiver is a live `TAG_DYNOBJ` cell; `key` is a live Str cell;
/// `(tag, value)` is an entry payload under the same flags contract
/// as the front door (an accessor-pair CELL as `value` is fine —
/// it is entry data here, never invoked). Throwing flavor.
///
/// # Safety
/// Same as `__torajs_dynobj_define`, plus the receiver-shape
/// contract above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_define_plain(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
) {
    unsafe { define_plain_apply(obj_slot, key, tag, value, flags_byte, true) };
}

/// Boolean flavor of [`__torajs_dynobj_define_plain`] — same narrow
/// contract, refusal answers 0 with no pending throw (the
/// `__torajs_dynobj_define_soft` twin; torajs-arr's expando defines
/// carry both flavors through the same bag).
///
/// # Safety
/// Same as [`__torajs_dynobj_define_plain`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_define_plain_soft(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
) -> i64 {
    unsafe { define_plain_apply(obj_slot, key, tag, value, flags_byte, false) }
}

unsafe fn define_plain_apply(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
    throw_on_refusal: bool,
) -> i64 {
    unsafe {
        let obj = *obj_slot;
        if obj.is_null() {
            return 1;
        }
        debug_assert_eq!(
            (obj.cast::<u8>().add(4) as *const u16).read(),
            crate::layout::TAG_DYNOBJ,
            "define_plain contract: receiver must be a plain dynobj"
        );
        // Same monkey-patch note the front door records — a define
        // landing on a builtin `<Ctor>.prototype` singleton flips
        // the fast-arm pre-gate bit (no-op for every other dynobj).
        if let Some((data, len)) = key_str_bytes(key) {
            __torajs_builtin_proto_note_own_write(obj, data, len as i64);
        }
        if entries_len(obj) == crate::probe::entries_cap(obj) {
            crate::resize::resize(obj);
        }
        let pr = probe(obj, key as *const c_void);
        if pr.found {
            let ent = entries(obj);
            crate::define_redefine::redefine_entry(
                ent.add(pr.entry as usize),
                tag,
                value,
                flags_byte,
                throw_on_refusal,
            )
        } else {
            define_fresh_entry(obj, key, pr.slot, tag, value, flags_byte, throw_on_refusal)
        }
    }
}
