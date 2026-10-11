//! The sidecar VALUE WORD of a declared class field — what it can
//! hold beside the attribute-only filler: the deletion record (RFC
//! 20260915-declared-field-delete, moved here from `define_struct`
//! when that file reached the cap) and the shadow value (RFC
//! 20260915-declared-field-shadow).
//!
//! A layout slot is typed (Any / Str / I64 / F64 / Bool) and the
//! typed store rules refuse a payload that does not fit it; the
//! spec's field is a plain data property that takes any value. The
//! value the any lane wrote lives in the field's attribute sidecar
//! (RFC 20260806: the same-named `+24` entry whose value word was
//! `ANY_UNDEF` filler) — the third thing that word can hold, beside
//! the filler and the HOLE of a deleted field (RFC 20260915). Every
//! own face that reads the slot asks for the shadow first, behind
//! the exotic-field header bit an untouched instance never sets.
//!
//! `undefined` as a shadow is spelled `Heap(__TORAJS_UNDEF_CELL)` —
//! the immortal cell a pointer slot's `undefined` already uses — so
//! it stays distinct from the filler. The callers spell it; this
//! module only stores and reports the word.

use core::ffi::c_void;

use crate::accessor::VALUE_UNDEFINED;
use crate::define::define_apply;
use crate::layout::{
    DEFINE_FLAG_CONFIGURABLE, DEFINE_FLAG_ENUMERABLE, DEFINE_FLAG_WRITABLE, DEFINE_PRESENT_VALUE,
    DYNOBJ_HOLE_SENTINEL,
};
use crate::probe::{entries, probe};

/// `torajs_rc::FLAG_OBJ_EXOTIC_FIELD` mirror (see `define_struct`).
const OBJ_HDR_FLAG_EXOTIC_FIELD: u16 = 1 << 15;

unsafe extern "C" {
    fn __torajs_obj_props_attach(cell: *mut u8, props: *mut c_void);
    fn __torajs_obj_flag_exotic_field(obj: *mut c_void);
    fn __torajs_dynobj_set_entry_value_raw(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
    );
    /// torajs-dispatch seam — an absent-member set turned into deletion
    /// records (torajs-meta `struct_absent`).
    fn __torajs_obj_absent_settle(cell: *mut c_void);
    /// torajs-meta — clear `key`'s bit in the absent set; 1 if it was set.
    fn __torajs_obj_absent_clear(cell: *mut c_void, key: *const c_void) -> i64;
}

/// `torajs_rc::FLAG_OBJ_ABSENT_FIELD` mirror (Tag::Obj-private bit 11).
const OBJ_HDR_FLAG_ABSENT_FIELD: u16 = 1 << 11;

/// Turn the absent-member set an object literal left on the struct cell
/// `obj` into deletion records before a define moves its members
/// (torajs-meta `struct_absent`): one header test when there is none.
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer.
#[inline]
pub(crate) unsafe fn settle_absent(obj: *mut c_void) {
    let hdr = unsafe { (obj.cast::<u8>().add(6) as *const u16).read() };
    if hdr & OBJ_HDR_FLAG_ABSENT_FIELD != 0 {
        unsafe { __torajs_obj_absent_settle(obj) };
    }
}

/// The sidecar's value word for `key`, or `None` when the instance
/// carries no sidecar for it. Free on an untouched instance.
unsafe fn sidecar_word(obj: *const c_void, key: *const c_void) -> Option<u64> {
    let hdr = unsafe { (obj.cast::<u8>().add(6) as *const u16).read() };
    if hdr & OBJ_HDR_FLAG_EXOTIC_FIELD == 0 {
        return None;
    }
    let props = unsafe { *(obj.cast::<u8>().add(crate::layout::CELL_PROPS_OFF) as *const u64) }
        as *const c_void;
    if props.is_null() {
        return None;
    }
    let pr = unsafe { probe(props, key) };
    if !pr.found {
        return None;
    }
    Some(unsafe { (*entries(props).add(pr.entry as usize)).value_anyv })
}

/// RFC 20260915-declared-field-delete — is the DECLARED field `key` on
/// `obj` deleted? A deletion is a sidecar entry whose value word holds
/// the HOLE sentinel (`set_entry_flags`, the same record `delete
/// arr[i]` leaves on a shadow index); the layout slot keeps its stale
/// bits and every own face asks this before reading it. Free for an
/// instance nothing ever redefined or deleted: the exotic-field header
/// bit gates the probe.
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer; `key` is a live key cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_declared_field_deleted(
    obj: *mut c_void,
    key: *mut c_void,
) -> i64 {
    unsafe { settle_absent(obj) };
    let hdr = unsafe { (obj.cast::<u8>().add(6) as *const u16).read() };
    if hdr & OBJ_HDR_FLAG_EXOTIC_FIELD == 0 {
        return 0;
    }
    let props = unsafe { *(obj.cast::<u8>().add(crate::layout::CELL_PROPS_OFF) as *const u64) }
        as *const c_void;
    if props.is_null() {
        return 0;
    }
    i64::from(unsafe { crate::set_entry_flags::__torajs_dynobj_entry_is_hole(props, key) } != 0)
}

