//! `__torajs_regex_test` + `__torajs_regex_find` — port of
//! `runtime_regex.c` L2142-2180, L2292-2302.

use core::ffi::c_void;

use super::{as_regex_mut, byte_to_utf16_units, haystack, utf16_units_to_byte};
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
    let s = unsafe { haystack(re, str_ptr) };
    let slen = s.len() as i64;

    let sticky = re.flags & RE_FLAG_Y != 0;
    let global = re.flags & RE_FLAG_G != 0;
    let track = sticky || global;
    // lastIndex is spec'd in UTF-16 code units; the engine works in
    // transcoded UTF-8 bytes — map on read (and on write below).
    // `haystack` owns the transcode; the walk is identity-valued on
    // pure-ASCII bytes and dominated by that O(n) transcode.
    let start = if track {
        utf16_units_to_byte(&s, re.last_index_i64(), false)
    } else {
        0
    };

    // Phase C-3 — `re.test(s)` is single-shot from `start`; bind the
    // AOT-baked DFA view if any so the search short-circuits the
    // runtime `build_dfa`.
    // Round 3 Phase B sub-batch 7.2 — fall back to runtime-baked
    // `RegExp.dfa_runtime` when AOT path absent (ctor pre-builds for
    // every DFA-eligible literal).
    let dfa_view = re.baked_dfa_view();
    let dfa_ref = dfa_view.as_ref().or(re.dfa_runtime.as_ref());
    let hit_end = if track && start > slen {
        None
    } else if sticky {
        match_anchor(&re.prog, &s, start, re.flags).map(|m| m.end)
    } else {
        search_from(&re.prog, &s, start, re.flags, dfa_ref).map(|m| m.end)
    };

    match hit_end {
        None => {
            if track {
                re.set_last_index_num(0.0);
            }
            0
        }
        Some(end) => {
            if track {
                re.set_last_index_num(byte_to_utf16_units(&s, end, false) as f64);
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
    let s = unsafe { haystack(re, str_ptr) };
    let slen = s.len() as i64;
    let from = start.max(0);
    if from > slen {
        return -1;
    }
    // Phase C-3 — same baked-DFA short-circuit on the lower-level
    // `__torajs_regex_find` helper.
    // Round 3 Phase B sub-batch 7.2 — runtime-baked DFA fallback.
    let dfa_view = re.baked_dfa_view();
    let dfa_ref = dfa_view.as_ref().or(re.dfa_runtime.as_ref());
    match search_from(&re.prog, &s, from, re.flags, dfa_ref) {
        Some(m) => (m.start << 32) | (m.end & 0xffff_ffff),
        None => -1,
    }
}

/// `s.search(re)` — ES §22.1.3.19 via §22.2.6.12 Symbol.search:
/// the search always starts at 0 and `lastIndex` is saved/restored,
/// so this helper never reads or writes it (global / sticky flags
/// don't advance anything). Sticky anchors at 0; everything else is
/// a plain scan. Returns the match start in UTF-16 code units, or
/// `-1` on miss.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `str_ptr` is null or a
/// live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_search_regex(
    str_ptr: *const c_void,
    re_ptr: *const c_void,
) -> i64 {
    if re_ptr.is_null() || str_ptr.is_null() {
        return -1;
    }
    let re = unsafe { super::as_regex(re_ptr) };
    let s = unsafe { haystack(re, str_ptr) };
    let hit = if (re.flags & RE_FLAG_Y) != 0 {
        match_anchor(&re.prog, &s, 0, re.flags)
    } else {
        let dfa_view = re.baked_dfa_view();
        let dfa_ref = dfa_view.as_ref().or(re.dfa_runtime.as_ref());
        search_from(&re.prog, &s, 0, re.flags, dfa_ref)
    };
    match hit {
        Some(m) => byte_to_utf16_units(&s, m.start, false),
        None => -1,
    }
}
