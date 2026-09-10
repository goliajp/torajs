//! 628-05 — the own-key ORDER anchors of runtime-computed INSTANCE
//! fields, keyed by class tag.
//!
//! `class C { foo = 1; [x] = 42; bar = 2 }` stores its computed field
//! in the +24 expando dict (the layout has no row for it), and every
//! own-key walk lists layout rows first, expandos after — so
//! `Object.keys` answered `foo,bar,b` where §10.1.11.1 says
//! `foo,b,bar`. The compiler knows which named field the computed one
//! was declared after and hands it here at the class-decl position
//! (`__torajs_class_computed_field_register`); the walks
//! (`obj_own_keys_struct` / `struct_enum` / JSON) emit the expando
//! entry right behind that row.
//!
//! Same shape as the sibling `PROTOS_BY_TAG_IMM` tables: a
//! process-lifetime, single-threaded-runtime `static mut` written by
//! direct indexing only (rust 2024 `static_mut_refs` posture — see
//! classmeta.rs). The key cells are owned by the table for the
//! process lifetime; a subclass registers its ancestors' anchors under
//! its own tag (the compiler emits them root-most first), so a lookup
//! never walks the class chain.

use core::ffi::c_void;

use super::{MAX_CLASSES, in_range};
use crate::reflect::{TAG_STR, heap_type_tag, is_cell_imm};
use crate::str_wtf8::StrWtf8;

unsafe extern "C" {
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    fn __torajs_rc_inc(p: *mut c_void);
}

/// `class_tag` u32 offset inside a `Tag::Obj` instance.
const OBJ_CLASS_TAG_OFF: usize = 8;

/// The anchor of a computed field that leads its class (or whose
/// named anchor the layout does not carry): before every row.
pub(crate) const BEFORE_EVERY_ROW: u32 = u32::MAX;

/// One registered computed instance field.
#[derive(Clone, Copy)]
pub(crate) struct ComputedField {
    /// The layout row it was declared right after, or
    /// [`BEFORE_EVERY_ROW`].
    pub after_row: u32,
    /// Its property key — a Str cell the table owns.
    pub key: *mut c_void,
}

static mut COMPUTED_FIELDS_BY_TAG: [*mut Vec<ComputedField>; MAX_CLASSES] =
    [core::ptr::null_mut(); MAX_CLASSES];

/// Record one computed instance field of class `tag`: `after` is the
/// Str cell naming the layout row it follows (NULL / empty = before
/// every row), `key` the boxed property key the ctor prefix stores
/// under. A Symbol key is outside every string-keyed walk and is not
/// recorded.
///
/// # Safety
/// `after` is NULL or a live Str cell; `key` is a valid AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_computed_field_register(
    tag: i64,
    after: *const u8,
    key: u64,
) {
    if !in_range(tag) || !is_cell_imm(key) {
        return;
    }
    let kp = key as *mut c_void;
    if unsafe { heap_type_tag(kp) } != TAG_STR {
        return;
    }
    let layout = unsafe { __torajs_struct_layout_lookup(tag as u32) };
    let after_row = if after.is_null() || layout.is_null() {
        BEFORE_EVERY_ROW
    } else {
        let view = unsafe { StrWtf8::of(after.cast::<c_void>()) };
        let name = view.as_bytes();
        if name.is_empty() {
            BEFORE_EVERY_ROW
        } else {
            unsafe { __torajs_struct_field_find(layout, name.as_ptr(), name.len() as u32) }
        }
    };
    // SAFETY: single-threaded JS runtime, direct indexed access; the
    // table keeps a share of the key for the process lifetime.
    unsafe {
        __torajs_rc_inc(kp);
        let mut v = COMPUTED_FIELDS_BY_TAG[tag as usize];
        if v.is_null() {
            v = Box::into_raw(Box::new(Vec::new()));
            COMPUTED_FIELDS_BY_TAG[tag as usize] = v;
        }
        (*v).push(ComputedField { after_row, key: kp });
    }
}

/// The registered computed fields of the class `cell` belongs to, in
/// declaration order (ancestors first); empty for every other class.
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell.
pub(crate) unsafe fn computed_fields(cell: *const c_void) -> &'static [ComputedField] {
    let tag = unsafe {
        cell.cast::<u8>()
            .add(OBJ_CLASS_TAG_OFF)
            .cast::<u32>()
            .read()
    } as usize;
    if tag >= MAX_CLASSES {
        return &[];
    }
    // SAFETY: single-threaded JS runtime, direct indexed read; a
    // registered Vec lives for the process lifetime.
    let v = unsafe { COMPUTED_FIELDS_BY_TAG[tag] };
    if v.is_null() {
        &[]
    } else {
        unsafe { (*v).as_slice() }
    }
}

/// Byte equality of two Str cells (an expando dict key against a
/// registered one — the dict interns nothing, so identity is not a
/// test).
///
/// # Safety
/// Both are live Str cells.
pub(crate) unsafe fn key_eq(a: *const c_void, b: *const c_void) -> bool {
    let (a, b) = unsafe { (StrWtf8::of(a), StrWtf8::of(b)) };
    a.as_bytes() == b.as_bytes()
}
