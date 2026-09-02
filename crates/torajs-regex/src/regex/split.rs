//! `__torajs_str_split_regex` — port of `runtime_regex.c`
//! L2861-2911.

use core::ffi::c_void;

use super::{
    __torajs_arr_alloc, __torajs_arr_push, __torajs_str_undef, RegExp, abort_unsupported, as_regex,
    haystack, str_from_bytes,
};
use crate::parser::RE_FLAG_Y;
use crate::vm::{Workspace, match_anchor, save_slot, search_from_with_ws};

/// ES §22.1.3.21 step 14.c.iii (chunk 803) — after each separator
/// match, the values of capture groups 1..=n splice into the result
/// array: participating groups as fresh Strs, non-participating
/// ones as the undefined sentinel (`"aXb".split(/(X)/)` answers
/// `["a", "X", "b"]`).
///
/// # Safety
/// `out` is a live Str array handle; `saves` is the match's row.
/// §22.2.6.14 step 19.b.i / 19.d.i AdvanceStringIndex, over the
/// transcoded haystack: one code unit without `u` (the code-unit
/// form spells every unit in one to three bytes), one code point
/// with it — either way the length of the form at `at`. A byte `+1`
/// landed inside a multi-byte character and read the string layer
/// off a torn cursor (`"xéy".split(/(?:)/)` was exit 138).
fn unit_len(s: &[u8], at: i64) -> i64 {
    crate::utf8::utf8_len_for(s[at as usize]) as i64
}

unsafe fn push_captures(out: *mut c_void, re: &RegExp, s: &[u8], saves: &[i64]) -> *mut c_void {
    let mut out = out;
    for i in 1..=(re.n_captures.max(0) as usize) {
        let gs = save_slot(saves, 2 * i);
        let ge = save_slot(saves, 2 * i + 1);
        if gs >= 0 && ge >= gs {
            let cell = unsafe { str_from_bytes(&s[gs as usize..ge as usize]) };
            out = unsafe { __torajs_arr_push(out, cell as i64) };
        } else {
            out = unsafe { __torajs_arr_push(out, __torajs_str_undef() as i64) };
        }
    }
    out
}

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
    let s = unsafe { haystack(re, str_ptr) };
    let slen = s.len() as i64;

    // Lazy-init Workspace — sticky branch uses match_anchor's own
    // Workspace; outer ws only needed in non-sticky branch.
    let mut ws: Option<Workspace> = None;
    let sticky = re.flags & RE_FLAG_Y != 0;
    // Phase C-3 — bind AOT-baked DFA view once. See match_all for the
    // same pattern and the rationale (zero `dfa_cache` reads on the
    // hot search loop once Phase C-4+ delivers baked entries).
    // Round 3 Phase B sub-batch 7.2 — runtime-baked DFA fallback.
    let dfa_view = re.baked_dfa_view();
    let dfa_ref = dfa_view.as_ref().or(re.dfa_runtime.as_ref());

    let mut out = out;
    // §22.2.6.14 step 14 (size == 0) — a zero-length subject only
    // asks whether the separator matches the empty string: a match
    // answers [], no match [""].
    if slen == 0 {
        let m = if sticky {
            match_anchor(&re.prog, &s, 0, re.flags)
        } else {
            search_from_with_ws(&re.prog, &s, 0, re.flags, &mut ws, dfa_ref, false, true)
        };
        if m.is_none() {
            let seg = unsafe { str_from_bytes(&[]) };
            out = unsafe { __torajs_arr_push(out, seg as i64) };
        }
        return out;
    }
    // §22.2.6.14 step 17 — p is the current segment start, q the
    // scan position. An empty match adjacent to the segment start
    // (`e == p`) never splits (`"x".split(/^/)` answers `["x"]`),
    // and the exec only happens while `q < size`, so a match
    // starting at the very end (`/$/`) never contributes either.
    let mut p: i64 = 0;
    let mut q: i64 = 0;
    while q < slen {
        let m = if sticky {
            // The spec splitter carries the `y` flag; a failed
            // anchor at q advances q (AdvanceStringIndex).
            match match_anchor(&re.prog, &s, q, re.flags) {
                Some(m) => Some(m),
                None => {
                    q += unit_len(&s, q);
                    continue;
                }
            }
        } else {
            // Round 3 Phase B attack #R-A1 — split currently routes
            // through `haystack` (transcodes to owned bytes), so the
            // ASCII-view shortcut isn't on this path. Pass `false`.
            // Round 5 attack #1 — Workspace materialises lazily
            // inside the vm.
            search_from_with_ws(&re.prog, &s, q, re.flags, &mut ws, dfa_ref, false, true)
        };
        let Some(m) = m else { break };
        if m.start >= slen {
            break;
        }
        let e = m.end.min(slen);
        if e == p {
            // Empty match at the segment start — no split; advance
            // past this position.
            q = m.start + unit_len(&s, m.start);
            continue;
        }
        let seg = unsafe { str_from_bytes(&s[p as usize..m.start as usize]) };
        out = unsafe { __torajs_arr_push(out, seg as i64) };
        out = unsafe { push_captures(out, re, &s, m.saves()) };
        p = e;
        q = e;
    }
    // Append final segment.
    let seg = unsafe { str_from_bytes(&s[p as usize..]) };
    out = unsafe { __torajs_arr_push(out, seg as i64) };
    out
}
