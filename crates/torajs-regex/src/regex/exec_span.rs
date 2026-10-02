//! `re.exec(s)` answered as spans — the kernel of the exec-record
//! lane (`torajs-core` `ast::regex_exec_record`, which lists the
//! results a program only null-tests, indexes by a constant, asks
//! `index` / `input` of, or re-assigns from the same regex). The walk
//! and every lastIndex effect are [`super::exec_find`]'s, shared with
//! `__torajs_regex_exec`; what is left out is the array, the capture
//! strings and their release. A literal's matcher writes its row
//! into the record directly: the row's layout — the whole match in
//! slots 0 and 1, group `k` in `2k` and `2k + 1` — is the record's.

use core::ffi::c_void;

use super::MatcherFn;
use super::RegExp;
use super::exec_find::{exec_engine, exec_matcher, exec_open, matcher_for, run_matcher};
use super::str_helpers::subject::UnitsView;
use crate::node::REGEX_MAX_CAPTURES;
use crate::vm::save_slot;

/// Run one exec and write the first `n_slots` groups' spans into
/// `out`: `out[2k]` / `out[2k + 1]` are group `k`'s start and end in
/// UTF-16 code units (`k = 0` is the whole match, so `out[0]` is the
/// spec's `index`). A group that did not participate, and every slot
/// past the pattern's capture count, reads -1 in both. Answers 1 on a
/// hit and 0 on a miss; after a miss `out` holds nothing readable.
///
/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `str_ptr` is null or a live
/// `*Str`; `out` points at `2 * n_slots` writable `i64`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_exec_span(
    re_ptr: *const c_void,
    str_ptr: *const c_void,
    out: *mut i64,
    n_slots: i64,
) -> i64 {
    let n = n_slots.max(0) as usize;
    let Some((re, view)) = (unsafe { exec_open(re_ptr, str_ptr) }) else {
        return 0;
    };
    if let Some(f) = matcher_for(re, &view) {
        return unsafe { span_matcher(re, &view, f, out, n) }.is_some() as i64;
    }
    let hit = unsafe {
        exec_engine(re, str_ptr, |re, subject, m| {
            let n_cap = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
            for k in 0..n {
                let (gs, ge) = if k == 0 {
                    (m.start, m.end)
                } else if k <= n_cap {
                    (save_slot(m.saves(), 2 * k), save_slot(m.saves(), 2 * k + 1))
                } else {
                    (-1, -1)
                };
                let (a, b) = if gs < 0 || ge < 0 {
                    (-1, -1)
                } else {
                    (subject.units_at(gs), subject.units_at(ge))
                };
                *out.add(2 * k) = a;
                *out.add(2 * k + 1) = b;
            }
        })
    };
    hit.is_some() as i64
}

/// One exec through the literal's matcher with the record as its row.
/// A row wider than the record runs over a local one and the record
/// takes its first `n` groups.
///
/// # Safety
///
/// `view` is the payload `f` was synthesized for; `out` points at
/// `2 * n` writable `i64`s.
unsafe fn span_matcher(
    re: &mut RegExp,
    view: &UnitsView<'_>,
    f: MatcherFn,
    out: *mut i64,
    n: usize,
) -> Option<()> {
    let stride = re.matcher_stride as usize;
    let n_cap = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
    if stride > 2 * n {
        let mut m = None;
        unsafe {
            exec_matcher(re, view, |from| {
                m = run_matcher(f, stride, view, from);
                m.as_ref().map_or(-1, |m| m.end)
            })
        }?;
        let m = m?;
        for k in 0..n {
            let (gs, ge) = if k == 0 {
                (m.start, m.end)
            } else {
                (save_slot(m.saves(), 2 * k), save_slot(m.saves(), 2 * k + 1))
            };
            let absent = gs < 0 || ge < 0;
            unsafe {
                *out.add(2 * k) = if absent { -1 } else { gs };
                *out.add(2 * k + 1) = if absent { -1 } else { ge };
            }
        }
        return Some(());
    }
    let len = view.len() as i64;
    unsafe { exec_matcher(re, view, |from| f(view.as_ptr(), len, from, out)) }?;
    // a group set on one side only did not participate
    let groups = (stride / 2).min(n_cap + 1);
    for k in 1..groups {
        unsafe {
            if *out.add(2 * k) < 0 || *out.add(2 * k + 1) < 0 {
                *out.add(2 * k) = -1;
                *out.add(2 * k + 1) = -1;
            }
        }
    }
    for i in 2 * groups..2 * n {
        unsafe { *out.add(i) = -1 };
    }
    Some(())
}
