//! The string value of one exec-record span — `m[k]` when the match
//! result is held as spans rather than as an array (torajs-core
//! `ssa_lower_exec_record`). A fresh Str of the subject's code units
//! where the group participated, the immortal Str `undefined` where
//! it did not: the two values a match array's element holds, so every
//! consumer of the read sees what it saw with the array.

use core::ffi::c_void;

use crate::slice::__torajs_str_slice;
use crate::undef_sentinel::undef_ptr;

/// A fresh Str of `subject[start..end)` (code units), or the Str
/// `undefined` sentinel when `start` is negative — the spelling
/// `__torajs_regex_exec_span` gives a group that did not participate.
///
/// # Safety
///
/// `subject` is a live Str heap pointer and `0 <= start <= end <=
/// subject.length` whenever `start` is not negative.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_group_span(
    subject: *const u8,
    start: i64,
    end: i64,
) -> *mut c_void {
    if start < 0 {
        return undef_ptr() as *mut c_void;
    }
    unsafe { __torajs_str_slice(subject, start, end) as *mut c_void }
}
