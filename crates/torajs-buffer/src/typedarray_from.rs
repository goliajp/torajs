//! §23.2.5.1 steps 5.b-5.d — building a typed array from an object
//! (RFC 20260823-typedarray-substrate 刀 3).
//!
//! Three sources, and the spec treats them as three different
//! operations:
//!
//! - **another typed array** (§23.2.5.1.2): element-by-element with
//!   conversion, and a Number source into a BigInt destination (or
//!   the reverse) is a TypeError rather than a coercion;
//! - **an iterable** (§23.2.5.1.4): drained to a list first, so the
//!   length is known before a single element is stored;
//! - **an array-like** (§23.2.5.1.3): `length` read once, then each
//!   index in turn.
//!
//! The last two are exactly what `Array.from` already distinguishes
//! (§23.1.2.1 makes the same @@iterator test), so they share its
//! kernel rather than growing a second copy of the walk that could
//! drift from it.

use core::ffi::{c_char, c_void};

use torajs_anyvalue::__torajs_anyv_box_pointer;
use torajs_anyvalue::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, box_void_ptr};

use crate::arraybuffer::{NOT_RESIZABLE, allocate};
use crate::typedarray::{Kind, is_typedarray, kind_of, resolve};
use crate::typedarray_ctor::mint;
use crate::typedarray_elem;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_throw_range_error(msg: *const u8);
    fn __torajs_throw_check() -> i64;
    fn __torajs_anyv_rc_dec(v: AnyValue);
    /// `Array.from(items)` — the §23.1.2.1 walk, which makes the
    /// same @@iterator-or-array-like decision §23.2.5.1 does. NULL
    /// when it threw.
    fn __torajs_array_from_dyn(items: AnyValue, mapfn: AnyValue, this_arg: AnyValue)
    -> *mut c_void;
    fn __torajs_arr_index_get(arr: *mut c_void, idx: i64) -> AnyValue;
    fn __torajs_any_length_get(recv: AnyValue) -> AnyValue;
    fn __torajs_anyv_to_number(v: AnyValue) -> f64;
    fn __torajs_arr_drop(p: *mut c_void);
}

/// §23.2.5.1 step 5 — `new T(object)`. `object` is BORROWED.
///
/// # Safety
/// `object` is a live cell AnyValue that is neither an ArrayBuffer
/// (the caller routes that) nor a primitive.
pub(crate) unsafe fn from_object(kind: Kind, object: AnyValue) -> AnyValue {
    if is_typedarray(object) {
        return unsafe { from_typedarray(kind, object) };
    }
    unsafe { from_list(kind, object) }
}

/// §23.2.5.1.2 InitializeTypedArrayFromTypedArray. The content
/// types must agree: a BigInt element type refuses a Number source
/// outright, which is the one place a typed array will not coerce.
///
/// # Safety
/// `src` is a live TypedArray AnyValue.
unsafe fn from_typedarray(kind: Kind, src: AnyValue) -> AnyValue {
    unsafe {
        let sptr = as_void_ptr(src);
        let src_kind = kind_of(sptr);
        if src_kind.is_bigint() != kind.is_bigint() {
            __torajs_throw_type_error(
                c"Cannot mix BigInt and other types, use explicit conversions".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let Some((sbase, len)) = resolve(sptr) else {
            __torajs_throw_type_error(
                c"Cannot construct a typed array on a detached buffer".as_ptr(),
            );
            return VALUE_UNDEFINED;
        };
        let Some(view) = alloc_view(kind, len) else {
            return VALUE_UNDEFINED;
        };
        let (dbase, _) = resolve(as_void_ptr(view)).expect("a fresh view is in bounds");
        for i in 0..len {
            // Through the element ABSTRACTION rather than a memcpy,
            // even when the two kinds match: the conversion is what
            // the spec says happens, and a same-kind copy is the
            // identity case of it rather than a separate path.
            let v = typedarray_elem::read(sbase, src_kind, i);
            if let Some(c) = typedarray_elem::coerce(kind, v) {
                typedarray_elem::store(dbase, kind, i, c);
            }
            __torajs_anyv_rc_dec(v);
        }
        view
    }
}

/// §23.2.5.1.3 / §23.2.5.1.4 — an array-like or an iterable, both
/// through the one walk that already tells them apart.
///
/// # Safety
/// `object` is a live cell AnyValue.
unsafe fn from_list(kind: Kind, object: AnyValue) -> AnyValue {
    unsafe {
        let list = __torajs_array_from_dyn(object, VALUE_UNDEFINED, VALUE_UNDEFINED);
        if list.is_null() || __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let list_any = box_void_ptr(list);
        let len_any = __torajs_any_length_get(list_any);
        let len = __torajs_anyv_to_number(len_any) as i64;
        __torajs_anyv_rc_dec(len_any);
        let Some(view) = alloc_view(kind, len) else {
            __torajs_arr_drop(list);
            return VALUE_UNDEFINED;
        };
        let (dbase, _) = resolve(as_void_ptr(view)).expect("a fresh view is in bounds");
        for i in 0..len {
            let v = __torajs_arr_index_get(list, i);
            let coerced = typedarray_elem::coerce(kind, v);
            __torajs_anyv_rc_dec(v);
            match coerced {
                Some(c) => typedarray_elem::store(dbase, kind, i, c),
                // A rejected element aborts the construction with
                // the pending throw already recorded — the partly
                // filled view is dropped rather than handed back.
                None => {
                    __torajs_anyv_rc_dec(view);
                    __torajs_arr_drop(list);
                    return VALUE_UNDEFINED;
                }
            }
        }
        __torajs_arr_drop(list);
        view
    }
}

/// A fresh view of `len` elements over its own fixed-length buffer.
/// `None` = the allocation was rejected and the RangeError recorded.
///
/// # Safety
/// Allocates.
unsafe fn alloc_view(kind: Kind, len: i64) -> Option<AnyValue> {
    unsafe {
        let Some(bytes) = len.checked_mul(kind.element_size()) else {
            __torajs_throw_range_error(b"Invalid typed array length\0".as_ptr());
            return None;
        };
        let buf_cell = allocate(bytes, NOT_RESIZABLE);
        if buf_cell.is_null() {
            __torajs_throw_range_error(b"Invalid typed array length\0".as_ptr());
            return None;
        }
        let buffer = __torajs_anyv_box_pointer(buf_cell);
        let view = mint(kind, buffer, 0, len);
        __torajs_anyv_rc_dec(buffer);
        Some(view)
    }
}
