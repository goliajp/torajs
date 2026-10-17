//! The READ side of the attribute sidecar — RFC
//! 20260806-declared-field-redefine.
//!
//! `Object.defineProperty` on a DECLARED class field leaves the value
//! in the layout slot and puts only the ATTRIBUTES into a same-named
//! entry in the `+24` expando dict (the write side lives in
//! torajs-dynobj `define_struct`). Two questions follow, and every
//! reflection face over a struct cell needs one or both:
//!
//! - **Does the layout declare this name?** An expando walk must skip
//!   the ones it does, or the sidecar would be listed as a second own
//!   property beside the field it describes. The answer is unambiguous
//!   because the ordinary `[[Set]]` expando path refuses to create a
//!   dict entry for a name the layout carries — a declared name in the
//!   dict can only be a sidecar.
//! - **What are this field's attributes?** The sidecar when one
//!   exists, else the caller's layout default.

use core::ffi::c_void;

use crate::obj_own_keys_struct::struct_props;

mod shadow;
pub(crate) use shadow::slot_or_shadow;
pub use shadow::{
    __torajs_obj_accessor_state, __torajs_obj_field_shadow, ACCESSOR_DELETED, ACCESSOR_LIVE,
    ACCESSOR_REPLACED,
};

unsafe extern "C" {
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    fn __torajs_struct_accessor_find(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
        kind: u8,
    ) -> u32;
    // Spelled with the `*const u8` key the rest of this crate declares
    // (arr_reflect / reflect) — a second, differently typed
    // declaration of the same symbol is a hard error here.
    fn __torajs_dynobj_has(obj: *const c_void, key: *const u8) -> bool;
    fn __torajs_dynobj_get_flags(obj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
    /// torajs-dynobj — the live attributes of a declared member: its
    /// sidecar when one exists, else the layout default the integrity
    /// level moves. -1 when the layout declares nothing under the key.
    fn __torajs_obj_declared_field_attrs(obj: *mut c_void, key: *mut c_void) -> i64;
    /// torajs-dynobj — the raw value word of entry `i`.
    fn __torajs_dynobj_iter_value(obj: *const c_void, i: u64) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// torajs-rc — is `p` the immortal undefined cell?
    fn __torajs_is_undef_cell(p: *const u8) -> i64;
}

/// `torajs_dynobj::layout::DYNOBJ_HOLE_SENTINEL` mirror — the raw value
/// word of a sidecar entry that records a deleted declared field
/// (RFC 20260915-declared-field-delete).
const DYNOBJ_HOLE_SENTINEL: u64 = 0x1A;

/// The attribute set a declared member currently reports, as the three
/// `BUCKET_FLAG_*` bits.
///
/// The default is not a constant — frozen ⇒ non-writable, sealed ⇒
/// non-configurable, an Error's `message` / `stack` ⇒ non-enumerable —
/// and the sidecar outranks it. The write side has to compute exactly
/// this to merge a descriptor over it, so the read side asks rather
/// than restating: two spellings of one default is how a `defineProperty`
/// and the `getOwnPropertyDescriptor` after it come to disagree.
///
/// # Safety
/// `cell` is a live `Tag::Obj` heap pointer whose layout declares
/// `key`; `key` is a live key cell.
pub(crate) unsafe fn declared_member_attrs(cell: *const c_void, key: *const c_void) -> u64 {
    unsafe { __torajs_obj_declared_field_attrs(cell as *mut c_void, key as *mut c_void) as u64 }
}

/// Is the DECLARED field named by `name` currently non-enumerable —
/// i.e. does it carry a sidecar whose `enumerable` bit is clear?
///
/// The bytes spelling exists because the enumeration walks hold a
/// layout name (`StrSlice` into the emitted metadata), not a `Str`
/// cell, and minting one per field per walk to ask a question that is
/// almost always "no" would be backwards. The header bit answers it
/// for free on every instance that was never redefined; only past
/// that gate is the dict scanned.
///
/// # Safety
/// `cell` is a live `Tag::Obj` heap pointer; `name` points at
/// `name_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_field_is_nonenumerable(
    cell: *const c_void,
    name: *const u8,
    name_len: u32,
) -> i64 {
    let want = unsafe { core::slice::from_raw_parts(name, name_len as usize) };
    // a member a literal left out reads like its deletion record
    if unsafe { crate::struct_absent::lacks(cell, want) } {
        return 1;
    }
    let flags = unsafe { cell.cast::<u8>().add(6).cast::<u16>().read() };
    if flags & OBJ_HDR_FLAG_EXOTIC_FIELD == 0 {
        return 0;
    }
    let props = unsafe { struct_props(cell) };
    if props.is_null() {
        return 0;
    }
    let n = unsafe { __torajs_dynobj_iter_len(props) };
    for i in 0..n {
        let key = unsafe { __torajs_dynobj_iter_key(props, i) };
        if key.is_null() {
            continue;
        }
        if unsafe { crate::str_wtf8::StrWtf8::of(key) }.as_bytes() == want {
            return i64::from(
                unsafe { __torajs_dynobj_iter_flags(props, i) } & BUCKET_FLAG_ENUMERABLE == 0,
            );
        }
    }
    0
}

