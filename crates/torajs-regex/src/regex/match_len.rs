//! `s.match(re)` held as the first match's UTF-16 length — the kernel
//! of the match-length lane (`torajs-core` `ast::regex_match_len_only`,
//! which lists the results a program only null-tests or asks
//! `m[0].length`). The walk and every lastIndex effect are
//! [`super::match_op::match_walk`]'s, shared with
//! `__torajs_str_match_regex`; what is left out is the array, the
//! `m[0]` string and their release.

use core::ffi::c_void;

use super::byte_to_utf16_units;
use super::match_op::match_walk;

/// The first match's length in UTF-16 code units, or -1 when
/// `s.match(re)` answers null. A global match stops at its first hit:
/// the later ones only ever filled the array, and the walk still ends
/// on `lastIndex = 0`.
///
/// # Safety
///
/// `str_ptr` is null or a live `*Str`; `re_ptr` is null or a live
/// `*RegExp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_match_len(
    str_ptr: *const c_void,
    re_ptr: *const c_void,
) -> i64 {
    let mut len: i64 = -1;
    unsafe {
        match_walk(str_ptr, re_ptr, |_, s, is_ascii, m| {
            len =
                byte_to_utf16_units(s, m.end, is_ascii) - byte_to_utf16_units(s, m.start, is_ascii);
            false
        });
    }
    len
}
