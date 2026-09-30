//! The match walk behind the replace-callback kernels: find every
//! match the pattern's flags ask for, hand each to the callback, and
//! splice what it returns over the matched span.
//!
//! Three texts are walked. An ASCII subject is searched in place and
//! the result built as bytes. A subject past ASCII under a literal
//! with a compiled matcher for its encoding is walked as its own code
//! units ([`super::replace_fn_units`]). Every other subject is
//! transcoded once for the engine.

use alloc::vec::Vec;
use core::ffi::c_void;

use super::exec_find::{matcher_for, run_matcher};
use super::replace_fn_units::UnitsWalk;
use super::str_helpers::subject::{Subject, UnitsView, str_units_view};
use super::{
    __torajs_str_drop, RegExp, abort_unsupported, as_regex_mut, byte_to_utf16_units, haystack,
    str_from_bytes, str_from_bytes_ascii, str_slice_ascii_view, str_units, utf16_units_to_byte,
};
use crate::parser::RE_FLAG_Y;
use crate::vm::{match_anchor, search_from_with_ws};

/// Shared match walk — find/anchor per the pattern's flags, hand
/// each match to `invoke`, splice its returned Str over the matched
/// span. `invoke` receives `(match_str, saves, subject, offset_cu)`
/// borrowed (the walk drops `match_str` after the call) and answers
/// the freshly-owned replacement Str; `Err(())` means the callback
/// raised a pending throw — the walk aborts and hands back a
/// placeholder copy the caller's throw check discards. The two
/// invoke strategies differ only in dispatch: the typed lanes
/// transmute to the callback's statically-declared arity, the boxed
/// lane rides the closure's boxed entry.
///
/// An ASCII subject is searched in place — through the literal's
/// compiled matcher when it has one, whose units are these bytes —
/// and the result is built as bytes. Any other subject of a literal
/// with a matcher for its encoding is walked as its own code units
/// ([`UnitsWalk`]); the rest are transcoded once for the engine and
/// built back the same way.
pub(super) unsafe fn replace_fn_inner(
    str_ptr: *const c_void,
    re_ptr: *const c_void,
    global: bool,
    mut invoke: impl FnMut(*mut c_void, &[i64], &Subject, i64) -> Result<*mut c_void, ()>,
) -> *mut c_void {
    if re_ptr.is_null() {
        let s = unsafe { str_units(str_ptr) };
        return unsafe { str_from_bytes(&s) as *mut c_void };
    }
    let re = unsafe { as_regex_mut(re_ptr as *mut c_void) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    // §22.2.6.11 step 10 — a global replace starts with
    // `Set(rx, "lastIndex", 0, true)`, which a non-writable lastIndex
    // refuses (the input comes back for the caller's throw check); the
    // walk's last exec misses, so 0 is also where lastIndex ends.
    if global && !unsafe { super::lifecycle::global_replace_start(re) } {
        let s = unsafe { str_units(str_ptr) };
        return unsafe { str_from_bytes(&s) as *mut c_void };
    }
    let sticky = re.flags & RE_FLAG_Y != 0;
    // without `g` the walk is one RegExpBuiltinExec (§22.2.7.2): a
    // sticky pattern anchors at lastIndex and stores the match end,
    // or 0 on a miss — a store a read-only lastIndex refuses up front
    let sticky_once = sticky && !global;
    if sticky_once && unsafe { super::lifecycle::tracked_store_refuses(re) } {
        let s = unsafe { str_units(str_ptr) };
        return unsafe { str_from_bytes(&s) as *mut c_void };
    }
    let re_cell = re_ptr as *mut RegExp;
    let re: &RegExp = re;
    // an ASCII subject is matched in place; any other goes through the
    // literal's matcher over its own code units, or transcodes once
    let s_owned: Vec<u8>;
    let (s, is_ascii): (&[u8], bool) = match unsafe { str_slice_ascii_view(str_ptr) } {
        Some(view) => (view, true),
        None => {
            let view = unsafe { str_units_view(str_ptr) };
            if let Some(f) = matcher_for(re, &view) {
                let walk = UnitsWalk {
                    str_ptr,
                    re,
                    re_cell,
                    view,
                    matcher: f,
                    global,
                    sticky_once,
                };
                return unsafe { walk.run(invoke) };
            }
            s_owned = unsafe { haystack(re, str_ptr) };
            (&s_owned, false)
        }
    };
    let slen = s.len() as i64;
    let subject = Subject::bytes(str_ptr, s, is_ascii);
    let matcher = if is_ascii { re.matcher_latin1 } else { None };
    let mut out_ascii = is_ascii;
    // Phase C-3 — baked DFA view bound once. See match_all.rs.
    // Round 3 Phase B sub-batch 7.2 — AOT then runtime-baked fallback.
    let dfa_view = re.baked_dfa_view();
    let dfa_ref = dfa_view.as_ref().or(re.dfa_runtime.as_ref());
    let mut out: Vec<u8> = Vec::with_capacity(s.len() + 16);
    let mut pos: i64 = if sticky_once {
        utf16_units_to_byte(s, re.last_index_i64(), is_ascii)
    } else {
        0
    };
    let mut matched = false;
    // bytes of `s` already in `out`; the sticky walk starts past 0
    let mut copied: usize = 0;
    while pos <= slen {
        let m = if let Some(f) = matcher {
            unsafe { run_matcher(f, re.matcher_stride as usize, &UnitsView::Narrow(s), pos) }
        } else if sticky {
            match_anchor(&re.prog, s, pos, re.flags)
        } else {
            // the per-RegExp workspace, taken for this search alone:
            // the callback may run the same regex before the next one
            let ws = unsafe { &mut *re.workspace_cache.get() };
            search_from_with_ws(&re.prog, s, pos, re.flags, ws, dfa_ref, is_ascii, true)
        };
        let Some(m) = m else { break };
        matched = true;
        if sticky_once {
            // §22.2.7.2 step 15, before the callback runs (step 14)
            let end = byte_to_utf16_units(s, m.end, is_ascii) as f64;
            unsafe { (*re_cell).set_last_index_num(end) };
        }
        out.extend_from_slice(&s[copied..m.start as usize]);
        let matched = &s[m.start as usize..m.end as usize];
        let match_str = if is_ascii {
            unsafe { str_from_bytes_ascii(matched) }
        } else {
            unsafe { str_from_bytes(matched) }
        };
        let ret = invoke(
            match_str as *mut c_void,
            m.saves(),
            &subject,
            // ES §22.1.3.18 — the cb offset arg is in UTF-16
            // code units of the input, not transcoded bytes.
            subject.units_at(m.start),
        );
        unsafe { __torajs_str_drop(match_str as *mut c_void) };
        let ret_str = match ret {
            Ok(p) => p,
            Err(()) => {
                // Pending throw inside the callback — abandon the
                // walk; the placeholder copy is discarded by the
                // caller's throw check.
                let s = unsafe { str_units(str_ptr) };
                return unsafe { str_from_bytes(&s) as *mut c_void };
            }
        };
        if !ret_str.is_null() {
            match unsafe { str_slice_ascii_view(ret_str) } {
                Some(view) => out.extend_from_slice(view),
                None => {
                    out_ascii = false;
                    out.extend_from_slice(&unsafe { str_units(ret_str) });
                }
            }
            unsafe { __torajs_str_drop(ret_str) };
        }
        if m.end == m.start {
            if m.start < slen {
                out.push(s[m.start as usize]);
            }
            pos = m.end + 1;
        } else {
            pos = m.end;
        }
        // pos can overshoot after an empty match at end-of-string
        copied = (pos as usize).min(s.len());
        if !global {
            break;
        }
    }
    if sticky_once && !matched {
        // §22.2.7.2 step 12.a.i
        unsafe { (*re_cell).set_last_index_num(0.0) };
    }
    out.extend_from_slice(&s[copied..]);
    if out_ascii {
        return unsafe { str_from_bytes_ascii(&out) as *mut c_void };
    }
    unsafe { str_from_bytes(&out) as *mut c_void }
}