/// Does the layout of `cell` declare the key of dict entry `pi` — i.e.
/// is that entry a sidecar (attributes / shadow / deletion record)
/// rather than an own expando? Enumeration faces skip it; the layout
/// row is the property.
///
/// # Safety
/// `cell` is a live `Tag::Obj` heap pointer; `props` its live expando
/// dict; `pi` a dense-entry index into it.
pub(crate) unsafe fn expando_key_declared(
    cell: *const c_void,
    props: *const c_void,
    pi: u64,
) -> bool {
    // Only an instance something redefined / shadowed / deleted has a
    // declared name in its dict (the [[Set]] expando path refuses to
    // create one), and only it carries the header bit.
    let flags = unsafe { cell.cast::<u8>().add(6).cast::<u16>().read() };
    if flags & OBJ_HDR_FLAG_EXOTIC_FIELD == 0 {
        return false;
    }
    let key = unsafe { __torajs_dynobj_iter_key(props, pi) };
    !key.is_null() && unsafe { __torajs_obj_declared_field_attrs(cell as *mut c_void, key) } >= 0
}

/// Is the DECLARED field named by `name` deleted (RFC
/// 20260915-declared-field-delete) — does it carry a sidecar whose
/// value word is the HOLE sentinel? Bytes spelling for the same
/// reason as [`__torajs_obj_field_is_nonenumerable`]; the header bit
/// answers "no" for free on every untouched instance.
///
/// # Safety
/// `cell` is a live `Tag::Obj` heap pointer; `name` points at
/// `name_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_field_is_deleted(
    cell: *const c_void,
    name: *const u8,
    name_len: u32,
) -> i64 {
    let want = unsafe { core::slice::from_raw_parts(name, name_len as usize) };
    // a member the literal that built the cell left out (never on a
    // cell that also carries sidecars)
    if unsafe { crate::struct_absent::lacks(cell, want) } {
        return 1;
    }
    let flags = unsafe { cell.cast::<u8>().add(6).cast::<u16>().read() };
    if flags & OBJ_HDR_FLAG_EXOTIC_FIELD == 0 {
        return 0;
    }
    let props = unsafe { struct_props(cell) };
    if props.is_null() {
        return 0;
    }
    let n = unsafe { __torajs_dynobj_iter_len(props) };
    for i in 0..n {
        let key = unsafe { __torajs_dynobj_iter_key(props, i) };
        if key.is_null() {
            continue;
        }
        if unsafe { crate::str_wtf8::StrWtf8::of(key) }.as_bytes() == want {
            return i64::from(
                unsafe { __torajs_dynobj_iter_value(props, i) } == DYNOBJ_HOLE_SENTINEL,
            );
        }
    }
    0
}

/// The `Str`-cell spelling of [`__torajs_obj_field_is_nonenumerable`].
///
/// The enumeration walks in this crate hold a layout name — a slice
/// into the emitted metadata — and ask by bytes. A compiled call site
/// holds the opposite thing: an interned literal cell, minted once at
/// link time. Both are asking the same question, so only the spelling
/// differs.
///
/// # Safety
/// `cell` is a live `Tag::Obj` heap pointer; `key` is a live `Str`
/// cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_key_is_nonenumerable(
    cell: *const c_void,
    key: *const c_void,
) -> i64 {
    let k = unsafe { crate::str_wtf8::StrWtf8::of(key) };
    unsafe { __torajs_obj_field_is_nonenumerable(cell, k.as_ptr(), k.len()) }
}

