//! §22.2.7.2 RegExpBuiltinExec up to the match itself: the argument
//! checks, the haystack view, the lastIndex read under `g` / `y`, the
//! one search, and the lastIndex write on a hit and on a miss. The two
//! exec kernels share it — `__torajs_regex_exec` builds the match
//! array from the hit, `__torajs_regex_exec_span` writes its spans —
//! so a program that takes either lane sees the same lastIndex
//! effects and the same frozen-lastIndex TypeError.
//!
//! A literal with a compiled matcher for the string's encoding runs
//! it over the string's own code units, whatever they are; every
//! other pattern, and every `new RegExp`, runs the engine over the
//! ASCII payload in place or a transcoded copy.

use alloc::vec::Vec;
use core::ffi::c_void;

use super::str_helpers::subject::{Subject, UnitsView, str_units_view};
use super::{
    MatcherFn, RegExp, abort_unsupported, as_regex_mut, byte_to_utf16_units, haystack,
    str_slice_ascii_view, utf16_units_to_byte,
};
use crate::flags::unicode_mode;
use crate::onepass::ir::{is_high_surrogate, is_low_surrogate};
use crate::parser::{RE_FLAG_G, RE_FLAG_Y};
use crate::vm::result::INLINE_SLOTS;
use crate::vm::{MatchResult, match_anchor, search_from_with_ws};

/// Run one exec search and hand the hit to `on_hit(re, subject, m)`.
/// `None` for a null argument, a refused lastIndex store (the
/// TypeError is already recorded) and a miss; lastIndex is updated
/// exactly as the spec's exec does in every case.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `str_ptr` is null or a
/// live `*Str`.
pub(crate) unsafe fn exec_find<R>(
    re_ptr: *const c_void,
    str_ptr: *const c_void,
    on_hit: impl FnOnce(&RegExp, &Subject, &MatchResult) -> R,
) -> Option<R> {
    if re_ptr.is_null() || str_ptr.is_null() {
        return None;
    }
    let re = unsafe { as_regex_mut(re_ptr as *mut c_void) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    let view = unsafe { str_units_view(str_ptr) };
    if let Some(f) = matcher_for(re, &view) {
        return unsafe { exec_matcher(re, str_ptr, &view, f, on_hit) };
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
    Some(on_hit(
        re,
        &Subject::bytes(str_ptr, s, haystack_is_ascii),
        &m,
    ))
}

/// The matcher the compiler synthesized for `re` over the encoding of
/// `view`, if it has one.
#[inline]
pub(crate) fn matcher_for(re: &RegExp, view: &UnitsView<'_>) -> Option<MatcherFn> {
    if view.wide() {
        re.matcher_utf16
    } else {
        re.matcher_latin1
    }
}

/// One matcher search from `start` over `view`, as the Pike VM's
/// row: the whole match in the result, the groups in its saves.
///
/// # Safety
///
/// `f` was synthesized for the encoding of `view`; `0 <= start <=
/// view.len()`.
#[inline(always)]
pub(crate) unsafe fn run_matcher(
    f: MatcherFn,
    stride: usize,
    view: &UnitsView<'_>,
    start: i64,
) -> Option<MatchResult> {
    let len = view.len() as i64;
    if stride <= INLINE_SLOTS {
        let mut row = [-1i64; INLINE_SLOTS];
        let end = unsafe { f(view.as_ptr(), len, start, row.as_mut_ptr()) };
        (end >= 0).then(|| MatchResult::with_saves_row(row[0], end, &row[..stride]))
    } else {
        let mut row = alloc::vec![-1i64; stride];
        let end = unsafe { f(view.as_ptr(), len, start, row.as_mut_ptr()) };
        (end >= 0).then(|| MatchResult::with_saves(row[0], end, row))
    }
}

/// Where a search from unit `start` begins under `flags`: the pair's
/// first unit when `start` names the low half of a surrogate pair in
/// `u` / `v` mode — §22.2.7.2 step 12.b takes the character element
/// `lastIndex` was obtained from, which is the whole pair.
#[inline]
pub(crate) fn search_start(view: &UnitsView<'_>, flags: u8, start: i64) -> i64 {
    if view.wide()
        && unicode_mode(flags)
        && start > 0
        && (start as usize) < view.len()
        && is_high_surrogate(view.unit(start as usize - 1))
        && is_low_surrogate(view.unit(start as usize))
    {
        start - 1
    } else {
        start
    }
}

/// The search through the literal's own matcher over the string's
/// payload: offsets are code units already, and the row it fills is
/// the Pike VM's, so `on_hit` sees the same `MatchResult`. `g` and
/// `y` decide the lastIndex flow; the anchoring a sticky pattern
/// needs is compiled into its matcher.
///
/// # Safety
///
/// `view` is the payload of `str_ptr` and `f` was synthesized for its
/// encoding.
unsafe fn exec_matcher<R>(
    re: &mut RegExp,
    str_ptr: *const c_void,
    view: &UnitsView<'_>,
    f: MatcherFn,
    on_hit: impl FnOnce(&RegExp, &Subject, &MatchResult) -> R,
) -> Option<R> {
    let track = re.flags & (RE_FLAG_G | RE_FLAG_Y) != 0;
    if track && unsafe { super::lifecycle::tracked_store_refuses(re) } {
        return None;
    }
    let len = view.len() as i64;
    let start = if track { re.last_index_i64() } else { 0 };
    let m = if start > len {
        None
    } else {
        let from = search_start(view, re.flags, start);
        unsafe { run_matcher(f, re.matcher_stride as usize, view, from) }
    };
    let Some(m) = m else {
        if track {
            re.set_last_index_num(0.0);
        }
        return None;
    };
    if track {
        re.set_last_index_num(m.end as f64);
    }
    Some(on_hit(re, &Subject::units(str_ptr, view.len()), &m))
}
