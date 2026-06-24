//! `__torajs_str_match_regex` + `__torajs_regex_exec` +
//! `attach_groups` — port of `runtime_regex.c` L2257-2387, L2929-2988.

use alloc::vec::Vec;
use core::ffi::c_void;

use super::{
    __torajs_arr_alloc, __torajs_arr_push, __torajs_arrprops_set, __torajs_dynobj_alloc,
    __torajs_dynobj_mark_null_proto, __torajs_dynobj_set, __torajs_rc_inc, __torajs_str_drop,
    ANY_HEAP, ANY_I64, ANY_UNDEF, RegExp, abort_unsupported, as_regex_mut, str_from_bytes,
    str_slice, str_slice_ascii_view,
};
use crate::node::{REGEX_MAX_CAPTURES, REGEX_SAVE_SLOTS};
use crate::parser::{RE_FLAG_G, RE_FLAG_Y};
use crate::vm::{Workspace, match_anchor, search_from, search_from_with_ws};

/// Attach the spec §22.2.7.8 match-result properties `index` (match
/// start; UTF-8 byte domain — same domain note as lastIndex) and
/// `input` (the original subject Str, rc-shared, zero copy + zero
/// encoding drift) to `arr` via the arrprops side table. Insertion
/// order matters for the print face: index → input (→ groups, which
/// [`attach_groups`] appends right after).
///
/// # Safety
///
/// `arr` is a live tora Array; `str_ptr` a live Str outliving it.
pub unsafe fn attach_exec_props(arr: *mut c_void, str_ptr: *const c_void, index: i64) {
    unsafe {
        let k_index = str_from_bytes(b"index");
        __torajs_arrprops_set(arr, k_index as *mut c_void, ANY_I64 as i64, index);
        __torajs_str_drop(k_index as *mut c_void);
        let k_input = str_from_bytes(b"input");
        __torajs_rc_inc(str_ptr as *mut c_void);
        __torajs_arrprops_set(arr, k_input as *mut c_void, ANY_HEAP as i64, str_ptr as i64);
        __torajs_str_drop(k_input as *mut c_void);
    }
}

/// Build `.groups` dynobj from the named captures recorded on `re`
/// and the just-finished match's saves[]. Attaches the dict to
/// `arr` via the arrprops side table (so `arr.groups` resolves via
/// the standard Array.<unknown-prop> path). Without named captures
/// the property is still attached, as `undefined` (spec §22.2.7.8
/// step 24; bun prints `groups: undefined`).
///
/// # Safety
///
/// Calls cross-tier extern allocators; `arr` must be a live tora
/// Array. `re` and `s` must outlive the call.
pub unsafe fn attach_groups(
    arr: *mut c_void,
    re: &RegExp,
    s: &[u8],
    saves: &[i64; REGEX_SAVE_SLOTS],
) {
    if re.n_named_captures == 0 || re.capture_names.is_empty() {
        let outer_key = unsafe { str_from_bytes(b"groups") };
        unsafe {
            __torajs_arrprops_set(arr, outer_key as *mut c_void, ANY_UNDEF as i64, 0);
            __torajs_str_drop(outer_key as *mut c_void);
        }
        return;
    }
    // Spec: the groups object is created with a null prototype
    // (OrdinaryObjectCreate(null)); the flag drives the
    // `[Object: null prototype] ` print prefix.
    let mut groups = unsafe { __torajs_dynobj_alloc() };
    unsafe { __torajs_dynobj_mark_null_proto(groups) };
    let n_cap_lim = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
    for i in 1..=n_cap_lim {
        let name = match re.capture_names.get(i) {
            Some(n) if !n.is_empty() => n,
            _ => continue,
        };
        let name_key = unsafe { str_from_bytes(name) };
        let gs = saves[2 * i];
        let ge = saves[2 * i + 1];
        if gs < 0 || ge < 0 {
            // Non-participating named group → undefined.
            unsafe {
                __torajs_dynobj_set(&mut groups, name_key as *mut c_void, ANY_UNDEF, 0);
            }
        } else {
            let val_str = unsafe { str_from_bytes(&s[gs as usize..ge as usize]) };
            unsafe {
                __torajs_dynobj_set(
                    &mut groups,
                    name_key as *mut c_void,
                    ANY_HEAP,
                    val_str as u64,
                );
            }
        }
        unsafe { __torajs_str_drop(name_key as *mut c_void) };
    }
    let outer_key = unsafe { str_from_bytes(b"groups") };
    unsafe {
        __torajs_arrprops_set(
            arr,
            outer_key as *mut c_void,
            ANY_HEAP as i64,
            groups as i64,
        );
        __torajs_str_drop(outer_key as *mut c_void);
    }
}