/// `torajs_rc::FLAG_OBJ_EXOTIC_FIELD` mirror — set once some declared
/// field of the instance carries a sidecar.
const OBJ_HDR_FLAG_EXOTIC_FIELD: u16 = 1 << 15;

/// `torajs_dynobj::layout::BUCKET_FLAG_*` mirrors — the three
/// PropertyDescriptor bits a dict entry's key word carries, and the
/// only three a sidecar ever holds.
pub(crate) const BUCKET_FLAG_WRITABLE: u64 = 1 << 0;
pub(crate) const BUCKET_FLAG_ENUMERABLE: u64 = 1 << 1;
pub(crate) const BUCKET_FLAG_CONFIGURABLE: u64 = 1 << 2;

/// `class_tag: u32` at `+8` of a `Tag::Obj` cell.
const OBJ_CLASS_TAG_OFF: usize = 8;

/// Does `cell`'s class layout declare `key` — as a data field or as an
/// accessor member?
///
/// A NULL layout row (anonymous struct interned too late) declares
/// nothing, which is the honest answer: such a cell carries only
/// expandos.
///
/// # Safety
/// `cell` is a live `Tag::Obj` heap pointer; `key` is a live `Str`
/// cell (a symbol key names no declared field and must not reach
/// here — callers route those separately).
pub(crate) unsafe fn layout_declares(cell: *const c_void, key: *const c_void) -> bool {
    let class_tag = unsafe {
        cell.cast::<u8>()
            .add(OBJ_CLASS_TAG_OFF)
            .cast::<u32>()
            .read()
    };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return false;
    }
    let k = unsafe { crate::str_wtf8::StrWtf8::of(key) };
    let (bytes, len) = (k.as_ptr(), k.len());
    unsafe {
        __torajs_struct_field_find(layout, bytes, len) != u32::MAX
            || ((__torajs_struct_accessor_find(layout, bytes, len, 0) != u32::MAX
                || __torajs_struct_accessor_find(layout, bytes, len, 1) != u32::MAX)
                // a replaced accessor's entry is the property itself
                && __torajs_obj_accessor_state(cell, bytes, len) != ACCESSOR_REPLACED)
    }
}

/// Is the declared member `name` no own property of `cell` any more:
/// deleted, left out by the literal that built the cell, or an
/// accessor an ordinary property replaced (whose dict entry then lists
/// itself, in its own key order)? The layout-name walks skip it.
///
/// # Safety
/// `cell` is a live `Tag::Obj` heap pointer whose layout declares
/// `name`; `name` points at `name_len` readable bytes.
pub(crate) unsafe fn layout_member_gone(
    cell: *const c_void,
    name: *const u8,
    name_len: u32,
) -> bool {
    unsafe {
        __torajs_obj_field_is_deleted(cell, name, name_len) != 0
            || __torajs_obj_accessor_state(cell, name, name_len) == ACCESSOR_REPLACED
    }
}

/// The layout row of `cell` (NULL for an anonymous struct interned too
/// late).
///
/// # Safety
/// `cell` is a live `Tag::Obj` heap pointer.
unsafe fn layout_of(cell: *const c_void) -> *const c_void {
    let class_tag = unsafe {
        cell.cast::<u8>()
            .add(OBJ_CLASS_TAG_OFF)
            .cast::<u32>()
            .read()
    };
    unsafe { __torajs_struct_layout_lookup(class_tag) }
}

/// The sidecar attributes recorded for `key` on `cell`, or `None` when
/// the field was never redefined.
///
/// `None` and `Some(0)` are different answers — a field redefined to
/// all-false attributes has a sidecar whose bits are zero — which is
/// why presence is asked separately rather than read off a zero
/// flags word.
///
/// # Safety
/// `cell` is a live `Tag::Obj` heap pointer; `key` is a live key cell.
pub(crate) unsafe fn sidecar_attrs(cell: *const c_void, key: *const c_void) -> Option<u64> {
    let props = unsafe { struct_props(cell) };
    let k = key.cast::<u8>();
    if props.is_null() || !unsafe { __torajs_dynobj_has(props, k) } {
        return None;
    }
    Some(unsafe { __torajs_dynobj_get_flags(props, k) })
}
