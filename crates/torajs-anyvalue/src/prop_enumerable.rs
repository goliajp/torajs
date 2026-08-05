//! `Object.prototype.propertyIsEnumerable` substrate — split out of
//! `prop_has.rs` when the struct-expando flag read pushed that file
//! past the 500-line limit.
//!
//! Presence and enumerability are two questions, and only the second
//! one has to read per-entry attributes: a struct FIELD has none (it
//! is always enumerable, bar the §20.5 error slots), while an expando
//! entry carries its own W/E/C flags — which is what makes the
//! ctor-installed `err.cause` own-but-not-enumerable.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::member_get::{closure_props, recv_cell};
use crate::nanbox::{AnyValue, is_null, is_short_str, is_undefined};

use super::prop_has::{
    ARR_LEN_OFF, ARR_PROPS_OFF, STR_LEN_OFF, canonical_index, key_is, struct_has_own,
};

unsafe extern "C" {
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_get_flags(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_arr_index_flags(arr: *const c_void, idx: u64) -> u64;
}

/// `Object.prototype.propertyIsEnumerable` substrate (chunk D-1,
/// RFC 20260711): own AND enumerable. Mirrors
/// [`__torajs_any_prop_has`]'s dispatch with the enumerable-flag
/// filter applied where flags exist:
///
/// - DynObj / expando entries → packed-flags bit 1 (absent key
///   answers 0 flags — the miss and the non-enumerable case
///   coincide, matching §20.1.4.5).
/// - Arr / Str index keys and struct fields → enumerable when
///   present (tr data slots carry no non-enumerable state).
/// - `length` / `name` virtual props → 0 (spec non-enumerable).
/// - primitives → 0; null / undefined → catchable TypeError.
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_prop_enumerable(recv: AnyValue, key: *const c_void) -> i64 {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr());
        }
        return 0;
    }
    if is_short_str(recv) {
        let len = (recv >> 40) & 0xFF;
        return unsafe { str_index_enumerable(len, key) };
    }
    match recv_cell(recv) {
        Some((ptr, t)) if t == Tag::DynObj as u16 => {
            ((unsafe { __torajs_dynobj_get_flags(ptr, key) } & 0x2) != 0) as i64
        }
        Some((ptr, t)) if t == Tag::Arr as u16 => {
            let len = unsafe { ptr.cast::<u8>().add(ARR_LEN_OFF).cast::<u64>().read() };
            if let Some(i) = unsafe { canonical_index(key) }
                && i < len
            {
                // RFC 20260712-arr-exotic-define chunk C — a
                // defineProperty'd index carries shadow flags.
                return ((unsafe { __torajs_arr_index_flags(ptr, i) } & 0x2) != 0) as i64;
            }
            let props = unsafe {
                ptr.cast::<u8>()
                    .add(ARR_PROPS_OFF)
                    .cast::<*const c_void>()
                    .read()
            };
            if props.is_null() {
                0
            } else {
                ((unsafe { __torajs_dynobj_get_flags(props, key) } & 0x2) != 0) as i64
            }
        }
        Some((ptr, t)) if t == Tag::Closure as u16 => {
            let props = unsafe { closure_props(ptr) };
            if props.is_null() {
                0
            } else {
                ((unsafe { __torajs_dynobj_get_flags(props, key) } & 0x2) != 0) as i64
            }
        }
        Some((ptr, t)) if t == Tag::Obj as u16 => unsafe {
            // §20.5.6.1.1 msgDesc / the `stack` header line are both
            // `[[Enumerable]]: false`; every other struct FIELD keeps
            // the ordinary all-true attributes.
            if crate::member_get::header_flag(ptr, torajs_rc::FLAG_ERROR)
                && (key_is(key, b"message") || key_is(key, b"stack"))
            {
                0
            } else {
                // An expando entry carries its own attributes, so its
                // answer is the enumerable BIT, not presence — the
                // ctor-installed `cause` is own and non-enumerable
                // (§20.5.8.1), which presence alone reports as `true`.
                // Layout fields have no per-entry flags and fall
                // through to the presence probe as before.
                let props = crate::member_get_layout::struct_props(ptr);
                if !props.is_null() && __torajs_dynobj_has(props, key) != 0 {
                    ((__torajs_dynobj_get_flags(props, key) & 0x2) != 0) as i64
                } else {
                    struct_has_own(ptr, key)
                }
            }
        },
        Some((ptr, t)) if t == Tag::Str as u16 => {
            let len = unsafe { ptr.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() } as u64;
            unsafe { str_index_enumerable(len, key) }
        }
        // RFC 20260716 刀 20 — StringWrapper receiver mirror of
        // the刀 13 `__torajs_any_prop_has` arm above. `length` is
        // spec non-enumerable per §22.1.5.1; canonical indices
        // `[0, [[StringData]].length)` are enumerable. View-through
        // the inner Str cell to read the code-unit count.
        Some((ptr, t)) if t == Tag::StringWrapper as u16 => {
            let inner_ptr = unsafe { (ptr.cast::<u8>().add(8) as *const *const c_void).read() };
            let len = if inner_ptr.is_null() {
                0
            } else {
                unsafe { inner_ptr.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() as u64 }
            };
            unsafe { str_index_enumerable(len, key) }
        }
        _ => 0,
    }
}

/// Str-receiver enumerable arm: index chars are enumerable,
/// `length` is not (§22.1.5.1).
pub(crate) unsafe fn str_index_enumerable(len: u64, key: *const c_void) -> i64 {
    if let Some(i) = unsafe { canonical_index(key) }
        && i < len
    {
        return 1;
    }
    0
}
