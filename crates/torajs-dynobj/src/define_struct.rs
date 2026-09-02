//! The class-instance receiver arm of [`crate::define::define_apply`]
//! — the expando half (RFC 20260714-struct-dynamic-props) and the
//! DECLARED-field half (RFC 20260806-declared-field-redefine).
//!
//! ## Why a declared field can be redefined at all
//!
//! It used to be refused, on the grounds that accepting it would put a
//! second own property of the same name into the `+24` dict while
//! every reader consults the layout first. The danger was real — the
//! enumeration merge concatenated layout names with expando keys
//! without deduplicating — but it only follows if the dict entry has
//! to carry the VALUE. It does not.
//!
//! **One property, two storage sites, each holding the half it is good
//! at.** The value stays in the layout slot: the only place it has ever
//! lived, and the only place the compiled `c.x` load will ever look —
//! so redefining costs a typed field read exactly nothing, and the
//! value cannot desynchronize because it is never duplicated. The
//! ATTRIBUTES move into a same-named dict entry whose bucket word
//! already carries the three bits a layout row cannot express; that
//! entry's `value_anyv` is `ANY_UNDEF` filler and is never read.
//!
//! Call it an **attribute sidecar**. Every enumeration face skips a
//! dict key the layout declares — which is the deduplication those
//! faces needed regardless, and is unambiguous because the ordinary
//! `[[Set]]` expando path refuses to create a dict entry for a name
//! the layout carries.

use core::ffi::c_void;

use crate::define::{define_apply, refuse};
use crate::define_struct_attrs::{merge_field_attrs, sidecar_flags_byte, validate_field_redefine};
use crate::layout::{
    BUCKET_FLAG_CONFIGURABLE, BUCKET_FLAG_ENUMERABLE, BUCKET_FLAG_WRITABLE, DEFINE_PRESENT_VALUE,
};

/// torajs-core `ssa_lower::OBJ_CLASS_TAG_OFF` mirror — the u32 class
/// tag right after the universal heap header of a `Tag::Obj` cell.
/// Named so a sweep for class-tag read points finds this one.
const OBJ_CLASS_TAG_OFF: usize = 8;
use crate::probe::{bucket_flags, entries, key_str_bytes, probe};

/// `torajs_rc::FLAG_NON_EXTENSIBLE` mirror — the universal heap
/// header's bit 8, which a `Tag::Obj` cell carries and its expando
/// dict does not.
const OBJ_HDR_FLAG_NON_EXTENSIBLE: u16 = 1 << 8;

/// `torajs_rc::FLAG_ERROR` mirror — bit 7, [[ErrorData]].
const OBJ_HDR_FLAG_ERROR: u16 = 1 << 7;

/// `torajs_rc::FLAG_OBJ_EXOTIC_FIELD` mirror — bit 15, "this instance
/// carries at least one declared field with non-default attributes".
/// The typed store's gate reads it; see that constant's doc.
const OBJ_HDR_FLAG_EXOTIC_FIELD: u16 = 1 << 15;

/// `torajs_rc::FLAG_OBJ_EXPANDO` mirror — bit 12, "this instance owns
/// at least one key its layout never mentions". A separate bit from
/// the one above on purpose: the typed store asks only the first
/// question, and would otherwise probe a dict for a sidecar that a
/// mere dynamic property never put there.
const OBJ_HDR_FLAG_EXPANDO: u16 = 1 << 12;

unsafe extern "C" {
    /// torajs-rc — a struct cell's first expando attach (r502): the
    /// instance's drop seams are guarded on this entry's liveness.
    fn __torajs_obj_props_attach(cell: *mut u8, props: *mut c_void);
    // torajs-structmeta (W-J Phase A4) — the read side over the
    // link-emitted `__torajs_class_layouts` table.
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    fn __torajs_struct_accessor_find(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
        kind: u8,
    ) -> u32;
    // torajs-anyvalue `struct_field_ffi` — the typed-slot store and
    // compare, borrowed rather than restated across the crate line.
    fn __torajs_struct_data_field_set(
        ptr: *mut c_void,
        key: *const c_void,
        tag: u64,
        value: u64,
    ) -> i64;
    fn __torajs_struct_data_field_same_value(
        ptr: *mut c_void,
        key: *const c_void,
        tag: u64,
        value: u64,
    ) -> i64;
    // torajs-rc integrity levels.
    fn __torajs_obj_is_frozen(p: *const c_void) -> bool;
    fn __torajs_obj_is_sealed_marked(p: *const c_void) -> bool;
}

