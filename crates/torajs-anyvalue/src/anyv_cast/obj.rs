//! The checked cast into an object-layout slot (RFC 20260922 §4.1, the
//! `Obj(sid)` row).
//!
//! The slot's readers load fields at its layout's offsets and, for a
//! class slot, call the class's own methods, so the cell that crosses
//! is the value itself, never a copy: a cell that fits the slot passes
//! with +1 and keeps its identity. What fits is the boxed adapter's
//! answer ([`__torajs_anyv_struct_fits`]): a class slot names the tags
//! its cell may wear (the class and its subclasses), any other layout
//! takes a cell with its fields.
//!
//! A dynamic object (an object literal through `any`, a `JSON.parse`
//! result) has no layout. The one exception is a fresh one: when the
//! slot's caller says the value is an owned temporary and nothing else
//! holds it (refcount 1), a struct cell with exactly its properties,
//! in its order, cannot be told apart from it, so the cast builds that
//! cell. Anything a copy would lose — another property, another order,
//! an accessor, a prototype of its own, a field the layout types as an
//! object — rejects instead.

use core::ffi::c_void;

use torajs_rc::Tag;
use torajs_rc::undef_cell::undef_cell_ptr;

use super::{cell_tag, reject};
use crate::STR_HDR_SIZE;
use crate::arg_struct_coerce::{
    __torajs_anyv_struct_fits, FieldInfo, OBJ_CLASS_TAG_OFF, OBJ_HEADER_SIZE, OBJ_TAG_OFF, StrSlice,
};
use crate::compare::STR_FLAG_IS_LATIN1;
use crate::member_get_own::DYNOBJ_HDR_FLAG_NULL_PROTO;
use crate::member_set::STR_LEN_OFF;
use crate::nanbox::{
    AnyValue, as_bool, as_double, as_int32, as_void_ptr, is_bool, is_cell, is_double, is_int32,
    is_null, is_short_str, is_undefined,
};

/// `mode` bit: the slot's annotation admits `undefined` / `null`
/// (`P | undefined`), which pass as the generic undefined cell and
/// NULL.
const CAST_OBJ_NULLISH: u32 = 1;
/// `mode` bit: the value is an owned temporary of a slot whose layout
/// is not a class's, so a fresh dynamic object may be rebuilt as a
/// cell of the layout.
const CAST_OBJ_FRESH: u32 = 2;

/// Mirror of `torajs-dynobj::layout::DYNOBJ_HDR_FLAG_NON_EXTENSIBLE`.
const DYNOBJ_HDR_FLAG_NON_EXTENSIBLE: u16 = 1 << 8;
/// Writable | enumerable | configurable: a plain data property.
const PLAIN_DATA: u64 = 0b111;
/// Below 2^53 every integer-valued double converts to i64 exactly.
const I64_EXACT: f64 = 9_007_199_254_740_992.0;

unsafe extern "C" {
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_count(layout: *const c_void) -> u32;
    fn __torajs_struct_field_name(layout: *const c_void, idx: u32) -> StrSlice;
    fn __torajs_struct_field_info(layout: *const c_void, idx: u32) -> FieldInfo;
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_value(obj: *const c_void, i: u64) -> u64;
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
    fn __torajs_rc_inc(p: *mut c_void);
    #[link_name = "__torajs_calloc"]
    fn calloc(size: usize) -> *mut u8;
}

