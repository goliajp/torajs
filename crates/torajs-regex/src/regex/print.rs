//! `console.log(/regex/flags)` pretty-print — nested-print
//! substrate trunk Commit 6.
//!
//! Emits the bun `/source/flags` form unquoted (both top-level and
//! nested context, bun never quotes RegExp values). Source comes
//! from `RegExp::src_bytes` (the original pattern bytes preserved
//! by the parser); flags are decoded from `RegExp::flags` (the
//! u8 bitmask compiled by `parse_flags`) and emitted in the spec
//! order `gimsuy` (sorted-ascending — bun matches this).

use core::ffi::c_void;

use crate::parser::{
    RE_FLAG_D, RE_FLAG_G, RE_FLAG_I, RE_FLAG_M, RE_FLAG_S, RE_FLAG_U, RE_FLAG_V, RE_FLAG_Y,
};

use super::as_regex;

unsafe extern "C" {
    fn __torajs_io_putc_out(c: i32) -> i32;
}

#[inline]
unsafe fn put_byte(b: u8) {
    unsafe { __torajs_io_putc_out(b as i32) };
}

/// `console.log(re)` walker — emits `/source/flags` with no trailing
/// newline. Both top-level `__torajs_print_anyv`'s Tag::RegExp arm
/// and the nested `__torajs_print_anyv_inline` use this helper;
/// the top-level arm appends '\n' itself.
///
/// # Safety
///
/// `re_ptr` must point to a valid `RegExp` heap object (Tag::RegExp,
/// compiled by `__torajs_regex_compile`). The pattern's `src_bytes`
/// must hold valid UTF-8 (parser invariant).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_print_inline(re_ptr: *const c_void) {
    if re_ptr.is_null() {
        return;
    }
    unsafe {
        let re = as_regex(re_ptr);
        put_byte(b'/');
        for &b in re.src_bytes.iter() {
            put_byte(b);
        }
        put_byte(b'/');
        // Spec emission order: d, g, i, m, s, u, v, y (alphabetical).
        // Bun matches this; node v8 too (per ECMA-262 §22.2.6 flag
        // accessor order).
        let f = re.flags;
        if f & RE_FLAG_D != 0 {
            put_byte(b'd');
        }
        if f & RE_FLAG_G != 0 {
            put_byte(b'g');
        }
        if f & RE_FLAG_I != 0 {
            put_byte(b'i');
        }
        if f & RE_FLAG_M != 0 {
            put_byte(b'm');
        }
        if f & RE_FLAG_S != 0 {
            put_byte(b's');
        }
        if f & RE_FLAG_U != 0 {
            put_byte(b'u');
        }
        if f & RE_FLAG_V != 0 {
            put_byte(b'v');
        }
        if f & RE_FLAG_Y != 0 {
            put_byte(b'y');
        }
    }
}
