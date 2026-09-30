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
    RegExp, abort_unsupported, as_regex_mut, byte_to_utf16_units, haystack, str_slice_ascii_view,
    utf16_units_to_byte,
};
use crate::parser::{RE_FLAG_G, RE_FLAG_Y};
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
