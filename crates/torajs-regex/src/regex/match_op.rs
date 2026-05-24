//! `__torajs_str_match_regex` + `__torajs_regex_exec` +
//! `attach_groups` — port of `runtime_regex.c` L2257-2387, L2929-2988.

use core::ffi::c_void;

use super::{
    __torajs_arr_alloc, __torajs_arr_push, __torajs_arrprops_set, __torajs_dynobj_alloc,
    __torajs_dynobj_set, __torajs_str_drop, ANY_HEAP, ANY_UNDEF, RegExp, abort_unsupported,
    as_regex_mut, str_from_bytes, str_slice,
};
use crate::node::{REGEX_MAX_CAPTURES, REGEX_SAVE_SLOTS};
use crate::parser::{RE_FLAG_G, RE_FLAG_Y};
use crate::vm::{Workspace, match_anchor, search_from, search_from_with_ws};

/// Build `.groups` dynobj from the named captures recorded on `re`
/// and the just-finished match's saves[]. Attaches the dict to
/// `arr` via the arrprops side table (so `arr.groups` resolves via
/// the standard Array.<unknown-prop> path). Skips work entirely if
/// `re` has no named captures.
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
        return;
    }
    let mut groups = unsafe { __torajs_dynobj_alloc() };
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
    let out = unsafe { __torajs_arr_alloc(0) };
    if re_ptr.is_null() || str_ptr.is_null() {
        return out;
    }
    let re = unsafe { as_regex_mut(re_ptr as *mut c_void) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    let s = unsafe { str_slice(str_ptr) };
    let slen = s.len() as i64;

    let global = re.flags & RE_FLAG_G != 0;
    let sticky = re.flags & RE_FLAG_Y != 0;

    let mut ws = Workspace::for_program(&re.prog);
    let mut out = out;
    let mut pos: i64 = 0;
    while pos <= slen {
        let hit = if !global && sticky {
            let start = re.last_index.max(0);
            let h = if start > slen {
                None
            } else {
                match_anchor(&re.prog, s, start, re.flags)
            };
            re.last_index = h.as_ref().map(|m| m.end).unwrap_or(0);
            h
        } else {
            search_from_with_ws(&re.prog, s, pos, re.flags, &mut ws)
        };
        let Some(m) = hit else { break };
        let seg = unsafe { str_from_bytes(&s[m.start as usize..m.end as usize]) };
        out = unsafe { __torajs_arr_push(out, seg as i64) };
        if !global {
            // Append captures.
            let n_cap_lim = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
            for i in 1..=n_cap_lim {
                let gs = m.saves[2 * i];
                let ge = m.saves[2 * i + 1];
                if gs < 0 || ge < 0 {
                    out = unsafe { __torajs_arr_push(out, 0) };
                } else {
                    let grp = unsafe { str_from_bytes(&s[gs as usize..ge as usize]) };
                    out = unsafe { __torajs_arr_push(out, grp as i64) };
                }
            }
            // Named captures → .groups dict.
            unsafe { attach_groups(out, re, s, &m.saves) };
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
    let out = unsafe { __torajs_arr_alloc(0) };
    if re_ptr.is_null() || str_ptr.is_null() {
        return out;
    }
    let re = unsafe { as_regex_mut(re_ptr as *mut c_void) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    let s = unsafe { str_slice(str_ptr) };
    let slen = s.len() as i64;

    let sticky = re.flags & RE_FLAG_Y != 0;
    let global = re.flags & RE_FLAG_G != 0;
    let track = sticky || global;
    let start = if track { re.last_index.max(0) } else { 0 };

    let m = if track && start > slen {
        None
    } else if sticky {
        match_anchor(&re.prog, s, start, re.flags)
    } else {
        search_from(&re.prog, s, start, re.flags)
    };
    let Some(m) = m else {
        if track {
            re.last_index = 0;
        }
        return out;
    };
    if track {
        re.last_index = m.end;
    }
    let mut out = out;
    let whole = unsafe { str_from_bytes(&s[m.start as usize..m.end as usize]) };
    out = unsafe { __torajs_arr_push(out, whole as i64) };
    let n_cap_lim = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
    for i in 1..=n_cap_lim {
        let gs = m.saves[2 * i];
        let ge = m.saves[2 * i + 1];
        if gs < 0 || ge < 0 {
            out = unsafe { __torajs_arr_push(out, 0) };
        } else {
            let grp = unsafe { str_from_bytes(&s[gs as usize..ge as usize]) };
            out = unsafe { __torajs_arr_push(out, grp as i64) };
        }
    }
    unsafe { attach_groups(out, re, s, &m.saves) };
    out
}
