//! `s.match(re)` held as the first match's UTF-16 length — the kernel
//! of the match-length lane (`torajs-core` `ast::regex_match_len_only`,
//! which lists the results a program only null-tests or asks
//! `m[0].length`). The walk and every lastIndex effect are
//! [`super::match_op::match_walk`]'s, shared with
//! `__torajs_str_match_regex`; what is left out is the array, the
//! `m[0]` string and their release.

use core::ffi::c_void;

use super::match_op::match_walk;
use super::{as_regex_mut, byte_to_utf16_units, str_slice_ascii_view};
use crate::parser::{RE_FLAG_G, RE_FLAG_Y};
use crate::search_anchors::find_literal;
use crate::vm::search::dfa_find_from;

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
    if let Some(len) = unsafe { plain_len(str_ptr, re_ptr) } {
        return len;
    }
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

/// The walk's answer for its most common shape, reached without the
/// walk: no `g` / `y` (so lastIndex is neither read nor written and a
/// single search from 0 decides) over an ASCII haystack (bytes are
/// UTF-16 units, and no match start can land on a continuation byte),
/// searched the way the walk's search would: a literal program by
/// memchr, any other by the DFA bound to the RegExp. `None` hands the
/// call to the walk.
///
/// # Safety
///
/// As [`__torajs_str_match_len`].
#[inline(always)]
unsafe fn plain_len(str_ptr: *const c_void, re_ptr: *const c_void) -> Option<i64> {
    if re_ptr.is_null() || str_ptr.is_null() {
        return None;
    }
    let re = unsafe { as_regex_mut(re_ptr as *mut c_void) };
    if re.rejected != 0 || re.flags & (RE_FLAG_G | RE_FLAG_Y) != 0 {
        return None;
    }
    let s = unsafe { str_slice_ascii_view(str_ptr) }?;
    let span = match re.prog.literal.as_deref() {
        Some(lit) => find_literal(s, 0, lit).map(|m| (m.start, m.end)),
        None => {
            let dfa_view = re.baked_dfa_view();
            let dfa = dfa_view.as_ref().or(re.dfa_runtime.as_ref())?;
            dfa_find_from(&re.prog, dfa, s, 0, re.flags, true)
        }
    };
    Some(span.map_or(-1, |(start, end)| end - start))
}
