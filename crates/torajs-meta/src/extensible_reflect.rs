//! RFC C5b — `Object.preventExtensions` / `isExtensible` / `seal` /
//! `isSealed` reflection family, split out of [`crate::reflect`]
//! (file-size). The header-flag pair (`FLAG_NON_EXTENSIBLE` +
//! `FLAG_SEALED`) lives on the universal heap header via torajs-rc;
//! the per-entry configurable walk lives in the DynObj substrate.

use core::ffi::c_void;

use crate::reflect::{TAG_DYNOBJ, heap_type_tag, is_cell_imm};

unsafe extern "C" {
    // RFC C5b — raw-pointer layer FLAG_NON_EXTENSIBLE / FLAG_SEALED
    // setters and readers from torajs-rc/src/extensible.rs. Cell-only
    // — caller filters out primitive imms first.
    fn __torajs_obj_prevent_extensions(p: *mut c_void) -> *mut c_void;
    fn __torajs_obj_is_extensible(p: *const c_void) -> bool;
    fn __torajs_obj_seal_mark(p: *mut c_void) -> *mut c_void;
    fn __torajs_obj_is_sealed_marked(p: *const c_void) -> bool;
    // DynObj entry-table walk from torajs-dynobj/src/seal.rs. Clears
    // BUCKET_FLAG_CONFIGURABLE on every live entry / checks whether
    // every live entry is already non-configurable. NULL / non-DynObj
    // input is a no-op for the setter and `true` for the predicate
    // (the caller already gated on the FLAG_NON_EXTENSIBLE bit, so
    // non-DynObj cells fall through to the SEALED marker check).
    fn __torajs_dynobj_seal_entries(obj: *mut c_void);
    fn __torajs_dynobj_all_entries_non_configurable(obj: *const c_void) -> bool;
}

/// RFC C5b — `Object.preventExtensions(O)`. Spec ES §20.1.2.16 step 1
/// `If Type(O) is not Object, return O.` Real objects route through
/// the raw-pointer setter that flips [`torajs_rc::FLAG_NON_EXTENSIBLE`]
/// on the universal heap header. The same boxed AnyValue is returned
/// (identity-preserving — `Object.preventExtensions(o) === o` holds at
/// the JS level because the cell still points at the same heap block).
///
/// # Safety
///
/// `obj_any` must carry a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_prevent_extensions(obj_any: u64) -> u64 {
    if !is_cell_imm(obj_any) {
        // primitive imm / null / undef — spec returns the value as-is.
        return obj_any;
    }
    // SAFETY: cell pointer to a valid heap object per invariant.
    unsafe { __torajs_obj_prevent_extensions(obj_any as *mut c_void) };
    obj_any
}

/// RFC C5b — `Object.isExtensible(O)`. Spec ES §20.1.2.13 step 1
/// `If Type(O) is not Object, return false.` Primitives, null, and
/// undefined report `false`; real cells delegate to the raw-pointer
/// reader that inspects [`torajs_rc::FLAG_NON_EXTENSIBLE`].
///
/// # Safety
///
/// `obj_any` must carry a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_is_extensible(obj_any: u64) -> bool {
    if !is_cell_imm(obj_any) {
        return false;
    }
    // SAFETY: cell pointer to a valid heap object.
    unsafe { __torajs_obj_is_extensible(obj_any as *const c_void) }
}

/// RFC C5b — `Object.seal(O)`. Spec ES §20.1.2.20: real objects flip
/// `[[Extensible]] = false` AND every own property's
/// `[[Configurable]] = false`. tora splits the work: the header-flag
/// pair (`FLAG_NON_EXTENSIBLE` + `FLAG_SEALED`) is set via
/// [`__torajs_obj_seal_mark`]; the per-entry configurable walk lives
/// in the DynObj substrate. Non-DynObj cells (typed Tag::Obj struct /
/// Arr / Closure / …) carry only the header markers — their property
/// table is the static field layout and has no per-entry configurable
/// bit to flip; the SEALED bit alone faithfully encodes "user called
/// `Object.seal` on this typed cell".
///
/// # Safety
///
/// `obj_any` must carry a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_seal(obj_any: u64) -> u64 {
    if !is_cell_imm(obj_any) {
        return obj_any;
    }
    let p = obj_any as *mut c_void;
    // SAFETY: cell pointer to a valid heap object.
    unsafe { __torajs_obj_seal_mark(p) };
    // SAFETY: non-DynObj cells short-circuit inside the helper.
    unsafe { __torajs_dynobj_seal_entries(p) };
    obj_any
}

/// RFC C5b — `Object.isSealed(O)`. Spec ES §20.1.2.15: `true` iff the
/// object is not extensible AND every own property is non-configurable.
///
/// Primitive / null / undef report `true` (spec: non-Object inputs are
/// trivially sealed). For cells:
/// * extensible cell → `false` (extensible ⇒ not sealed)
/// * sealed-marker bit set → `true` (user called `Object.seal`)
/// * non-DynObj cell with prevent-only → `false` (typed Obj/Arr fields
///   are spec-configurable until `seal` is explicitly invoked — bun
///   parity for `Object.isSealed(class_instance_after_prevent)`)
/// * DynObj cell → walk entry table for all-non-configurable (vacuously
///   `true` on an empty dict — matches bun's
///   `Object.isSealed({})` after `preventExtensions`).
///
/// # Safety
///
/// `obj_any` must carry a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_is_sealed(obj_any: u64) -> bool {
    if !is_cell_imm(obj_any) {
        return true;
    }
    let p = obj_any as *const c_void;
    // SAFETY: cell pointer.
    if unsafe { __torajs_obj_is_extensible(p) } {
        return false;
    }
    // SAFETY: same.
    if unsafe { __torajs_obj_is_sealed_marked(p) } {
        return true;
    }
    // prevent-only path. DynObj walks the entry table; typed-Obj /
    // Arr / Closure / Map / Set / Date / Promise / RegExp / WeakRef /
    // WeakMap / WeakSet answer false (their own properties remained
    // configurable through preventExtensions per bun parity).
    // SAFETY: pointer to a valid heap object with universal header.
    let tag = unsafe { heap_type_tag(p) };
    if tag != TAG_DYNOBJ {
        return false;
    }
    // SAFETY: confirmed DynObj cell.
    unsafe { __torajs_dynobj_all_entries_non_configurable(p) }
}
