//! `any`-receiver writes into a class-layout data field: which
//! payloads the field's own slot can hold.

use core::ffi::c_void;

use torajs_rc::{AnySlotTag, Tag};

use crate::nanbox_ffi::__torajs_anyv_rc_dec;

unsafe extern "C" {
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    fn __torajs_struct_field_info(layout: *const c_void, idx: u32) -> FieldInfo;
    fn __torajs_str_undef() -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
}

/// Mirror of `torajs-structmeta::FieldInfo`.
#[repr(C)]
struct FieldInfo {
    field_byte_offset: u32,
    type_tag: u8,
}

const OBJ_CLASS_TAG_OFF: usize = 8;

/// `any`-receiver assignment into a class-layout DATA field — the
/// §10.1.9 [[Set]] face `member_set`'s struct arm previously
/// rejected wholesale. Consumes the `(tag, value)` payload on
/// success (true); an absent field / accessor spelling / mismatched
/// payload type answers false with the payload untouched so the
/// caller's reject stays loud. The caller has already run the
/// frozen gate (see the body).
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` is a live Str
/// cell; the payload follows the lowering's consume convention
/// (heap tag carries a transferred +1).
pub(crate) unsafe fn struct_data_field_set(
    ptr: *mut c_void,
    key: *const c_void,
    tag: u64,
    value: u64,
) -> bool {
    let class_tag = unsafe { ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return false;
    }
    let k = unsafe { torajs_rc::str_wtf8::StrWtf8::of(key) };
    let idx = unsafe { __torajs_struct_field_find(layout, k.as_ptr(), k.len()) };
    if idx == u32::MAX {
        return false;
    }
    let info = unsafe { __torajs_struct_field_info(layout, idx) };
    let slot = unsafe { ptr.cast::<u8>().add(info.field_byte_offset as usize) } as *mut u64;
    // The frozen gate is the CALLER's (`struct_field_ffi::
    // frozen_data_field_refuses`): it flavors the refusal — §13.15.2
    // strict throws, sloppy and Reflect.set answer false — and this
    // arm only ever sees a writable field.
    match info.type_tag {
        // Any slot — the NaN-box stores verbatim; the old box's
        // stake is released, the payload's transferred +1 becomes
        // the slot's.
        0 => unsafe {
            let old = slot.read();
            slot.write(crate::nanbox_encode::__torajs_anyv_box_from_pair(
                tag as i64,
                value as i64,
            ));
            __torajs_anyv_rc_dec(old);
            true
        },
        // Str slot ← null / undefined: the slot's own spellings of
        // them (NULL, the undefined sentinel cell), the values a typed
        // Str read already tells apart. Not on an error object: there
        // the sentinel in `name` / `message` means the own property is
        // absent, which is not what an own `undefined` is.
        4 if (tag == AnySlotTag::Null as u64 || tag == AnySlotTag::Undef as u64)
            && !unsafe { crate::member_get::header_flag(ptr, torajs_rc::FLAG_ERROR) } =>
        unsafe {
            let old = slot.read();
            slot.write(if tag == AnySlotTag::Null as u64 {
                0
            } else {
                __torajs_str_undef() as u64
            });
            if old != 0 {
                __torajs_str_drop(old as *mut c_void);
            }
            true
        },
        // Str slot — accepts a heap Str cell (the sentinel included:
        // it IS a Str cell whose rc traffic no-ops). Any other heap
        // shape / a non-heap payload falls back to the loud reject.
        4 => unsafe {
            if tag != AnySlotTag::Heap as u64 || value == 0 {
                return false;
            }
            let cell_tag = (value as *const u8).add(4).cast::<u16>().read();
            if cell_tag != Tag::Str as u16 {
                return false;
            }
            let old = slot.read();
            slot.write(value);
            if old != 0 {
                __torajs_str_drop(old as *mut c_void);
            }
            true
        },
        // I64 slot ← I64 payload.
        1 if tag == AnySlotTag::I64 as u64 => unsafe {
            slot.write(value);
            true
        },
        // I64 slot ← an integer-valued F64 payload, converted — the
        // mirror of the F64 arm's I64 conversion below. The typed
        // lanes box a number[] element as F64 whatever its value, so
        // a callback's `this.count = v` met this slot with F64 bits
        // for a plain integer. A fractional value keeps the loud
        // reject: it cannot live in this slot without breaking the
        // layout's invariant.
        1 if tag == AnySlotTag::F64 as u64 => {
            let f = f64::from_bits(value);
            if f.fract() != 0.0 || f < i64::MIN as f64 || f >= i64::MAX as f64 {
                return false;
            }
            unsafe {
                slot.write(f as i64 as u64);
            }
            true
        }
        // F64 slot ← F64 bits, or an I64 payload converted.
        2 if tag == AnySlotTag::F64 as u64 => unsafe {
            slot.write(value);
            true
        },
        2 if tag == AnySlotTag::I64 as u64 => unsafe {
            slot.write((value as i64 as f64).to_bits());
            true
        },
        // Bool slot ← Bool payload.
        3 if tag == AnySlotTag::Bool as u64 => unsafe {
            slot.write(value);
            true
        },
        // A heap-cell slot (array, object, function, collection, …)
        // ← null / undefined: NULL and the generic undefined cell,
        // which every typed read of such a field already treats as
        // possible (typeof, member guards). A heap value stays in the
        // shadow: the slot's element kind / class is not in the
        // layout table, so nothing here can prove the cell fits it.
        // A function (8) or symbol (14) slot keeps the shadow too: the
        // typed JSON.stringify omits those keys statically and learns
        // of a stored null only through the shadow's exotic bit.
        6 | 7 | 9..=13 | 15..=20
            if tag == AnySlotTag::Null as u64 || tag == AnySlotTag::Undef as u64 =>
        unsafe {
            let old = slot.read();
            slot.write(if tag == AnySlotTag::Null as u64 {
                0
            } else {
                torajs_rc::undef_cell::undef_cell_ptr() as u64
            });
            if old != 0 {
                __torajs_anyv_rc_dec(crate::nanbox_encode::__torajs_anyv_box_from_pair(
                    AnySlotTag::Heap as i64,
                    old as i64,
                ));
            }
            true
        },
        // Mismatched payload for a typed slot — the caller keeps it in
        // the declared-field shadow.
        _ => false,
    }
}