/// What a class instance declares under `key`, if anything.
pub(crate) enum Declared {
    /// Nothing — the name is free for an ordinary expando.
    No,
    /// A DATA field: a layout slot holding the value.
    Field,
    /// An accessor member, carried under synthetic `__getter_v` /
    /// `__setter_v` slots. Its attributes take the same sidecar a data
    /// field's do; replacing its faces does not.
    Accessor,
}

/// Classify `key` against `obj`'s toolchain-emitted layout metadata —
/// the same table every struct reflection surface consults.
///
/// A NULL layout row (anonymous struct interned too late) declares
/// nothing, which is the honest answer: such a cell carries only
/// expandos.
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer; `key` is a live key cell.
pub(crate) unsafe fn struct_declares(obj: *const c_void, key: *const c_void) -> Declared {
    if unsafe { key_str_bytes(key) }.is_none() {
        // A symbol key names no declared field by construction.
        return Declared::No;
    }
    let k = unsafe { crate::key_wtf8::KeyWtf8::of(key) };
    let (data, len) = (k.as_ptr(), k.len());
    let class_tag = unsafe { (obj.cast::<u8>().add(OBJ_CLASS_TAG_OFF) as *const u32).read() };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return Declared::No;
    }
    unsafe {
        if __torajs_struct_field_find(layout, data, len) != u32::MAX {
            return Declared::Field;
        }
        if __torajs_struct_accessor_find(layout, data, len, 0) != u32::MAX
            || __torajs_struct_accessor_find(layout, data, len, 1) != u32::MAX
        {
            return Declared::Accessor;
        }
    }
    Declared::No
}

/// The whole class-instance receiver arm, as one answer for
/// [`define_apply`]'s dispatcher: `Some(result)` when this module
/// handled the define, `None` for a request it does not carry (see
/// the accessor arm), which falls back to the dispatcher's loud
/// no-op.
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer; `key` is a live key cell.
/// Same `[[Value]]` stake contract as [`define_apply`] — a `None`
/// answer consumes nothing, leaving the stake with the caller.
pub(crate) unsafe fn struct_receiver_define(
    obj: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
    throw_on_refusal: bool,
) -> Option<i64> {
    match unsafe { struct_declares(obj, key) } {
        Declared::No => {
            Some(unsafe { struct_define(obj, key, tag, value, flags_byte, throw_on_refusal) })
        }
        Declared::Field => Some(unsafe {
            redefine_declared_field(obj, key, tag, value, flags_byte, throw_on_refusal)
        }),
        // An accessor member takes the same sidecar for the same
        // reason, as long as the descriptor only moves attributes.
        // `enumerable` and `configurable` are real questions about an
        // accessor and a layout row can express neither, while the
        // getter and setter stay exactly where they were — so the
        // split that makes the data half work ("the layout keeps what
        // it is good at, the dict keeps the rest") needs no second
        // spelling here.
        //
        // A descriptor carrying a `[[Value]]` is a different request:
        // a data value, or the AccessorPair the get / set half builds,
        // asks to change what the property IS. The faces live in
        // synthetic layout slots that the compiled read dispatches
        // through, so replacing them is another mechanism; that one
        // keeps the dispatcher's loud no-op.
        Declared::Accessor if flags_byte & DEFINE_PRESENT_VALUE == 0 => Some(unsafe {
            redefine_declared_field(obj, key, tag, value, flags_byte, throw_on_refusal)
        }),
        Declared::Accessor => None,
    }
}