/// Cast into a slot of the layout of `class_tag`: the cell with +1,
/// the undefined cell / NULL for a nullish value a `CAST_OBJ_NULLISH`
/// slot admits, a fresh cell for a fresh dynamic object of a
/// `CAST_OBJ_FRESH` slot, or NULL after recording a TypeError. `kin`
/// is the class list of a class slot, null for any other layout.
///
/// # Safety
/// A cell payload of `v` is live; `kin` is null or a class list;
/// `site` is a live Str literal block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_cast_obj(
    v: AnyValue,
    class_tag: u32,
    kin: *const u8,
    site: *const u8,
    mode: u32,
) -> *mut c_void {
    if mode & CAST_OBJ_NULLISH != 0 {
        if is_undefined(v) {
            return undef_cell_ptr().cast();
        }
        if is_null(v) {
            return core::ptr::null_mut();
        }
    }
    if unsafe { __torajs_anyv_struct_fits(v, class_tag, kin) } == 1 {
        let p = as_void_ptr(v);
        unsafe { __torajs_rc_inc(p) };
        return p;
    }
    if mode & CAST_OBJ_FRESH != 0
        && let Some(cell) = unsafe { rebuild_fresh(v, class_tag) }
    {
        return cell;
    }
    unsafe { reject(v, site) };
    core::ptr::null_mut()
}

/// A cell of the layout of `class_tag` built from `v`, when `v` is a
/// dynamic object nothing else holds whose own properties are exactly
/// the layout's fields, in the layout's order, each a plain data
/// property of the field's type.
///
/// # Safety
/// A cell payload of `v` is live.
unsafe fn rebuild_fresh(v: AnyValue, class_tag: u32) -> Option<*mut c_void> {
    if !is_cell(v) || unsafe { cell_tag(v) } != Tag::DynObj as u16 {
        return None;
    }
    let obj = as_void_ptr(v);
    let (rc, flags) = unsafe {
        (
            obj.cast::<u32>().read(),
            obj.cast::<u8>().add(6).cast::<u16>().read(),
        )
    };
    if rc != 1 || flags & (DYNOBJ_HDR_FLAG_NULL_PROTO | DYNOBJ_HDR_FLAG_NON_EXTENSIBLE) != 0 {
        return None;
    }
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return None;
    }
    let props = unsafe { own_props(obj) };
    let n = unsafe { __torajs_struct_field_count(layout) };
    if props.len() != n as usize {
        return None;
    }
    let mut fields = Vec::with_capacity(props.len());
    for (idx, &(key, val, flags)) in props.iter().enumerate() {
        let name = unsafe { __torajs_struct_field_name(layout, idx as u32) };
        let name = unsafe { core::slice::from_raw_parts(name.ptr, name.len) };
        let info = unsafe { __torajs_struct_field_info(layout, idx as u32) };
        let fits = flags == PLAIN_DATA
            && info.field_byte_offset != 0
            && unsafe { key_is(key, name) && field_takes(val, info.type_tag) };
        if !fits {
            return None;
        }
        fields.push((info, val));
    }
    let cell = unsafe { calloc(OBJ_HEADER_SIZE + fields.len() * 8) };
    unsafe {
        *(cell as *mut u32) = 1;
        *(cell.add(OBJ_TAG_OFF) as *mut u16) = Tag::Obj as u16;
        *(cell.add(OBJ_CLASS_TAG_OFF) as *mut u32) = class_tag;
        for (info, val) in fields {
            let slot = field_slot(val, info.type_tag);
            (cell.add(info.field_byte_offset as usize) as *mut u64).write(slot);
        }
    }
    Some(cell.cast())
}

/// The own properties of the dynamic object `obj` in insertion order:
/// key Str cell, value, descriptor flags.
///
/// # Safety
/// `obj` is a live DynObj cell.
unsafe fn own_props(obj: *const c_void) -> Vec<(*const u8, u64, u64)> {
    let len = unsafe { __torajs_dynobj_iter_len(obj) };
    (0..len)
        .filter_map(|i| unsafe {
            let key = __torajs_dynobj_iter_key(obj, i);
            (!key.is_null()).then(|| {
                (
                    key as *const u8,
                    __torajs_dynobj_iter_value(obj, i),
                    __torajs_dynobj_iter_flags(obj, i),
                )
            })
        })
        .collect()
}

