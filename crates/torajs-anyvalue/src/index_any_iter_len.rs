//! `__torajs_any_iter_len` — the for-of driver's live bound over an
//! `any` receiver (split from `index_any.rs`, whose question is
//! "what is `recv[i]`" — this one answers "how many steps does the
//! loop still have", re-asked per step because a typed array's
//! buffer can detach or resize mid-loop).

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::index_any::{
    MIRROR_ARR_LEN_OFF, MIRROR_FLAG_SUBSTR_INLINE, MIRROR_STR_LEN_OFF, MIRROR_SUBSTR_LEN_OFF,
    utf16_units_of_utf8,
};
use crate::nanbox::{AnyValue, as_void_ptr, is_cell, is_short_str, short_str_bytes, short_str_len};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_typedarray_validate(recv: AnyValue) -> i64;
}

/// Indexed-iteration bound where recv is an `any` value (RFC
/// 20260704 S5). Iterable receivers answer their element /
/// code-unit count; everything else raises a catchable TypeError
/// (ES §7.4.2 GetIterator on a non-iterable) and answers 0. Since
/// S5+ this is no longer emitted directly by ssa-lower — it serves
/// as the per-step live bound inside `__torajs_any_iter_next`
/// (sibling iter_any module).
///
/// Strings iterate per UTF-16 code unit here (the element read is
/// `recv[i]`) — astral code points split into surrogate halves,
/// a documented deviation from the spec's per-code-point string
/// iteration tracked in the RFC (typed `for..of` over `string` has
/// its own per-cp path; this is the `any`-erased fallback).
///
/// # Safety
/// Cell receivers must be valid heap pointers matching their header
/// tag layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_iter_len(recv: AnyValue) -> i64 {
    if is_short_str(recv) {
        let len = short_str_len(recv) as usize;
        let bytes = short_str_bytes(recv);
        let payload = &bytes[..len];
        if payload.iter().all(|b| *b < 0x80) {
            return len as i64;
        }
        return utf16_units_of_utf8(payload);
    }
    if is_cell(recv) {
        let ptr = as_void_ptr(recv);
        unsafe {
            let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
            if tag == Tag::Arr as u16 {
                return *(ptr.cast::<u8>().add(MIRROR_ARR_LEN_OFF) as *const u64) as i64;
            }
            // §23.2.5.1 — the Array Iterator's typed-array branch
            // asks ValidateTypedArray on EVERY step, not once at the
            // start, which is why detaching mid-loop throws here and
            // does nothing over an array. -1 carries that pending
            // throw out: the caller's `idx >= len` is then true and
            // the loop ends, leaving the throw for its own check.
            if tag == Tag::TypedArray as u16 {
                return __torajs_typedarray_validate(recv);
            }
            if tag == Tag::Str as u16 {
                let flags = (ptr.cast::<u8>().add(6) as *const u16).read();
                return if flags & MIRROR_FLAG_SUBSTR_INLINE != 0 {
                    *(ptr.cast::<u8>().add(MIRROR_SUBSTR_LEN_OFF) as *const u64) as i64
                } else {
                    *(ptr.cast::<u8>().add(MIRROR_STR_LEN_OFF) as *const u32) as i64
                };
            }
            // RFC 20260716 刀 12 — StringWrapper receiver: view-through
            // the [[StringData]] inner cell and delegate. Empty-str
            // wrapper (`new String()` no-arg or NULL sentinel) has no
            // inner cell — length = 0.
            if tag == Tag::StringWrapper as u16 {
                let inner_ptr = (ptr.cast::<u8>().add(8) as *const *const c_void).read();
                if inner_ptr.is_null() {
                    return 0;
                }
                let inner_tag = (inner_ptr.cast::<u8>().add(4) as *const u16).read();
                if inner_tag == Tag::Str as u16 {
                    let flags = (inner_ptr.cast::<u8>().add(6) as *const u16).read();
                    return if flags & MIRROR_FLAG_SUBSTR_INLINE != 0 {
                        *(inner_ptr.cast::<u8>().add(MIRROR_SUBSTR_LEN_OFF) as *const u64) as i64
                    } else {
                        *(inner_ptr.cast::<u8>().add(MIRROR_STR_LEN_OFF) as *const u32) as i64
                    };
                }
            }
        }
    }
    unsafe {
        __torajs_throw_type_error(c"value is not iterable".as_ptr());
    }
    0
}
