//! `__torajs_str_split_regex` — port of `runtime_regex.c`
//! L2861-2911.

use core::ffi::c_void;

use super::{
    __torajs_arr_alloc, __torajs_arr_push, abort_unsupported, as_regex, str_from_bytes, str_slice,
};
use crate::parser::RE_FLAG_Y;
use crate::vm::{Workspace, match_anchor, search_from_with_ws};

/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `str_ptr` is null or a
/// live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_split_regex(
    str_ptr: *const c_void,
    re_ptr: *const c_void,
) -> *mut c_void {
    let out = unsafe { __torajs_arr_alloc(0) };
    if re_ptr.is_null() || str_ptr.is_null() {
        return out;
    }
    let re = unsafe { as_regex(re_ptr) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    let s = unsafe { str_slice(str_ptr) };
    let slen = s.len() as i64;

    let mut ws = Workspace::for_program(&re.prog);
    let sticky = re.flags & RE_FLAG_Y != 0;

    let mut out = out;
    let mut pos: i64 = 0;
    while pos <= slen {
        let m = if sticky {
            match_anchor(&re.prog, s, pos, re.flags)
        } else {
            search_from_with_ws(&re.prog, s, pos, re.flags, &mut ws)
        };
        let Some(m) = m else { break };
        if m.end == m.start {
            // Empty separator — JS: "ab".split(//) → ["a","b"].
            // Take one byte, push, advance.
            if m.start >= slen {
                break;
            }
            let seg = unsafe { str_from_bytes(&s[pos as usize..m.start as usize]) };
            out = unsafe { __torajs_arr_push(out, seg as i64) };
            pos = m.end + 1;
            continue;
        }
        let seg = unsafe { str_from_bytes(&s[pos as usize..m.start as usize]) };
        out = unsafe { __torajs_arr_push(out, seg as i64) };
        pos = m.end;
    }
    // Append final tail.
    if pos <= slen {
        let seg = unsafe { str_from_bytes(&s[pos as usize..]) };
        out = unsafe { __torajs_arr_push(out, seg as i64) };
    }
    out
}