/// The key cell `key` is the string `name` (an ASCII field name).
///
/// # Safety
/// `key` is a live key cell (a Str or a Symbol).
unsafe fn key_is(key: *const u8, name: &[u8]) -> bool {
    unsafe {
        if key.add(4).cast::<u16>().read() != Tag::Str as u16 {
            return false;
        }
        let latin1 = key.add(6).cast::<u16>().read() & STR_FLAG_IS_LATIN1 != 0;
        let len = key.add(STR_LEN_OFF).cast::<u32>().read() as usize;
        latin1
            && len == name.len()
            && core::slice::from_raw_parts(key.add(STR_HDR_SIZE), len) == name
    }
}

/// Whether a field of type tag `type_tag` takes the property value
/// `val` as it is. Type tags follow `ssa::field_type_tag_of`: 0 any,
/// 1 integer number, 2 number, 3 boolean, 4 string; a field of any
/// other type (an object, an array, a function) would need the
/// property's own layout, which a dynamic object does not have, and
/// an accessor is not a value.
///
/// # Safety
/// A cell payload of `val` is live.
unsafe fn field_takes(val: u64, type_tag: u8) -> bool {
    let tag = is_cell(val).then(|| unsafe { cell_tag(val) });
    match type_tag {
        0 => tag != Some(Tag::AccessorPair as u16),
        1 => {
            is_int32(val)
                || (is_double(val)
                    && as_double(val).fract() == 0.0
                    && as_double(val).abs() < I64_EXACT)
        }
        2 => is_int32(val) || is_double(val),
        3 => is_bool(val),
        4 => is_short_str(val) || tag == Some(Tag::Str as u16),
        _ => false,
    }
}

/// The bits a field of type tag `type_tag` holds for `val`, which it
/// takes ([`field_takes`]); a heap value comes with a stake of its own.
///
/// # Safety
/// As [`field_takes`].
unsafe fn field_slot(val: u64, type_tag: u8) -> u64 {
    match type_tag {
        1 if is_int32(val) => as_int32(val) as i64 as u64,
        1 => as_double(val) as i64 as u64,
        2 if is_int32(val) => (as_int32(val) as f64).to_bits(),
        2 => as_double(val).to_bits(),
        3 => u64::from(as_bool(val)),
        4 => unsafe { crate::nanbox_ffi::__torajs_anyv_to_str(val) as u64 },
        _ => {
            if is_cell(val) {
                unsafe { __torajs_rc_inc(as_void_ptr(val)) };
            }
            val
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nanbox::{VALUE_NULL, VALUE_TRUE, VALUE_UNDEFINED, box_double, box_int32};

    #[test]
    fn scalar_fields_take_values_of_their_type() {
        unsafe {
            assert!(field_takes(box_int32(3), 1) && field_takes(box_double(4.0), 1));
            assert!(!field_takes(box_double(4.5), 1));
            assert_eq!(field_slot(box_double(4.0), 1), 4);
            assert_eq!(field_slot(box_int32(2), 2), 2.0f64.to_bits());
            assert!(field_takes(VALUE_TRUE, 3) && !field_takes(box_int32(1), 3));
            assert_eq!(field_slot(VALUE_TRUE, 3), 1);
            assert!(!field_takes(VALUE_TRUE, 4));
            assert!(!field_takes(box_int32(1), 7));
        }
    }

    #[test]
    fn an_any_field_takes_any_primitive_as_it_is() {
        unsafe {
            for v in [VALUE_UNDEFINED, VALUE_NULL, VALUE_TRUE, box_int32(9)] {
                assert!(field_takes(v, 0));
                assert_eq!(field_slot(v, 0), v);
            }
        }
    }

    #[test]
    fn a_nullish_slot_admits_nullish_values_as_their_spellings() {
        unsafe {
            let undef = __torajs_anyv_cast_obj(
                VALUE_UNDEFINED,
                1,
                core::ptr::null(),
                core::ptr::null(),
                CAST_OBJ_NULLISH,
            );
            assert_eq!(undef, undef_cell_ptr().cast());
            let null = __torajs_anyv_cast_obj(
                VALUE_NULL,
                1,
                core::ptr::null(),
                core::ptr::null(),
                CAST_OBJ_NULLISH,
            );
            assert!(null.is_null());
        }
    }
}
