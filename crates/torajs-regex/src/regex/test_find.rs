//! `__torajs_regex_test` + `__torajs_regex_find` — port of
//! `runtime_regex.c` L2142-2180, L2292-2302.

use core::ffi::c_void;

use super::{as_regex_mut, str_slice};
use crate::parser::{RE_FLAG_G, RE_FLAG_Y};
use crate::vm::{match_anchor, search_from};

/// `re.test(s)` — per ES spec §22.2.5.2 == `(exec(s) !== null)`.
/// Sticky / global lastIndex bookkeeping matches exec.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `str_ptr` is null or a
/// live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_test(re_ptr: *const c_void, str_ptr: *const c_void) -> i64 {
    if re_ptr.is_null() {
        return 0;
    }
    let re = unsafe { as_regex_mut(re_ptr as *mut c_void) };
    let s = unsafe { str_slice(str_ptr) };
    let slen = s.len() as i64;

    let sticky = re.flags & RE_FLAG_Y != 0;
    let global = re.flags & RE_FLAG_G != 0;
    let track = sticky || global;
    let mut start = if track { re.last_index } else { 0 };
    if start < 0 {
        start = 0;
    }

    let hit_end = if track && start > slen {
        None
    } else if sticky {
        match_anchor(&re.prog, s, start, re.flags).map(|m| m.end)
    } else {
        search_from(&re.prog, s, start, re.flags).map(|m| m.end)
    };

    match hit_end {
        None => {
            if track {
                re.last_index = 0;
            }
            0
        }
        Some(end) => {
            if track {
                re.last_index = end;
            }
            1
        }
    }
}

/// `__torajs_regex_find` — ssa_lower-emitted helper that returns a
/// packed `(start << 32) | (end & 0xffffffff)` (sentinel `-1` for
/// no match). Reserved for raw position consumers — current surface
/// methods use the higher-level helpers directly.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `str_ptr` is null or a
/// live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_find(
    re_ptr: *const c_void,
    str_ptr: *const c_void,
    start: i64,
) -> i64 {
    if re_ptr.is_null() {
        return -1;
    }
    let re = unsafe { super::as_regex(re_ptr) };
    let s = unsafe { str_slice(str_ptr) };
    let slen = s.len() as i64;
    let from = start.max(0);
    if from > slen {
        return -1;
    }
    match search_from(&re.prog, s, from, re.flags) {
        Some(m) => (m.start << 32) | (m.end & 0xffff_ffff),
        None => -1,
    }
}