/// The attributes a DECLARED field currently has: its sidecar when one
/// exists, else the layout default.
///
/// The default is not a constant — the integrity level moves it
/// (`frozen` ⇒ non-writable and non-configurable, `sealed` ⇒
/// non-configurable), and the Error header lines `message` / `stack`
/// are the one pair of fields whose `enumerable` is false. Both
/// mirror what `struct_cell_descriptor` reports on the read side; a
/// define that disagreed with the descriptor would let an absent
/// attribute silently flip.
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer; `key` is a live key cell.
unsafe fn current_field_attrs(obj: *mut c_void, key: *mut c_void) -> u64 {
    let props = unsafe { *(obj.cast::<u8>().add(crate::layout::CELL_PROPS_OFF) as *const u64) }
        as *mut c_void;
    if !props.is_null() {
        let pr = unsafe { probe(props, key as *const c_void) };
        if pr.found {
            let ent = unsafe { entries(props).add(pr.entry as usize) };
            return bucket_flags(unsafe { (*ent).key_ptr_tagged });
        }
    }
    let frozen = unsafe { __torajs_obj_is_frozen(obj) };
    let sealed = frozen || unsafe { __torajs_obj_is_sealed_marked(obj) };
    let hdr_flags = unsafe { (obj.cast::<u8>().add(6) as *const u16).read() };
    let nonenum = hdr_flags & OBJ_HDR_FLAG_ERROR != 0
        && match unsafe { key_str_bytes(key) } {
            Some((data, len, true)) => {
                let name = unsafe { core::slice::from_raw_parts(data, len as usize) };
                name == b"message" || name == b"stack"
            }
            _ => false,
        };
    let mut flags = 0;
    if !frozen {
        flags |= BUCKET_FLAG_WRITABLE;
    }
    if !sealed {
        flags |= BUCKET_FLAG_CONFIGURABLE;
    }
    if !nonenum {
        flags |= BUCKET_FLAG_ENUMERABLE;
    }
    flags
}

/// The live attribute set of the DECLARED member `key` on `obj` — its
/// sidecar when one exists, else the layout default — as the three
/// `BUCKET_FLAG_*` bits, or `-1` when the layout declares nothing
/// under `key`.
///
/// The `-1` answer is what a caller that has not yet classified the
/// key needs: `delete` asks this one question and reads both halves
/// of it, routing a `-1` to the expando dict instead.
///
/// One judgment, one implementation. The default is not a constant —
/// frozen ⇒ non-writable, sealed ⇒ non-configurable, an Error's
/// `message` / `stack` ⇒ non-enumerable — and a second site restating
/// those three inputs is how the read and write sides come to
/// disagree about an attribute nobody set.
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer; `key` is a live key cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_declared_field_attrs(
    obj: *mut c_void,
    key: *mut c_void,
) -> i64 {
    match unsafe { struct_declares(obj, key) } {
        Declared::No => -1,
        Declared::Field | Declared::Accessor => (unsafe { current_field_attrs(obj, key) }) as i64,
    }
}

/// `Object.defineProperty(instance, declaredField, desc)` — the arm
/// this module's doc describes.
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer whose layout declares `key`
/// as a DATA field; `key` is a live key cell. Same `[[Value]]` stake
/// contract as [`define_apply`].
pub(crate) unsafe fn redefine_declared_field(
    obj: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
    throw_on_refusal: bool,
) -> i64 {
    let cur = unsafe { current_field_attrs(obj, key) };
    let value_same = flags_byte & DEFINE_PRESENT_VALUE != 0
        && unsafe { __torajs_struct_data_field_same_value(obj, key, tag, value) } != 0;
    if !validate_field_redefine(cur, flags_byte, value_same) {
        return unsafe {
            refuse(
                throw_on_refusal,
                c"Attempting to change value of a readonly property.".as_ptr() as *const u8,
                flags_byte & DEFINE_PRESENT_VALUE != 0,
                tag,
                value,
            )
        };
    }
    // The value goes to the layout slot. A store the typed rules
    // refuse (slot-type mismatch) leaves the stake with us; SameValue
    // needs no store at all, and asking for one would pointlessly
    // re-run the frozen gate.
    if flags_byte & DEFINE_PRESENT_VALUE != 0
        && !value_same
        && unsafe { __torajs_struct_data_field_set(obj, key, tag, value) } == 0
    {
        return unsafe {
            refuse(
                throw_on_refusal,
                c"Attempting to define property on object that is not extensible.".as_ptr()
                    as *const u8,
                true,
                tag,
                value,
            )
        };
    }
    if value_same {
        // The store was skipped, so nothing consumed the caller's
        // stake in the (identical) payload.
        unsafe { crate::define::drop_rejected_value(tag, value) };
    }
    // The attributes go to the sidecar. Its `value_anyv` is filler:
    // no `[[Value]]` is passed, so the entry takes `ANY_UNDEF`.
    let merged = merge_field_attrs(cur, flags_byte);
    let props_slot =
        unsafe { obj.cast::<u8>().add(crate::layout::CELL_PROPS_OFF) } as *mut *mut c_void;
    let ok = unsafe {
        if (*props_slot).is_null() {
            __torajs_obj_props_attach(obj.cast::<u8>(), crate::alloc::__torajs_dynobj_alloc());
        }
        define_apply(
            props_slot,
            key,
            0,
            0,
            sidecar_flags_byte(merged),
            throw_on_refusal,
        )
    };
    // Raise the header bit the typed `o.field = v` store gates on, so
    // that a field demoted to non-writable actually refuses the store
    // rather than silently taking it. The bit is deliberately coarse
    // ("some field of this instance is exotic"): the store site has no
    // reason to pay for a finer question, and the instances that trip
    // it are exactly the ones that were redefined.
    if ok != 0 {
        unsafe { __torajs_obj_flag_exotic_field(obj) };
    }
    ok
}

