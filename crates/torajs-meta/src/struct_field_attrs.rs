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

use core::ffi::{c_char, c_void};

use crate::obj_own_keys_struct::struct_props;

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
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
    /// torajs-dynobj — the live attributes of a declared member: its
    /// sidecar when one exists, else the layout default the integrity
    /// level moves. -1 when the layout declares nothing under the key.
    fn __torajs_obj_declared_field_attrs(obj: *mut c_void, key: *mut c_void) -> i64;
}

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
    let flags = unsafe { cell.cast::<u8>().add(6).cast::<u16>().read() };
    if flags & OBJ_HDR_FLAG_EXOTIC_FIELD == 0 {
        return 0;
    }
    let props = unsafe { struct_props(cell) };
    if props.is_null() {
        return 0;
    }
    let want = unsafe { core::slice::from_raw_parts(name, name_len as usize) };
    let n = unsafe { __torajs_dynobj_iter_len(props) };
    for i in 0..n {
        let key = unsafe { __torajs_dynobj_iter_key(props, i) };
        if key.is_null() {
            continue;
        }
        let klen = unsafe { key.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() };
        let kbytes = unsafe {
            core::slice::from_raw_parts(key.cast::<u8>().add(STR_DATA_OFF), klen as usize)
        };
        if kbytes == want {
            return i64::from(
                unsafe { __torajs_dynobj_iter_flags(props, i) } & BUCKET_FLAG_ENUMERABLE == 0,
            );
        }
    }
    0
}

/// `torajs_rc::FLAG_FROZEN` / `FLAG_OBJ_EXOTIC_FIELD` mirrors — the
/// two header bits that make a typed field store anything other than
/// one store.
const OBJ_HDR_FLAG_FROZEN: u16 = 1 << 4;
const OBJ_HDR_FLAG_EXOTIC_FIELD: u16 = 1 << 15;

/// The mutation guard `ssa_lower` emits at every typed
/// `instance.field = value` site — successor to
/// `__torajs_obj_check_not_frozen`, which asked only half the
/// question.
///
/// Both refusals arm a `TypeError` and RETURN; the throw lands at the
/// `emit_throw_check` the caller emits immediately after, before the
/// store. §10.1.9 / §7.3.14: an assignment whose target is
/// non-writable fails, and module code is strict.
///
/// The fast path is one header load and one masked test — the same
/// shape the frozen-only guard had, with a wider immediate. A cell
/// that was never frozen and never redefined leaves here without
/// touching the expando dict.
///
/// # Safety
/// `p` is NULL or a live heap pointer; `name` is a live `Str` cell
/// naming the field being stored (a compile-time literal).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_check_field_writable(p: *const c_void, name: *const c_void) {
    if p.is_null() {
        return;
    }
    let flags = unsafe { p.cast::<u8>().add(6).cast::<u16>().read() };
    if flags & (OBJ_HDR_FLAG_FROZEN | OBJ_HDR_FLAG_EXOTIC_FIELD) == 0 {
        return;
    }
    if flags & OBJ_HDR_FLAG_FROZEN != 0 {
        unsafe { __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr()) };
        return;
    }
    // Redefined somewhere on this instance — is it THIS field?
    if unsafe { sidecar_attrs(p, name) }.is_some_and(|a| a & BUCKET_FLAG_WRITABLE == 0) {
        unsafe { __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr()) };
    }
}

/// `torajs_dynobj::layout::BUCKET_FLAG_*` mirrors — the three
/// PropertyDescriptor bits a dict entry's key word carries, and the
/// only three a sidecar ever holds.
pub(crate) const BUCKET_FLAG_WRITABLE: u64 = 1 << 0;
pub(crate) const BUCKET_FLAG_ENUMERABLE: u64 = 1 << 1;
pub(crate) const BUCKET_FLAG_CONFIGURABLE: u64 = 1 << 2;

/// `class_tag: u32` at `+8` of a `Tag::Obj` cell.
const OBJ_CLASS_TAG_OFF: usize = 8;
/// `Str` payload offsets (torajs-str mirror).
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;

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
    let k = key as *const u8;
    let len = unsafe { k.add(STR_LEN_OFF).cast::<u32>().read() };
    let bytes = unsafe { k.add(STR_DATA_OFF) };
    unsafe {
        __torajs_struct_field_find(layout, bytes, len) != u32::MAX
            || __torajs_struct_accessor_find(layout, bytes, len, 0) != u32::MAX
            || __torajs_struct_accessor_find(layout, bytes, len, 1) != u32::MAX
    }
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
