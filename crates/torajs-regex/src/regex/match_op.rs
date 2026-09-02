//! `__torajs_str_match_regex` + `__torajs_regex_exec` +
//! `attach_groups` — port of `runtime_regex.c` L2257-2387, L2929-2988.

use alloc::vec::Vec;
use core::ffi::c_void;

use super::match_indices::attach_indices;
use super::static_keys::{K_GROUPS, K_INDEX, K_INPUT, cached_static_key};
use super::{
    __torajs_arr_alloc, __torajs_arr_push, __torajs_arrprops_attach_exec3, __torajs_arrprops_set,
    __torajs_dynobj_alloc, __torajs_dynobj_mark_null_proto, __torajs_dynobj_set, __torajs_rc_inc,
    __torajs_str_drop, __torajs_str_undef, ANY_HEAP, ANY_I64, ANY_UNDEF, RegExp, abort_unsupported,
    as_regex_mut, byte_to_utf16_units, haystack, str_from_bytes, str_slice_ascii_view,
    utf16_units_to_byte,
};
use crate::node::REGEX_MAX_CAPTURES;
use crate::parser::{RE_FLAG_G, RE_FLAG_Y};
use crate::vm::{Workspace, match_anchor, save_slot, search_from, search_from_with_ws};

/// Attach the spec §22.2.7.8 match-result properties `index` (match
/// start; UTF-8 byte domain — same domain note as lastIndex) and
/// `input` (the original subject Str, rc-shared, zero copy + zero
/// encoding drift) to `arr` via the arrprops side table. Insertion
/// order matters for the print face: index → input (→ groups, which
/// [`attach_groups`] appends right after).
///
/// Round 4 wire-back Phase B chunk 1 — `"index"` / `"input"` keys
/// route through `cached_static_key` (immortal static slot,
/// `FLAG_STATIC_LITERAL` stamped). Drops elided since the flag
/// makes them no-ops; `__torajs_rc_inc` on the cached key inside
/// `dynobj_set`'s fresh-insert branch is likewise a no-op.
///
/// # Safety
///
/// `arr` is a live tora Array; `str_ptr` a live Str outliving it.
/// Attach the full exec triple (`index` / `input` / `groups`) —
/// Round 5 attack #4 batch fast path for the no-named-captures shape
/// (one cross-tier call, no probe, no per-key rc_inc; the dynobj
/// side computes the three hash slots from compile-time FNV
/// constants). Named-capture regexes keep the generic
/// [`attach_exec_props`] + [`attach_groups`] pair. `/d` regexes grow
/// the fourth prop `indices` (§22.2.7.8 MakeIndicesArray) via the
/// trailing [`attach_indices`] call — a no-op without the flag.
///
/// # Safety
///
/// Same contract as [`attach_exec_props`] + [`attach_groups`]; `arr`
/// must be a fresh match array (NULL props slot).
pub unsafe fn attach_exec_all(
    arr: *mut c_void,
    re: &RegExp,
    s: &[u8],
    str_ptr: *const c_void,
    m_start: i64,
    m_end: i64,
    saves: &[i64],
    haystack_is_ascii: bool,
) {
    // `.index` is spec'd in UTF-16 code units; `m_start` / `m_end`
    // are byte offsets in the transcoded haystack (`attach_indices`
    // needs the byte span for slot 0's pair, so the mapping happens
    // here instead of at the call sites).
    let index = byte_to_utf16_units(s, m_start, haystack_is_ascii);
    if re.n_named_captures == 0 || re.capture_names.is_empty() {
        unsafe {
            let k_index = cached_static_key(&K_INDEX, b"index");
            let k_input = cached_static_key(&K_INPUT, b"input");
            let k_groups = cached_static_key(&K_GROUPS, b"groups");
            // the entry takes an rc share of the subject (exactly
            // like the dynobj_set path did).
            __torajs_rc_inc(str_ptr as *mut c_void);
            __torajs_arrprops_attach_exec3(
                arr,
                k_index as *mut c_void,
                index,
                k_input as *mut c_void,
                str_ptr as i64,
                k_groups as *mut c_void,
            );
        }
    } else {
        unsafe {
            attach_exec_props(arr, str_ptr, index);
            attach_groups(arr, re, s, saves);
        }
    }
    unsafe {
        attach_indices(arr, re, s, m_start, m_end, saves, haystack_is_ascii);
    }
}