/// Raise `FLAG_OBJ_EXOTIC_FIELD` on a struct cell — the one writer of
/// that bit, and so (r503) a link-guard anchor: the typed field-write
/// guard is stubbed when this atom and `__torajs_obj_freeze` are both
/// dead. `inline(never)` keeps the atom whole under its in-crate
/// caller.
///
/// # Safety
/// `obj` is a live `Tag::Obj` cell.
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_flag_exotic_field(obj: *mut c_void) {
    unsafe {
        let flags = obj.cast::<u8>().add(6) as *mut u16;
        *flags |= OBJ_HDR_FLAG_EXOTIC_FIELD;
    }
}

/// The EXPANDO half of the class-instance arm — a key the layout does
/// not declare.
///
/// Same recursion as the Closure receiver: the instance's own
/// properties live in the lazy expando dynobj at `+24`, so the full
/// §10.1.6.3 validate/apply runs against that entry table. What the
/// Closure arm does not need is the extensibility read below.
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer whose key is known not to
/// name a declared field; `key` is a live key cell. Same `[[Value]]`
/// stake contract as [`define_apply`].
pub(crate) unsafe fn struct_define(
    obj: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
    throw_on_refusal: bool,
) -> i64 {
    let props_slot =
        unsafe { obj.cast::<u8>().add(crate::layout::CELL_PROPS_OFF) } as *mut *mut c_void;
    unsafe {
        // §10.1.6.3 step 2 — a non-extensible object refuses a key it
        // does not already carry. The flag sits on the STRUCT header,
        // not on the dict the recursion targets, so it has to be read
        // before descending.
        if (obj.cast::<u8>().add(6) as *const u16).read() & OBJ_HDR_FLAG_NON_EXTENSIBLE != 0
            && ((*props_slot).is_null() || !probe(*props_slot, key as *const c_void).found)
        {
            return refuse(
                throw_on_refusal,
                c"Attempting to define property on object that is not extensible.".as_ptr()
                    as *const u8,
                flags_byte & DEFINE_PRESENT_VALUE != 0,
                tag,
                value,
            );
        }
        if (*props_slot).is_null() {
            __torajs_obj_props_attach(obj.cast::<u8>(), crate::alloc::__torajs_dynobj_alloc());
        }
        let ok = define_apply(props_slot, key, tag, value, flags_byte, throw_on_refusal);
        // An own key the layout never mentions moves this instance's
        // property set away from what the layout says — which every
        // reader that unfolds a compile-time member list needs to know.
        if ok != 0 {
            mark_expando(obj);
        }
        ok
    }
}

/// Raise the header bit that tells a compile-time unfold its member
/// list is no longer the whole story.
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer.
unsafe fn mark_expando(obj: *mut c_void) {
    unsafe {
        let flags = obj.cast::<u8>().add(6) as *mut u16;
        *flags |= OBJ_HDR_FLAG_EXPANDO;
    }
}