/// §10.1.10 [[Delete]] of a DECLARED field that passed the configurable
/// gate (`prop_delete::struct_delete`): record the deletion as a HOLE
/// sidecar — minting the bag when the instance has none — and raise
/// the exotic-field bit the own faces gate on. The slot's bits stay;
/// nothing reads them past the sidecar. The hole clears the entry's
/// W/E/C bits, so a repeat `delete` must ask "deleted?" before the
/// configurable gate (it does). Answers 1 (spec success).
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer whose layout declares `key`;
/// `key` is a live key cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_declared_field_delete(
    obj: *mut c_void,
    key: *mut c_void,
) -> i64 {
    let props_slot =
        unsafe { obj.cast::<u8>().add(crate::layout::CELL_PROPS_OFF) } as *mut *mut c_void;
    unsafe {
        if (*props_slot).is_null() {
            __torajs_obj_props_attach(obj.cast::<u8>(), crate::alloc::__torajs_dynobj_alloc());
        }
        crate::set_entry_flags::__torajs_dynobj_set_entry_hole(props_slot, key);
        __torajs_obj_flag_exotic_field(obj);
    }
    1
}

/// A store that creates a deleted declared field again (§10.1.9.2 step
/// 2.c.i; the caller has run the extensible gate): the deletion record
/// is dropped, which brings back the layout's default attributes, and
/// the store that follows fills the slot. When no declared name keeps
/// an entry in the dict the exotic-field bit comes down, so the cell's
/// typed stores are one store again.
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer carrying a deletion record
/// for `key`; `key` is a live key cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_declared_field_revive(obj: *mut c_void, key: *mut c_void) {
    // a member the literal that built the cell left out has no record
    if unsafe { __torajs_obj_absent_clear(obj, key) } != 0 {
        return;
    }
    let props = unsafe { *(obj.cast::<u8>().add(crate::layout::CELL_PROPS_OFF) as *const u64) }
        as *mut c_void;
    unsafe { crate::delete::__torajs_dynobj_delete(props, key) };
    let n = unsafe { crate::iter::__torajs_dynobj_iter_len(props) };
    let declared = (0..n).any(|i| {
        let k = unsafe { crate::iter::__torajs_dynobj_iter_key(props, i) };
        !k.is_null()
            && unsafe { crate::define_struct::__torajs_obj_declared_field_attrs(obj, k) } >= 0
    });
    if !declared {
        let flags = unsafe { obj.cast::<u8>().add(6).cast::<u16>() };
        unsafe { *flags &= !OBJ_HDR_FLAG_EXOTIC_FIELD };
    }
}

/// [[Set]] of a payload the layout slot cannot hold: the value goes
/// to the sidecar — minted with default attributes when the field has
/// none, its value word replaced when it has (the attributes stay).
/// `(tag, value)` transfers in. The caller has already run the
/// writable / frozen gate. Answers 1.
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer whose layout declares
/// `key`; `key` a live key cell; `(tag, value)` the caller's owned
/// payload, never a bare `undefined` (spell it as the undef cell).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_declared_field_shadow_set(
    obj: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
) -> i64 {
    let props_slot =
        unsafe { obj.cast::<u8>().add(crate::layout::CELL_PROPS_OFF) } as *mut *mut c_void;
    unsafe {
        if (*props_slot).is_null() {
            __torajs_obj_props_attach(obj.cast::<u8>(), crate::alloc::__torajs_dynobj_alloc());
        }
        if probe(*props_slot, key).found {
            __torajs_dynobj_set_entry_value_raw(props_slot, key, tag, value);
        } else {
            define_apply(
                props_slot,
                key,
                tag,
                value,
                DEFINE_FLAG_WRITABLE
                    | DEFINE_FLAG_ENUMERABLE
                    | DEFINE_FLAG_CONFIGURABLE
                    | DEFINE_PRESENT_VALUE,
                false,
            );
        }
        __torajs_obj_flag_exotic_field(obj);
    }
    1
}

/// A store the slot DID take supersedes any shadow: the sidecar's
/// value word goes back to the filler (its stake released), the
/// attributes stay. Free when there is no shadow.
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer; `key` a live key cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_declared_field_shadow_clear(
    obj: *mut c_void,
    key: *mut c_void,
) {
    let Some(w) = (unsafe { sidecar_word(obj, key) }) else {
        return;
    };
    if w == VALUE_UNDEFINED || w == DYNOBJ_HOLE_SENTINEL {
        return;
    }
    let props_slot =
        unsafe { obj.cast::<u8>().add(crate::layout::CELL_PROPS_OFF) } as *mut *mut c_void;
    // tag 5 = ANY_UNDEF: the box_from_pair spelling of the filler.
    unsafe { __torajs_dynobj_set_entry_value_raw(props_slot, key, 5, 0) };
}