pub unsafe fn attach_exec_props(arr: *mut c_void, str_ptr: *const c_void, index: i64) {
    unsafe {
        let k_index = cached_static_key(&K_INDEX, b"index");
        __torajs_arrprops_set(arr, k_index as *mut c_void, ANY_I64 as i64, index);
        let k_input = cached_static_key(&K_INPUT, b"input");
        __torajs_rc_inc(str_ptr as *mut c_void);
        __torajs_arrprops_set(arr, k_input as *mut c_void, ANY_HEAP as i64, str_ptr as i64);
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
pub unsafe fn attach_groups(arr: *mut c_void, re: &RegExp, s: &[u8], saves: &[i64]) {
    if re.n_named_captures == 0 || re.capture_names.is_empty() {
        // Round 4 wire-back Phase B chunk 1 — cached static "groups" key.
        let outer_key = unsafe { cached_static_key(&K_GROUPS, b"groups") };
        unsafe {
            __torajs_arrprops_set(arr, outer_key as *mut c_void, ANY_UNDEF as i64, 0);
        }
        return;
    }
    // Spec: the groups object is created with a null prototype
    // (OrdinaryObjectCreate(null)); the flag drives the
    // `[Object: null prototype] ` print prefix.
    let mut groups = unsafe { __torajs_dynobj_alloc() };
    unsafe { __torajs_dynobj_mark_null_proto(groups) };
    let n_cap_lim = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
    // Duplicate named groups (`(?:(?<z>c)|(?<z>d))`) — §22.2.7.8:
    // the groups object carries the PARTICIPATING twin's value, but
    // key order follows the FIRST occurrence in source order. So a
    // non-participating slot still writes its undefined placeholder
    // (a later participating twin's dynobj set updates in place,
    // keeping the slot), unless a participating twin already wrote
    // a defined value — that must not be clobbered back.
    let mut defined_names: Vec<&[u8]> = Vec::new();
    for i in 1..=n_cap_lim {
        let name = match re.capture_names.get(i) {
            Some(n) if !n.is_empty() => n,
            _ => continue,
        };
        let name_key = unsafe { str_from_bytes(name) };
        let gs = save_slot(saves, 2 * i);
        let ge = save_slot(saves, 2 * i + 1);
        if gs < 0 || ge < 0 {
            if defined_names.iter().any(|n| *n == name.as_slice()) {
                unsafe { __torajs_str_drop(name_key as *mut c_void) };
                continue;
            }
            // Non-participating named group → undefined.
            unsafe {
                __torajs_dynobj_set(&mut groups, name_key as *mut c_void, ANY_UNDEF, 0);
            }
        } else {
            defined_names.push(name.as_slice());
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
    // Round 4 wire-back Phase B chunk 1 — same cached static "groups" key.
    let outer_key = unsafe { cached_static_key(&K_GROUPS, b"groups") };
    unsafe {
        __torajs_arrprops_set(
            arr,
            outer_key as *mut c_void,
            ANY_HEAP as i64,
            groups as i64,
        );
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
    want_exec: i64,
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
            _s_owned = unsafe { haystack(re, str_ptr) };
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
    // A boxed-form lastIndex (non-numeric any-lane store) normalizes
    // to numeric ONLY on the shapes that consume it (§22.2.7.2 reads
    // + always re-Sets lastIndex under global/sticky, so the verbatim
    // value is dead either way); a plain non-global non-sticky match
    // never touches lastIndex and must leave the stored value intact.
    // Done before the dfa_view borrow so the sticky arm below can
    // keep its disjoint field writes.
    if (global || sticky) && re.last_index_boxed != 0 {
        let n = unsafe { super::__torajs_anyv_to_number(re.last_index_boxed) };
        unsafe { super::__torajs_value_drop_heap(re.last_index_boxed as *mut c_void) };
        re.last_index_boxed = 0;
        re.last_index = n;
    }
    // Phase C-3 — bind the AOT-baked DFA view once outside the loop;
    // see match_all.rs for the rationale.
    // Round 3 Phase B sub-batch 7.2 — runtime-baked DFA fallback.
    let dfa_view = re.baked_dfa_view();
    let dfa_ref = dfa_view.as_ref().or(re.dfa_runtime.as_ref());
    let mut out: *mut c_void = core::ptr::null_mut();
    let mut pos: i64 = 0;
    while pos <= slen {
        let hit = if !global && sticky {
            // lastIndex is spec'd in UTF-16 code units; the engine
            // works in transcoded UTF-8 bytes — map on read + write.
            let start = utf16_units_to_byte(&s, re.last_index_i64(), haystack_is_ascii);
            let h = if start > slen {
                None
            } else {
                match_anchor(&re.prog, &s, start, re.flags)
            };
            // Disjoint field write (the boxed form was normalized
            // above the dfa_view borrow, so no drop is needed here).
            re.last_index = h
                .as_ref()
                .map(|m| byte_to_utf16_units(&s, m.end, haystack_is_ascii) as f64)
                .unwrap_or(0.0);
            h
        } else if sticky {
            // Global + sticky: sticky wins the search shape — each
            // successive match must anchor exactly at `pos` (spec
            // §22.1.3.12 loops RegExpExec, and §22.2.7.2 step 11.a
            // fails a /y/ exec that doesn't match at lastIndex).
            // Pre-fix this fell into the free-search arm below and
            // collected non-contiguous matches / matched past a
            // leading miss.
            match_anchor(&re.prog, &s, pos, re.flags)
        } else {
            // Round 5 attack #1 — Workspace materialisation is sunk
            // into the vm's `vm_match_at` call sites: DFA-resident +
            // no-save programs never touch it, so the whole 5-Vec
            // alloc/free cycle disappears from this hot loop.
            search_from_with_ws(
                &re.prog,
                &s,
                pos,
                re.flags,
                &mut ws,
                dfa_ref,
                haystack_is_ascii,
                true,
            )
        };
        let Some(m) = hit else { break };
        if out.is_null() {
            // Round 5 attack #6 — pre-size to the exec shape
            // (m[0] + capture groups) so the first push never takes
            // the cap0→4 grow (pool pop + 32B header memcpy + pool
            // push) that used to fire on every fresh result array.
            // The global branch pushes one segment per hit and grows
            // normally past the pre-size; still strictly better than
            // starting at 0.
            let n_cap_lim = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
            out = unsafe { __torajs_arr_alloc(1 + n_cap_lim as u64) };
        }
        let seg = unsafe { str_from_bytes(&s[m.start as usize..m.end as usize]) };
        out = unsafe { __torajs_arr_push(out, seg as i64) };
        if !global {
            // Append captures.
            let n_cap_lim = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
            for i in 1..=n_cap_lim {
                let gs = save_slot(m.saves(), 2 * i);
                let ge = save_slot(m.saves(), 2 * i + 1);
                if gs < 0 || ge < 0 {
                    // Non-participating group = JS undefined (RFC
                    // 20260707 chunk 2: the sentinel cell, not NULL).
                    out = unsafe { __torajs_arr_push(out, __torajs_str_undef() as i64) };
                } else {
                    let grp = unsafe { str_from_bytes(&s[gs as usize..ge as usize]) };
                    out = unsafe { __torajs_arr_push(out, grp as i64) };
                }
            }
            // Non-global match = exec shape (spec §22.2.7.8):
            // index / input / groups (+ `/d` indices) attach in
            // print order; byte→UTF-16 mapping happens inside.
            if want_exec != 0 {
                unsafe {
                    attach_exec_all(
                        out,
                        re,
                        &s,
                        str_ptr,
                        m.start,
                        m.end,
                        m.saves(),
                        haystack_is_ascii,
                    );
                }
            }
            break;
        }
        // Empty match — bump pos by 1.
        pos = if m.end == m.start { m.end + 1 } else { m.end };
    }
    if global {
        // Spec §22.1.3.12: global match resets lastIndex to 0 before
        // the exec loop, and the loop only terminates on an exec miss
        // which itself stores 0 — so the observable post-state is
        // always 0, even when the caller pre-set a nonzero value.
        re.set_last_index_num(0.0);
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
    want_exec: i64,
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
    // lastIndex is spec'd in UTF-16 code units (§22.2.7.2 step 4);
    // map to a byte offset in the transcoded haystack. Out-of-range
    // maps to slen + 1 so the `start > slen` guard below fires.
    let start = if track {
        utf16_units_to_byte(s, re.last_index_i64(), haystack_is_ascii)
    } else {
        0
    };

    // Phase C-3 — single-shot exec hits the same baked-DFA short-circuit.
    // Round 3 Phase B sub-batch 7.2 — runtime-baked DFA fallback.
    let dfa_view = re.baked_dfa_view();
    let dfa_ref = dfa_view.as_ref().or(re.dfa_runtime.as_ref());
    let m = if track && start > slen {
        None
    } else if sticky {
        match_anchor(&re.prog, &s, start, re.flags)
    } else {
        search_from(&re.prog, &s, start, re.flags, dfa_ref)
    };
    let Some(m) = m else {
        if track {
            re.set_last_index_num(0.0);
        }
        return core::ptr::null_mut();
    };
    if track {
        re.set_last_index_num(byte_to_utf16_units(s, m.end, haystack_is_ascii) as f64);
    }
    // Round 5 attack #6 — pre-size to the exec shape (see match loop
    // above); the pushes below fill exactly 1 + n_cap_lim slots.
    let n_cap_lim = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
    let mut out = unsafe { __torajs_arr_alloc(1 + n_cap_lim as u64) };
    let whole = unsafe { str_from_bytes(&s[m.start as usize..m.end as usize]) };
    out = unsafe { __torajs_arr_push(out, whole as i64) };
    for i in 1..=n_cap_lim {
        let gs = save_slot(m.saves(), 2 * i);
        let ge = save_slot(m.saves(), 2 * i + 1);
        if gs < 0 || ge < 0 {
            // Non-participating group = JS undefined (RFC 20260707
            // chunk 2: the sentinel cell, not NULL).
            out = unsafe { __torajs_arr_push(out, __torajs_str_undef() as i64) };
        } else {
            let grp = unsafe { str_from_bytes(&s[gs as usize..ge as usize]) };
            out = unsafe { __torajs_arr_push(out, grp as i64) };
        }
    }
    if want_exec != 0 {
        unsafe {
            attach_exec_all(
                out,
                re,
                s,
                str_ptr,
                m.start,
                m.end,
                m.saves(),
                haystack_is_ascii,
            );
        }
    }
    out
}
