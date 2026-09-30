//! `re.exec(s)` answered as spans — the kernel of the exec-record
//! lane (`torajs-core` `ast::regex_exec_record`, which lists the
//! results a program only null-tests, indexes by a constant, asks
//! `index` / `input` of, or re-assigns from the same regex). The walk
//! and every lastIndex effect are [`super::exec_find::exec_find`]'s,
//! shared with `__torajs_regex_exec`; what is left out is the array,
//! the capture strings and their release.

use core::ffi::c_void;

use super::byte_to_utf16_units;
use super::exec_find::exec_find;
use crate::node::REGEX_MAX_CAPTURES;
use crate::vm::save_slot;

/// Run one exec and write the first `n_slots` groups' spans into
/// `out`: `out[2k]` / `out[2k + 1]` are group `k`'s start and end in
/// UTF-16 code units (`k = 0` is the whole match, so `out[0]` is the
/// spec's `index`). A group that did not participate, and every slot
/// past the pattern's capture count, reads -1 in both. Answers 1 on a
/// hit and 0 on a miss; `out` is untouched on a miss.
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
    let hit = unsafe {
        exec_find(re_ptr, str_ptr, |re, s, is_ascii, m| {
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
                    (
                        byte_to_utf16_units(s, gs, is_ascii),
                        byte_to_utf16_units(s, ge, is_ascii),
                    )
                };
                *out.add(2 * k) = a;
                *out.add(2 * k + 1) = b;
            }
        })
    };
    hit.is_some() as i64
}
