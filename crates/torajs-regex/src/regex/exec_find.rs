//! §22.2.7.2 RegExpBuiltinExec up to the match itself: the argument
//! checks, the haystack view, the lastIndex read under `g` / `y`, the
//! one search, and the lastIndex write on a hit and on a miss. The two
//! exec kernels share it — `__torajs_regex_exec` builds the match
//! array from the hit, `__torajs_regex_exec_span` writes its spans —
//! so a program that takes either lane sees the same lastIndex
//! effects and the same frozen-lastIndex TypeError.

use alloc::vec::Vec;
use core::ffi::c_void;

use super::{
    MatcherFn, RegExp, abort_unsupported, as_regex_mut, byte_to_utf16_units, haystack,
    str_slice_ascii_view, utf16_units_to_byte,
};
use crate::parser::{RE_FLAG_G, RE_FLAG_Y};
use crate::vm::result::INLINE_SLOTS;
use crate::vm::{MatchResult, match_anchor, search_from_with_ws};

/// Run one exec search and hand the hit to `on_hit(re, haystack,
/// haystack_is_ascii, m)`. `None` for a null argument, a refused
/// lastIndex store (the TypeError is already recorded) and a miss;
/// lastIndex is updated exactly as the spec's exec does in every case.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `str_ptr` is null or a
/// live `*Str`.
pub(crate) unsafe fn exec_find<R>(
    re_ptr: *const c_void,
    str_ptr: *const c_void,
    on_hit: impl FnOnce(&RegExp, &[u8], bool, &MatchResult) -> R,
) -> Option<R> {
    if re_ptr.is_null() || str_ptr.is_null() {
        return None;
    }
    let re = unsafe { as_regex_mut(re_ptr as *mut c_void) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    if let Some(f) = re.matcher_latin1
        && let Some(s) = unsafe { str_slice_ascii_view(str_ptr) }
    {
        return unsafe { exec_matcher(re, s, f, on_hit) };
    }
    // zero-copy ASCII Latin-1 view; transcode fallback for the rest
    let _s_owned: Vec<u8>;
    let (s, haystack_is_ascii): (&[u8], bool) = match unsafe { str_slice_ascii_view(str_ptr) } {
        Some(view) => (view, true),
        None => {
            _s_owned = unsafe { haystack(re, str_ptr) };
            (&_s_owned, false)
        }
    };
    let slen = s.len() as i64;

    let sticky = re.flags & RE_FLAG_Y != 0;
    let global = re.flags & RE_FLAG_G != 0;
    let track = sticky || global;
    if track && unsafe { super::lifecycle::tracked_store_refuses(re) } {
        return None;
    }
    // lastIndex is spec'd in UTF-16 code units (§22.2.7.2 step 4);
    // map to a byte offset in the transcoded haystack. Out-of-range
    // maps to slen + 1 so the `start > slen` guard below fires.
    let start = if track {
        utf16_units_to_byte(s, re.last_index_i64(), haystack_is_ascii)
    } else {
        0
    };

    let dfa_view = re.baked_dfa_view();
    let dfa_ref = dfa_view.as_ref().or(re.dfa_runtime.as_ref());
    let m = if track && start > slen {
        None
    } else if sticky {
        match_anchor(&re.prog, s, start, re.flags)
    } else {
        // the per-RegExp workspace: a capturing pattern's second pass
        // reuses its thread lists and arena instead of building and
        // freeing them on every call
        let ws = unsafe { &mut *re.workspace_cache.get() };
        search_from_with_ws(
            &re.prog,
            s,
            start,
            re.flags,
            ws,
            dfa_ref,
            haystack_is_ascii,
            true,
        )
    };
    let Some(m) = m else {
        if track {
            re.set_last_index_num(0.0);
        }
        return None;
    };
    if track {
        re.set_last_index_num(byte_to_utf16_units(s, m.end, haystack_is_ascii) as f64);
    }
    Some(on_hit(re, s, haystack_is_ascii, &m))
}

/// The search through the literal's own matcher over the string's
/// ASCII payload: offsets are code units already, and the row it
/// fills is the Pike VM's, so `on_hit` sees the same `MatchResult`.
/// The pattern carries no `y` (the lane declines sticky literals), so
/// `g` alone decides the lastIndex flow.
///
/// # Safety
///
/// `s` is the payload `f` was synthesized for.
unsafe fn exec_matcher<R>(
    re: &mut RegExp,
    s: &[u8],
    f: MatcherFn,
    on_hit: impl FnOnce(&RegExp, &[u8], bool, &MatchResult) -> R,
) -> Option<R> {
    let global = re.flags & RE_FLAG_G != 0;
    if global && unsafe { super::lifecycle::tracked_store_refuses(re) } {
        return None;
    }
    let slen = s.len() as i64;
    let start = if global { re.last_index_i64() } else { 0 };
    let stride = re.matcher_stride as usize;
    let m = if start > slen {
        None
    } else if stride <= INLINE_SLOTS {
        let mut row = [-1i64; INLINE_SLOTS];
        let end = unsafe { f(s.as_ptr(), slen, start, row.as_mut_ptr()) };
        (end >= 0).then(|| MatchResult::with_saves_row(row[0], end, &row[..stride]))
    } else {
        let mut row = alloc::vec![-1i64; stride];
        let end = unsafe { f(s.as_ptr(), slen, start, row.as_mut_ptr()) };
        (end >= 0).then(|| MatchResult::with_saves(row[0], end, row))
    };
    let Some(m) = m else {
        if global {
            re.set_last_index_num(0.0);
        }
        return None;
    };
    if global {
        re.set_last_index_num(m.end as f64);
    }
    Some(on_hit(re, s, true, &m))
}
