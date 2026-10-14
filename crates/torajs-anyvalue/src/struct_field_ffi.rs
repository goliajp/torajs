//! Cross-crate shells over the class-layout DATA-field store and
//! compare — RFC 20260806-declared-field-redefine.
//!
//! `Object.defineProperty` on a class instance is implemented in
//! torajs-dynobj (it owns the `+24` expando dict where the redefined
//! field's ATTRIBUTES live), but the field's VALUE lives in the layout
//! slot, whose typed store rules — slot-type match, frozen gate, rc
//! transfer — are already settled here by the `a.x = v` path. Rather
//! than restate those rules across the crate line, the define arm
//! calls back through these two symbols.
//!
//! Both are thin: no logic beyond the raw↔bool boundary conversion.
//! [`exotic`] holds the other direction: the typed lane's own field
//! access falling back to the any lane.

use core::ffi::c_void;

pub(crate) mod exotic;

unsafe extern "C" {
    /// torajs-rc — the cell's FROZEN header bit.
    fn __torajs_obj_is_frozen(p: *const c_void) -> bool;
}

/// Write `[[Value]]` into the layout slot the DECLARED field `key`
/// names. Answers 1 when the store landed — or when the frozen gate
/// refused it LOUDLY, having recorded its own TypeError, since either
/// way the caller must not go on to write attributes. Answers 0 when
/// `key` names no data field of `ptr`'s layout, or when the payload
/// does not fit the slot's type (a typed slot never silently
/// coerces).
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` is a live `Str`
/// cell. The caller transfers one rc of a `Heap`-tagged `value`,
/// consumed by the store.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_data_field_set(
    ptr: *mut c_void,
    key: *const c_void,
    tag: u64,
    value: u64,
) -> i64 {
    unsafe {
        if frozen_data_field_refuses(ptr, key, tag, value, true) {
            return 1;
        }
        i64::from(crate::struct_field_set::struct_data_field_set(
            ptr, key, tag, value,
        ))
    }
}

/// ES §7.3.14 — a frozen struct's data fields are all non-writable,
/// so a [[Set]] on one is refused; the refusal is flavored here
/// (§13.15.2 strict records the TypeError, sloppy assignment and
/// `Reflect.set` answer false). Checked only when `key` resolves to a
/// layout DATA field: a miss is the caller's (different) reject, and
/// an accessor runs its setter whatever the integrity level. Answers
/// true when it refused (the payload is dropped).
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` a live `Str` cell;
/// the payload follows the lowering's consume convention.
pub(crate) unsafe fn frozen_data_field_refuses(
    ptr: *mut c_void,
    key: *const c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> bool {
    unsafe {
        if crate::struct_probe::struct_field_pair(ptr, key).is_none()
            || !__torajs_obj_is_frozen(ptr)
        {
            return false;
        }
        crate::member_set::drop_payload(tag, value);
        if throw_on_refusal {
            crate::member_set::__torajs_throw_type_error(
                c"Attempted to assign to readonly property.".as_ptr(),
            );
        }
        true
    }
}

/// Does the DECLARED field `key` already hold `(tag, value)`?
///
/// §10.1.6.3 step 4 lets a redefine through on a non-writable,
/// non-configurable property when the incoming `[[Value]]` is
/// SameValue with the current one — so the define arm has to be able
/// to ask, and only this crate can decode a typed slot into a
/// comparable pair. Both pairs are compared as the values they spell
/// (§7.2.10 SameValue): a number field held as a double still matches
/// an incoming integer `5`, and two equal strings in different cells
/// match.
///
/// Answers 0 for a missing layout / absent field, which is the honest
/// reading: nothing is stored there to match.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` is a live `Str`
/// cell. Both operands are BORROWED — no stake is taken or released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_data_field_same_value(
    ptr: *mut c_void,
    key: *const c_void,
    tag: u64,
    value: u64,
) -> i64 {
    match unsafe { crate::struct_probe::struct_field_pair(ptr, key) } {
        Some((cur_tag, cur_value)) => unsafe {
            use crate::nanbox_encode::__torajs_anyv_box_from_pair as boxed;
            let (cur, new) = (
                boxed(cur_tag as i64, cur_value as i64),
                boxed(tag as i64, value as i64),
            );
            i64::from(crate::nanbox_ffi::__torajs_anyv_same_value(cur, new))
        },
        None => 0,
    }
}