/// `s.match(re)` — Phase 1c shape: Array<Str>.
/// - Without `g`: `[match, group1, group2, ...]` + `.groups` for
///   named captures.
/// - With `g`: array of all non-overlapping match substrings (per
///   ES spec drops capture info).
/// - Empty matches bump pos by 1 to avoid infinite loops.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `str_ptr` is null or a
/// live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_match_regex(
    str_ptr: *const c_void,
    re_ptr: *const c_void,
) -> *mut c_void {
    // Spec §22.2.7.5/.8: no match → null (both global and non-global).
    // The array is allocated lazily on first hit so the null path
    // never owns a stray +1 block.
    if re_ptr.is_null() || str_ptr.is_null() {
        return core::ptr::null_mut();
    }
    let re = unsafe { as_regex_mut(re_ptr as *mut c_void) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    // chunk 7.7 v2 step 12 C2 Phase B-1 attack #A — zero-copy
    // ASCII Latin-1 view; transcode fallback for non-ASCII Latin-1
    // / UTF-16 payloads. `_s_owned` is alive for the whole fn body
    // so the view's `'_` lifetime is safely bounded by `str_ptr`'s
    // caller-held reference.
    // Round 3 Phase B attack #R-A1 — capture the ASCII-view discrimination
    // so the hot-loop call can pass it down to `search_from_with_ws`,
    // letting the u-flag continuation-byte gate short-circuit on
    // ASCII-only haystacks.
    let _s_owned: Vec<u8>;
    let (s, haystack_is_ascii): (&[u8], bool) = match unsafe { str_slice_ascii_view(str_ptr) } {
        Some(view) => (view, true),
        None => {
            _s_owned = unsafe { str_slice(str_ptr) };
            (&_s_owned, false)
        }
    };
    let slen = s.len() as i64;

    let global = re.flags & RE_FLAG_G != 0;
    let sticky = re.flags & RE_FLAG_Y != 0;

    // Lazy-init Workspace — the `!global && sticky` branch below uses
    // match_anchor which builds its own Workspace, so the outer
    // search_from_with_ws's Workspace is only needed in the `else`
    // branch. Sticky-only callers (typical `r.exec` / `str.match(r)`
    // with /y/) skip this ~50KB allocation entirely.
    let mut ws: Option<Workspace> = None;
    // Phase C-3 — bind the AOT-baked DFA view once outside the loop;
    // see match_all.rs for the rationale.
    let dfa_view = re.baked_dfa_view();
    let mut out: *mut c_void = core::ptr::null_mut();
    let mut pos: i64 = 0;
    while pos <= slen {
        let hit = if !global && sticky {
            let start = re.last_index.max(0);
            let h = if start > slen {
                None
            } else {
                match_anchor(&re.prog, &s, start, re.flags)
            };
            re.last_index = h.as_ref().map(|m| m.end).unwrap_or(0);
            h
        } else {
            let ws_ref = ws.get_or_insert_with(|| Workspace::for_program(&re.prog));
            search_from_with_ws(
                &re.prog,
                &s,
                pos,
                re.flags,
                ws_ref,
                dfa_view.as_ref(),
                haystack_is_ascii,
            )
        };
        let Some(m) = hit else { break };
        if out.is_null() {
            out = unsafe { __torajs_arr_alloc(0) };
        }
        let seg = unsafe { str_from_bytes(&s[m.start as usize..m.end as usize]) };
        out = unsafe { __torajs_arr_push(out, seg as i64) };
        if !global {
            // Append captures.
            let n_cap_lim = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
            for i in 1..=n_cap_lim {
                let gs = m.saves()[2 * i];
                let ge = m.saves()[2 * i + 1];
                if gs < 0 || ge < 0 {
                    out = unsafe { __torajs_arr_push(out, 0) };
                } else {
                    let grp = unsafe { str_from_bytes(&s[gs as usize..ge as usize]) };
                    out = unsafe { __torajs_arr_push(out, grp as i64) };
                }
            }
            // Non-global match = exec shape (spec §22.2.7.8):
            // index / input / groups attach in print order.
            unsafe {
                attach_exec_props(out, str_ptr, m.start);
                attach_groups(out, re, &s, m.saves());
            }
            break;
        }
        // Empty match — bump pos by 1.
        pos = if m.end == m.start { m.end + 1 } else { m.end };
    }
    out
}

/// `re.exec(s)` — Phase 1c.1 spec-shape result `[match, g1, g2,
/// ...]` with named-capture `.groups` attached. Sticky / global
/// lastIndex bookkeeping matches spec §22.2.5.2.2.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `str_ptr` is null or a
/// live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_exec(
    re_ptr: *const c_void,
    str_ptr: *const c_void,
) -> *mut c_void {
    // Spec §22.2.7.2 step 9.a: no match → null. The array is
    // allocated only after a hit so the null path owns nothing.
    if re_ptr.is_null() || str_ptr.is_null() {
        return core::ptr::null_mut();
    }
    let re = unsafe { as_regex_mut(re_ptr as *mut c_void) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    // chunk 7.7 v2 step 12 C2 Phase B-1 attack #A — zero-copy ASCII
    // Latin-1 view; mirrors `__torajs_str_match_regex` rationale.
    let _s_owned: Vec<u8>;
    let s: &[u8] = match unsafe { str_slice_ascii_view(str_ptr) } {
        Some(view) => view,
        None => {
            _s_owned = unsafe { str_slice(str_ptr) };
            &_s_owned
        }
    };
    let slen = s.len() as i64;

    let sticky = re.flags & RE_FLAG_Y != 0;
    let global = re.flags & RE_FLAG_G != 0;
    let track = sticky || global;
    let start = if track { re.last_index.max(0) } else { 0 };

    // Phase C-3 — single-shot exec hits the same baked-DFA short-circuit.
    let dfa_view = re.baked_dfa_view();
    let m = if track && start > slen {
        None
    } else if sticky {
        match_anchor(&re.prog, &s, start, re.flags)
    } else {
        search_from(&re.prog, &s, start, re.flags, dfa_view.as_ref())
    };
    let Some(m) = m else {
        if track {
            re.last_index = 0;
        }
        return core::ptr::null_mut();
    };
    if track {
        re.last_index = m.end;
    }
    let mut out = unsafe { __torajs_arr_alloc(0) };
    let whole = unsafe { str_from_bytes(&s[m.start as usize..m.end as usize]) };
    out = unsafe { __torajs_arr_push(out, whole as i64) };
    let n_cap_lim = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
    for i in 1..=n_cap_lim {
        let gs = m.saves()[2 * i];
        let ge = m.saves()[2 * i + 1];
        if gs < 0 || ge < 0 {
            out = unsafe { __torajs_arr_push(out, 0) };
        } else {
            let grp = unsafe { str_from_bytes(&s[gs as usize..ge as usize]) };
            out = unsafe { __torajs_arr_push(out, grp as i64) };
        }
    }
    unsafe {
        attach_exec_props(out, str_ptr, m.start);
        attach_groups(out, re, &s, m.saves());
    }
    out
}
