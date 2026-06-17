//! `console.log(arr)` no-\n typed walker family — multi-arg joiner
//! substrate for ssa-lower's `lower_top_stmt` multi-arg path.
//!
//! Mirror of [`crate::print`]'s 5-typed-walker family
//! (`__torajs_arr_print_{i64,f64,bool,str,substr}`) but each arm
//! emits its bytes **without** the trailing `'\n'` so the multi-arg
//! console.log joiner can splice them with `' '` separators and a
//! single final `'\n'`.
//!
//! Output shape (matches bun for these element types):
//! - `null` for NULL arr (defensive — regex no-match doesn't reach
//!   here, but stays consistent with the standalone family)
//! - `[]` for empty arr
//! - `[ a, b, c ]` for non-empty
//!
//! Pure no-\n mirror — same per-element format, same `put_arrprops`
//! tail (regex `.index` / `.input` etc), only the `b" ]\n"` suffix
//! drops the `'\n'`.

use core::ffi::c_void;

use crate::print::{
    HDR_FLAGS_OFF, STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF, SUBSTR_LEN_OFF,
    SUBSTR_OFFSET_OFF, SUBSTR_PARENT_OFF, print_header_inline, put_byte, put_bytes,
    put_close_bracket_inline, put_sep, put_snprintf_f64_g, put_snprintf_i64, put_str_payload,
    slot_addr,
};

/// `console.log(arr: Array<I64>)` inline (no-\n) variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_i64_inline(arr: *const c_void) {
    let arr = arr as *const u8;
    unsafe {
        let Some((head, len)) = print_header_inline(arr) else {
            return;
        };
        for i in 0..len {
            put_sep(i);
            let v = *(slot_addr(arr, head, i) as *const i64);
            put_snprintf_i64(v);
        }
        crate::print_props::put_arrprops(arr as *mut c_void);
        put_close_bracket_inline();
    }
}

/// `console.log(arr: Array<F64>)` inline (no-\n) variant. JS-spec
/// NaN / Infinity / -Infinity literals, else `%g` shortest-roundtrip
/// per parent family.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_f64_inline(arr: *const c_void) {
    let arr = arr as *const u8;
    unsafe {
        let Some((head, len)) = print_header_inline(arr) else {
            return;
        };
        for i in 0..len {
            put_sep(i);
            let v = *(slot_addr(arr, head, i) as *const f64);
            if v.is_nan() {
                put_bytes(b"NaN");
            } else if v == f64::INFINITY {
                put_bytes(b"Infinity");
            } else if v == f64::NEG_INFINITY {
                put_bytes(b"-Infinity");
            } else {
                put_snprintf_f64_g(v);
            }
        }
        crate::print_props::put_arrprops(arr as *mut c_void);
        put_close_bracket_inline();
    }
}

/// `console.log(arr: Array<Bool>)` inline (no-\n) variant. Slots are
/// i64 (0 / non-0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_bool_inline(arr: *const c_void) {
    let arr = arr as *const u8;
    unsafe {
        let Some((head, len)) = print_header_inline(arr) else {
            return;
        };
        for i in 0..len {
            put_sep(i);
            let v = *(slot_addr(arr, head, i) as *const i64);
            put_bytes(if v != 0 { b"true" } else { b"false" });
        }
        crate::print_props::put_arrprops(arr as *mut c_void);
        put_close_bracket_inline();
    }
}

/// `console.log(arr: Array<Str>)` inline (no-\n) variant. Each slot
/// is a `*Str` (NULL → `undefined`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_str_inline(arr: *const c_void) {
    let arr = arr as *const u8;
    unsafe {
        let Some((head, len)) = print_header_inline(arr) else {
            return;
        };
        for i in 0..len {
            put_sep(i);
            let s = *(slot_addr(arr, head, i) as *const *const u8);
            if s.is_null() {
                put_bytes(b"undefined");
                continue;
            }
            let length = *(s.add(STR_LEN_OFF) as *const u32);
            let flags = *(s.add(HDR_FLAGS_OFF) as *const u16);
            let is_latin1 = (flags & STR_FLAG_IS_LATIN1) != 0;
            let byte_cnt = if is_latin1 {
                length as usize
            } else {
                (length as usize) * 2
            };
            put_byte(b'"');
            if byte_cnt > 0 {
                let bytes = core::slice::from_raw_parts(s.add(STR_DATA_OFF), byte_cnt);
                put_str_payload(bytes, is_latin1);
            }
            put_byte(b'"');
        }
        crate::print_props::put_arrprops(arr as *mut c_void);
        put_close_bracket_inline();
    }
}

/// `console.log(arr: Array<Substr>)` inline (no-\n) variant. Each
/// slot is a `*Substr` — layout differs from Str (parent_ptr +
/// offset instead of inline bytes).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_substr_inline(arr: *const c_void) {
    let arr = arr as *const u8;
    unsafe {
        let Some((head, len)) = print_header_inline(arr) else {
            return;
        };
        for i in 0..len {
            put_sep(i);
            let v = *(slot_addr(arr, head, i) as *const *const u8);
            if v.is_null() {
                put_bytes(b"undefined");
                continue;
            }
            let cu_len = *(v.add(SUBSTR_LEN_OFF) as *const u64) as usize;
            let parent = *(v.add(SUBSTR_PARENT_OFF) as *const *const u8);
            let cu_offset = *(v.add(SUBSTR_OFFSET_OFF) as *const u64) as usize;
            put_byte(b'"');
            if cu_len > 0 {
                let flags = *(parent.add(HDR_FLAGS_OFF) as *const u16);
                let is_latin1 = (flags & STR_FLAG_IS_LATIN1) != 0;
                let stride = if is_latin1 { 1 } else { 2 };
                let bytes = core::slice::from_raw_parts(
                    parent.add(STR_DATA_OFF + cu_offset * stride),
                    cu_len * stride,
                );
                put_str_payload(bytes, is_latin1);
            }
            put_byte(b'"');
        }
        crate::print_props::put_arrprops(arr as *mut c_void);
        put_close_bracket_inline();
    }
}
