//! The static half of the class-object own-entry defines — the
//! entries that land on the CLASS OBJECT (`static` method / field /
//! accessor, and the §15.7.14 own-key reorder), split out of `define`
//! when the K2b accessor-face work pushed it past the 500-line hard
//! limit. Verbatim move. The parent keeps what lands on the
//! PROTOTYPE (`accessor` / computed method / computed accessor) and
//! the shared `define_accessor_pair` both halves mint through.

use core::ffi::c_void;

use super::*;

/// Knife B cut 2 — one static-method own entry on the class object.
/// ssa_lower hands the resolved triple (`tag`, the method-name Str
/// cell, the `__sm_<C>__<M>` boxed adapter's vaddr); the minted cell
/// is the same reified-method shape the prototype entries use, and
/// the define applies the §10.2.10 `{writable: true, enumerable:
/// false, configurable: true}` attribute set. A `name` / `length`
/// static method redefines the reflection slot the register lock
/// shaped (both are configurable, so the redefine is legal — and the
/// function-valued entry is the spec answer for a static method
/// shadow).
///
/// # Safety
/// `name_str` is a live Str cell (caller-owned; the define takes its
/// own key reference); `adapter` is a live boxed-adapter code
/// address.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_static_method_define(
    tag: i64,
    name_str: *const u8,
    adapter: u64,
    this_free: u64,
    twin: u64,
) {
    if !in_range(tag) || name_str.is_null() || adapter == 0 {
        return;
    }
    // SAFETY: single-threaded JS; the register sequence filled the
    // slot before any reify call (class_globals.rs emit order).
    let class_anyv = unsafe { CLASSES_BY_TAG_IMM[tag as usize] };
    if !is_cell_imm(class_anyv) {
        return;
    }
    if unsafe { heap_type_tag(class_anyv as *const c_void) } != TAG_DYNOBJ {
        return;
    }
    unsafe {
        // S2.38 — a static body's `this` resolves to the class object
        // at compile time (the `__sm_` adapter drops its env
        // argument); the compiler still gates the flag on a lossless
        // argument surface (all-`Any` params, no caller-side
        // defaults), so the emit side decides and this define just
        // carries the verdict.
        // RFC 20260804-fn-this-channel knife 3b — `twin` is the
        // `__smany_` receiver-polymorphic body's boxed adapter (0 =
        // this-free / mint residue). A STATIC face encodes as
        // `(tag 0, twin ≠ 0)` in the guard pair: the mono body has
        // no receiver channel at all, so a `.call/.apply` rebind
        // must ALWAYS take the twin — there is no receiver value
        // the mono path could honor (the instance guard's tag
        // compare has nothing to compare against).
        let cell =
            __torajs_class_method_cell_new(adapter, this_free, 0, twin, core::ptr::null_mut());
        let mut slot = class_anyv as *mut c_void;
        __torajs_dynobj_define_plain(
            &mut slot,
            name_str,
            ANY_HEAP as u64,
            cell as u64,
            DEFINE_CTOR_FLAGS,
        );
        // rotation 186 — a define may resize (fresh block + free
        // old); publish the moved cell so later table reads don't
        // dereference freed memory. Same on every sibling below.
        CLASSES_BY_TAG_IMM[tag as usize] = slot as u64;
    }
}

/// RFC L3b static-field-reflect (2026-07-22) — one static-FIELD own
/// entry on the class object. ssa_lower hands the resolved
/// `(tag, name-Str, value-tag, value)` quad: the value pair comes
/// from `box_to_tag_value` over the `__sf_<C>__<f>` global slot's
/// current value (the heap arm's rc_inc is the entry's stake — the
/// define takes it). Attribute set is the §7.3.6 data-field triple
/// `{writable: true, enumerable: true, configurable: true}`.
/// Reflection-only: the compile-time `C.<f>` fold keeps reading the
/// global slot (the static-method reify precedent's split).
///
/// # Safety
/// `name_str` is a live Str cell (caller-owned; the define takes its
/// own key reference); `vtag`/`vvalue` follow the dynobj_define pair
/// contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_static_field_define(
    tag: i64,
    name_str: *const u8,
    vtag: u64,
    vvalue: u64,
) {
    if !in_range(tag) || name_str.is_null() {
        return;
    }
    // SAFETY: single-threaded JS; the register sequence filled the
    // slot before any reify call (class_globals.rs emit order).
    let class_anyv = unsafe { CLASSES_BY_TAG_IMM[tag as usize] };
    if !is_cell_imm(class_anyv) {
        return;
    }
    if unsafe { heap_type_tag(class_anyv as *const c_void) } != TAG_DYNOBJ {
        return;
    }
    unsafe {
        let mut slot = class_anyv as *mut c_void;
        __torajs_dynobj_define_plain(&mut slot, name_str, vtag, vvalue, DEFINE_FIELD_FLAGS);
        CLASSES_BY_TAG_IMM[tag as usize] = slot as u64;
    }
}

/// 刀 3 static twin of [`__torajs_class_accessor_define`] — the
/// AccessorPair own entry lands on the CLASS object (`gOPD(C, "s")`;
/// §15.7.14 static accessors are class-object properties).
///
/// # Safety
/// `name_str` is NULL or a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_static_accessor_define(
    tag: i64,
    name_str: *const u8,
    get_adapter: u64,
    set_adapter: u64,
) {
    if !in_range(tag) || name_str.is_null() || (get_adapter == 0 && set_adapter == 0) {
        return;
    }
    let class_anyv = unsafe { CLASSES_BY_TAG_IMM[tag as usize] };
    if !is_cell_imm(class_anyv) {
        return;
    }
    if unsafe { heap_type_tag(class_anyv as *const c_void) } != TAG_DYNOBJ {
        return;
    }
    unsafe {
        let spelling = crate::str_wtf8::StrWtf8::of(name_str.cast());
        let mut slot = class_anyv as *mut c_void;
        define_accessor_pair(
            &mut slot,
            name_str,
            spelling.as_bytes(),
            FaceHalf::mono(get_adapter),
            FaceHalf::mono(set_adapter),
            DEFINE_ACCESSOR_FLAGS,
            0,
        );
        CLASSES_BY_TAG_IMM[tag as usize] = slot as u64;
    }
}

/// 563-03 — move the class object's own `<name>` entry behind
/// everything defined so far. §15.7.14 defines every static element
/// in ONE ordered pass, but a COMPUTED static member's key exists
/// only at the class-decl position, long after the prologue's
/// registration walk defined the plain members, and an own entry can
/// only be APPENDED. So the members declared after the computed one
/// are moved behind it — one emitted call each, ordered by the class
/// element list.
///
/// The instance side answers the same question from the class's
/// method table ([`super::reify::redefine_rows_after`]); a class
/// object has no such table, its members arrive one emitted reify at
/// a time, so the order is repaired from the emit side instead.
/// Nothing can observe the intermediate states: this runs inside the
/// class definition, before the binding exists.
///
/// # Safety
/// `name_str` is NULL or a live Str cell (caller-owned — the entry
/// keeps the key cell it already owns).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_static_own_move_to_end(tag: i64, name_str: *const u8) {
    if !in_range(tag) || name_str.is_null() {
        return;
    }
    let class_anyv = unsafe { CLASSES_BY_TAG_IMM[tag as usize] };
    if !is_cell_imm(class_anyv) {
        return;
    }
    unsafe {
        if heap_type_tag(class_anyv as *const c_void) != TAG_DYNOBJ {
            return;
        }
        __torajs_dynobj_move_own_to_end(class_anyv as *mut c_void, name_str.cast::<c_void>());
    }
}
