//! `__torajs_str_match_all_regex` — port of `runtime_regex.c`
//! L2992-3059.
//!
//! Returns `Array<Array<Str>>` — array of exec-shape arrays for
//! each non-overlapping match. Per JS spec §22.1.3.13 throws
//! TypeError when called on a non-`g` regex (P9.4 follow-up
//! enforced here too).

use core::ffi::c_void;

use super::{
    __torajs_arr_alloc, __torajs_arr_push, __torajs_throw_type_error, abort_unsupported, as_regex,
    str_from_bytes, str_slice,
};
use crate::node::{REGEX_MAX_CAPTURES, REGEX_SAVE_SLOTS};
use crate::parser::{RE_FLAG_G, RE_FLAG_Y};
use crate::vm::{Workspace, match_anchor, search_from_with_ws};

/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `str_ptr` is null or a
/// live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_match_all_regex(
    str_ptr: *const c_void,
    re_ptr: *const c_void,
) -> *mut c_void {
    let outer = unsafe { __torajs_arr_alloc(0) };
    if re_ptr.is_null() || str_ptr.is_null() {
        return outer;
    }
    let re = unsafe { as_regex(re_ptr) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    if re.flags & RE_FLAG_G == 0 {
        unsafe {
            __torajs_throw_type_error(
                b"String.prototype.matchAll called with a non-global RegExp argument\0".as_ptr(),
            );
        }
        return outer;
    }
    let s = unsafe { str_slice(str_ptr) };
    let slen = s.len() as i64;

    // Lazy-init Workspace — sticky path uses match_anchor's own
    // Workspace; the outer ws is only needed for the non-sticky
    // search_from_with_ws branch. Sticky callers skip the ~50KB alloc.
    let mut ws: Option<Workspace> = None;
    let sticky = re.flags & RE_FLAG_Y != 0;
    // Phase C-3 — bind the AOT-baked DFA view once outside the loop.
    // Round 3 Phase B sub-batch 7.2 — fall back to runtime-baked
    // `RegExp.dfa_runtime` (eager-built once at
    // `__torajs_regex_compile` ctor for every DFA-eligible literal).
    // The earlier `None` fall-through to `search_from_with_ws`'s
    // inline per-call `build_dfa` becomes dead in sub-batch 7.3; AOT
    // regexes still short-circuit straight to the `.rodata`-baked DFA.
    let dfa_view = re.baked_dfa_view();
    let dfa_ref = dfa_view.as_ref().or(re.dfa_runtime.as_ref());

    let mut outer = outer;
    let mut pos: i64 = 0;
    while pos <= slen {
        let hit = if sticky {
            match_anchor(&re.prog, &s, pos, re.flags)
        } else {
            let ws_ref = ws.get_or_insert_with(|| Workspace::for_program(&re.prog));
            // Round 3 Phase B attack #R-A1 — match_all currently
            // routes through `str_slice` (always transcodes to a
            // freshly-allocated Vec<u8>), so the ASCII-view shortcut
            // isn't on this path. Pass `false`; the u-flag
            // continuation-byte gate stays correct for any haystack.
            search_from_with_ws(&re.prog, &s, pos, re.flags, ws_ref, dfa_ref, false)
        };
        let Some(m) = hit else { break };
        outer = unsafe { append_inner(outer, re, &s, m.saves(), m.start, m.end) };
        pos = if m.end == m.start { m.end + 1 } else { m.end };
    }
    outer
}

/// Build the exec-shape inner array `[match, g1, g2, ...]` and
/// push it onto `outer`.
unsafe fn append_inner(
    outer: *mut c_void,
    re: &super::RegExp,
    s: &[u8],
    saves: &[i64; REGEX_SAVE_SLOTS],
    st: i64,
    en: i64,
) -> *mut c_void {
    let mut inner = unsafe { __torajs_arr_alloc(0) };
    let whole = unsafe { str_from_bytes(&s[st as usize..en as usize]) };
    inner = unsafe { __torajs_arr_push(inner, whole as i64) };
    let n_cap_lim = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
    for i in 1..=n_cap_lim {
        let gs = saves[2 * i];
        let ge = saves[2 * i + 1];
        if gs < 0 || ge < 0 {
            inner = unsafe { __torajs_arr_push(inner, 0) };
        } else {
            let grp = unsafe { str_from_bytes(&s[gs as usize..ge as usize]) };
            inner = unsafe { __torajs_arr_push(inner, grp as i64) };
        }
    }
    unsafe { __torajs_arr_push(outer, inner as i64) }
}
