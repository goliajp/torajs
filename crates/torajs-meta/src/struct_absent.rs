//! The absent-member set of a struct cell (`torajs_rc::FLAG_OBJ_ABSENT_FIELD`).
//!
//! An object literal that leaves out an optional member of the type it
//! is stored as (`const p: { a?: number; b: number } = { b: 2 }`) builds
//! a cell of the declared layout whose `a` is no own property
//! (§10.1.11). The literal records that in the cell itself: the header
//! bit goes up and the high half of the `+8` word holds the set, bit
//! `i` for layout slot `i`. Those are two stores among the ones that
//! build the cell, so leaving a member out costs what spelling it costs.
//!
//! Every runtime face that asks whether a declared member is an own
//! property goes through [`crate::struct_field_attrs::__torajs_obj_field_is_deleted`],
//! which reads the set ([`lacks`]) the way it reads a deletion record,
//! and a store that creates the member clears its bit. The faces that
//! change what a member is (a define, an integrity level, a private
//! element install) first settle the set into the deletion records
//! `delete` leaves, so the set never coexists with a frozen, sealed,
//! non-extensible or redefined cell, nor with a private-element cursor.

use core::ffi::c_void;

/// `torajs_rc::FLAG_OBJ_ABSENT_FIELD` mirror (Tag::Obj-private bit 11).
const FLAG_OBJ_ABSENT_FIELD: u16 = 1 << 11;

unsafe extern "C" {
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_order(obj: *const c_void, out: *mut u64, cap: u64) -> u64;
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_count(layout: *const c_void) -> u32;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    fn __torajs_struct_field_info(layout: *const c_void, idx: u32) -> FieldInfo;
    fn __torajs_struct_field_name(layout: *const c_void, idx: u32) -> StrSlice;
    /// torajs-dynobj — the deletion record of a declared field.
    fn __torajs_obj_declared_field_delete(obj: *mut c_void, key: *mut c_void) -> i64;
    fn __torajs_str_drop(s: *mut u8);
}

/// `torajs-structmeta::FieldInfo` mirror.
#[repr(C)]
struct FieldInfo {
    field_byte_offset: u32,
    type_tag: u8,
}

/// `torajs-structmeta::StrSlice` mirror.
#[repr(C)]
struct StrSlice {
    ptr: *const u8,
    len: usize,
}

/// The first field slot's byte offset; the set's offset in the cell.
const OBJ_HEADER_SIZE: u32 = 32;
const ABSENT_SET_OFF: usize = 12;

unsafe fn absent_set(cell: *const c_void) -> u32 {
    let flags = unsafe { cell.cast::<u8>().add(6).cast::<u16>().read() };
    if flags & FLAG_OBJ_ABSENT_FIELD == 0 {
        return 0;
    }
    unsafe { cell.cast::<u8>().add(ABSENT_SET_OFF).cast::<u32>().read() }
}

fn slot_of(field_byte_offset: u32) -> Option<u32> {
    let slot = field_byte_offset.checked_sub(OBJ_HEADER_SIZE)? / 8;
    (slot < 32).then_some(slot)
}

unsafe fn layout_of(cell: *const c_void) -> *const c_void {
    let tag = unsafe { cell.cast::<u8>().add(8).cast::<u32>().read() };
    unsafe { __torajs_struct_layout_lookup(tag) }
}

/// The entries of the struct cell's expando dict `props` (NULL when it
/// has none) in ES dict order, or `None` when its own keys are exactly
/// the layout's: no expando, and no member a literal left out.
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell; `props` its expando dict or NULL.
pub(crate) unsafe fn expando_order(cell: *const c_void, props: *const c_void) -> Option<Vec<u64>> {
    let n_props = match props.is_null() {
        true => 0,
        false => unsafe { __torajs_dynobj_iter_len(props) },
    };
    if n_props == 0 {
        return (unsafe { absent_set(cell) } != 0).then(Vec::new);
    }
    let mut order = vec![0u64; n_props as usize];
    let n = unsafe { __torajs_dynobj_iter_order(props, order.as_mut_ptr(), n_props) };
    order.truncate(n as usize);
    Some(order)
}

/// Whether the struct cell `cell` lacks its declared member `name`
/// because the literal that built it left it out.
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell.
pub(crate) unsafe fn lacks(cell: *const c_void, name: &[u8]) -> bool {
    let set = unsafe { absent_set(cell) };
    if set == 0 {
        return false;
    }
    let layout = unsafe { layout_of(cell) };
    let row = unsafe { __torajs_struct_field_find(layout, name.as_ptr(), name.len() as u32) };
    if row == u32::MAX {
        return false;
    }
    let off = unsafe { __torajs_struct_field_info(layout, row) }.field_byte_offset;
    slot_of(off).is_some_and(|s| set & (1 << s) != 0)
}

/// A store creates the member `key` the literal that built `cell` left
/// out (§10.1.9.2 step 2.c.i; the caller has run the extensible gate):
/// its bit goes, and the bit of the set with it once the set is empty.
/// Answers 1 when the set named the member.
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell; `key` a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_absent_clear(cell: *mut c_void, key: *const c_void) -> i64 {
    let set = unsafe { absent_set(cell) };
    if set == 0 {
        return 0;
    }
    let k = unsafe { crate::str_wtf8::StrWtf8::of(key) };
    let name = k.as_bytes();
    let layout = unsafe { layout_of(cell) };
    let row = unsafe { __torajs_struct_field_find(layout, name.as_ptr(), name.len() as u32) };
    let off = match row {
        u32::MAX => return 0,
        r => unsafe { __torajs_struct_field_info(layout, r) }.field_byte_offset,
    };
    let Some(bit) = slot_of(off).map(|s| 1u32 << s).filter(|b| set & b != 0) else {
        return 0;
    };
    unsafe {
        cell.cast::<u8>()
            .add(ABSENT_SET_OFF)
            .cast::<u32>()
            .write(set & !bit);
        if set == bit {
            *cell.cast::<u8>().add(6).cast::<u16>() &= !FLAG_OBJ_ABSENT_FIELD;
        }
    }
    1
}

/// Turn the absent set of the struct cell `cell` into deletion records
/// and drop the bit; the caller has seen the bit up. Reached through
/// the `__torajs_obj_absent_settle` seam in torajs-dispatch, which `tr
/// build` stubs in a program none of whose literals leaves a member out.
///
/// # Safety
/// `cell` is a live `Tag::Obj` cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_absent_settle_impl(cell: *mut c_void) {
    let set = unsafe { absent_set(cell) };
    // the half goes back to being the (unused) private-element cursor
    unsafe {
        *cell.cast::<u8>().add(6).cast::<u16>() &= !FLAG_OBJ_ABSENT_FIELD;
        cell.cast::<u8>().add(ABSENT_SET_OFF).cast::<u32>().write(0);
    }
    let layout = unsafe { layout_of(cell) };
    let n = unsafe { __torajs_struct_field_count(layout) };
    for row in 0..n {
        let off = unsafe { __torajs_struct_field_info(layout, row) }.field_byte_offset;
        if !slot_of(off).is_some_and(|s| set & (1 << s) != 0) {
            continue;
        }
        let name = unsafe { __torajs_struct_field_name(layout, row) };
        let bytes = match name.ptr.is_null() {
            true => &[][..],
            false => unsafe { core::slice::from_raw_parts(name.ptr, name.len) },
        };
        let key = unsafe { crate::reflect::alloc_str_key(bytes) };
        unsafe {
            __torajs_obj_declared_field_delete(cell, key.cast());
            __torajs_str_drop(key);
        }
    }
}
