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

    // Lazy-init Workspace — sticky branch uses match_anchor's own
    // Workspace; outer ws only needed in non-sticky branch.
    let mut ws: Option<Workspace> = None;
    let sticky = re.flags & RE_FLAG_Y != 0;
    // Phase C-3 — bind AOT-baked DFA view once. See match_all for the
    // same pattern and the rationale (zero `dfa_cache` reads on the
    // hot search loop once Phase C-4+ delivers baked entries).
    let dfa_view = re.baked_dfa_view();

    let mut out = out;
    let mut pos: i64 = 0;
    while pos <= slen {
        let m = if sticky {
            match_anchor(&re.prog, &s, pos, re.flags)
        } else {
            let ws_ref = ws.get_or_insert_with(|| Workspace::for_program(&re.prog));
            search_from_with_ws(&re.prog, &s, pos, re.flags, ws_ref, dfa_view.as_ref())
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
