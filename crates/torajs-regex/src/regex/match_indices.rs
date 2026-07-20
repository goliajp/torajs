//! `attach_indices` — MakeIndicesArray, spec §22.2.7.8. When the
//! regex carries the `d` (hasIndices) flag, every exec-shape match
//! result grows an `.indices` own prop: an Array whose element `i`
//! is the `[start, end]` pair (UTF-16 code units) for capture `i`
//! (`undefined` for a non-participating group), plus an
//! unconditional `groups` own prop (`undefined` without named
//! captures; a null-proto dict of name → the SAME pair array
//! otherwise — the spec shares one matchIndicesArray object between
//! `A[i]` and `groups[name]`, mirrored here via an rc share).
//!
//! Everything is built as self-describing Array<Any> cells (NaN-box
//! slots) so the checker-side Any lanes (`m.indices` member read →
//! arrprops probe → Any) print / index / gOPD without an external
//! elem-kind mark.

use alloc::vec::Vec;
use core::ffi::c_void;

use super::static_keys::{K_GROUPS, K_INDICES, cached_static_key};
use super::{
    __torajs_arr_alloc_any, __torajs_arr_push_any, __torajs_arrprops_set, __torajs_dynobj_alloc,
    __torajs_dynobj_mark_null_proto, __torajs_dynobj_set, __torajs_rc_inc, __torajs_str_drop,
    ANY_HEAP, ANY_I64, ANY_UNDEF, RegExp, byte_to_utf16_units, str_from_bytes,
};
use crate::node::REGEX_MAX_CAPTURES;
use crate::parser::RE_FLAG_D;
use crate::vm::save_slot;

/// Build and attach the `.indices` prop onto the exec-shape match
/// array `arr`. No-op unless `re` carries `RE_FLAG_D` — the flag
/// gate is the first branch so `/d`-less hot paths pay one test.
///
/// `m_start` / `m_end` are the whole-match BYTE span — slot 0's pair
/// comes from them, not from `saves[0..1]` (the vm leaves those
/// slots unset; `match_op` reads the whole match off the Match
/// record for the same reason). Capture slots ≥ 1 read `saves`.
///
/// # Safety
///
/// Same contract as `attach_exec_all`: `arr` is a live tora Array,
/// `str_ptr`-backed `s` and `re` outlive the call, `saves` is the
/// finished match's save-slot view.
pub(super) unsafe fn attach_indices(
    arr: *mut c_void,
    re: &RegExp,
    s: &[u8],
    m_start: i64,
    m_end: i64,
    saves: &[i64],
    haystack_is_ascii: bool,
) {
    if re.flags & RE_FLAG_D == 0 {
        return;
    }
    let n_cap_lim = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
    let mut indices: *mut c_void = unsafe { __torajs_arr_alloc_any((1 + n_cap_lim) as u64) };
    let has_groups = re.n_named_captures != 0 && !re.capture_names.is_empty();
    // §22.2.7.8 step 10-11: groups is a null-proto dict iff the
    // regex has named captures, else undefined.
    let mut groups: *mut c_void = core::ptr::null_mut();
    if has_groups {
        groups = unsafe { __torajs_dynobj_alloc() };
        unsafe { __torajs_dynobj_mark_null_proto(groups) };
    }
    // Duplicate named groups — same contract as `attach_groups`:
    // key order follows the FIRST occurrence, the PARTICIPATING
    // twin's pair wins (a defined value is never clobbered back to
    // undefined).
    let mut defined_names: Vec<&[u8]> = Vec::new();
    for i in 0..=n_cap_lim {
        let (gs, ge) = if i == 0 {
            (m_start, m_end)
        } else {
            (save_slot(saves, 2 * i), save_slot(saves, 2 * i + 1))
        };
        // §22.2.7.8 step 13.a-c — GetMatchIndexPair for a
        // participating slot, undefined otherwise. `.indices` pairs
        // are spec'd in UTF-16 code units; map the byte offsets.
        let pair: *mut c_void = if gs < 0 || ge < 0 {
            core::ptr::null_mut()
        } else {
            let st = byte_to_utf16_units(s, gs, haystack_is_ascii);
            let en = byte_to_utf16_units(s, ge, haystack_is_ascii);
            let mut p = unsafe { __torajs_arr_alloc_any(2) };
            unsafe {
                p = __torajs_arr_push_any(p, ANY_I64, st as u64);
                p = __torajs_arr_push_any(p, ANY_I64, en as u64);
            }
            p
        };
        // Element i (CreateDataProperty(A, ToString(i), ...)) —
        // push_any takes ownership of the pair's fresh reference.
        indices = if pair.is_null() {
            unsafe { __torajs_arr_push_any(indices, ANY_UNDEF, 0) }
        } else {
            unsafe { __torajs_arr_push_any(indices, ANY_HEAP, pair as u64) }
        };
        // §22.2.7.8 step 13.e — named capture: groups[name] shares
        // the SAME pair object (rc share, not a copy).
        if i == 0 || !has_groups {
            continue;
        }
        let name = match re.capture_names.get(i) {
            Some(n) if !n.is_empty() => n,
            _ => continue,
        };
        if pair.is_null() {
            if defined_names.iter().any(|n| *n == name.as_slice()) {
                continue;
            }
            let name_key = unsafe { str_from_bytes(name) };
            unsafe {
                __torajs_dynobj_set(&mut groups, name_key as *mut c_void, ANY_UNDEF, 0);
                __torajs_str_drop(name_key as *mut c_void);
            }
        } else {
            defined_names.push(name.as_slice());
            let name_key = unsafe { str_from_bytes(name) };
            unsafe {
                __torajs_rc_inc(pair);
                __torajs_dynobj_set(&mut groups, name_key as *mut c_void, ANY_HEAP, pair as u64);
                __torajs_str_drop(name_key as *mut c_void);
            }
        }
    }
    // §22.2.7.8 step 12 — `groups` own prop, unconditional.
    let k_groups = unsafe { cached_static_key(&K_GROUPS, b"groups") };
    unsafe {
        if has_groups {
            __torajs_arrprops_set(
                indices,
                k_groups as *mut c_void,
                ANY_HEAP as i64,
                groups as i64,
            );
        } else {
            __torajs_arrprops_set(indices, k_groups as *mut c_void, ANY_UNDEF as i64, 0);
        }
    }
    // RegExpBuiltinExec step 33.b — CreateDataProperty(A, "indices").
    let k_indices = unsafe { cached_static_key(&K_INDICES, b"indices") };
    unsafe {
        __torajs_arrprops_set(
            arr,
            k_indices as *mut c_void,
            ANY_HEAP as i64,
            indices as i64,
        );
    }
}
