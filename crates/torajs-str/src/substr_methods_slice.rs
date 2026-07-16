//! View-slice family Substr methods split from `substr_methods.rs`
//! (rotation 119 chunk 9, file-size decomp — parent 499 → ~415, back
//! under 460 soft-warn margin).
//!
//! Three cousin functions that mint a fresh Substr view on the SAME
//! root parent (drop chain stays depth-1) from an existing Substr
//! receiver:
//!
//! - `__torajs_substr_slice(v, start, end)` — ES §22.1.3.29 negative-
//!   wrap + clamp.
//! - `__torajs_substr_index_view(v, i)` — ES §10.4.3 [[Get]] on a JS
//!   code-unit index, out-of-range → immortal Substr undef sentinel.
//! - `__torajs_substr_substring(v, start, end)` — ES §22.1.3.32
//!   clamp + swap (no negative-wrap).

use core::ffi::c_void;

use crate::substr::__torajs_substr_create;

use super::substr_methods::{substr_len, substr_offset, substr_parent};

/// `substr.slice(start, end)` per ES §22.1.3.29: negative args wrap
/// to `len + arg`, clamp to `[0, len]`, `start > end` collapses to
/// empty.
///
/// # Safety
/// `v` is a live `*const Substr`. Returned pointer is a fresh Substr
/// (rc=1) referencing the SAME root parent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_slice(v: *const u8, start: i64, end: i64) -> *mut c_void {
    let cu_len = unsafe { substr_len(v) } as i64;
    let mut s = if start < 0 { cu_len + start } else { start };
    let mut e = if end < 0 { cu_len + end } else { end };
    if s < 0 {
        s = 0;
    }
    if e < 0 {
        e = 0;
    }
    if s > cu_len {
        s = cu_len;
    }
    if e > cu_len {
        e = cu_len;
    }
    if s > e {
        s = e;
    }
    let parent = unsafe { substr_parent(v) };
    let v_off = unsafe { substr_offset(v) };
    unsafe { __torajs_substr_create(parent as *mut c_void, v_off + s as u64, (e - s) as u64) }
}

/// `v[i]` — Substr INDEX read (ES §10.4.3 [[Get]]). Unlike the
/// slice family (which clamps OOB to an empty view), an
/// out-of-range index answers JS `undefined` — the immortal
/// Substr-shaped sentinel. A sentinel receiver propagates itself
/// (deref-safe; the spec TypeError guard face is ledgered
/// separately). In-range reads mint a fresh 1-code-unit view on
/// the same root parent, exactly like `substr_slice(v, i, i+1)`.
///
/// # Safety
/// `v` is a live `*const Substr` or the Substr sentinel.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_index_view(v: *const u8, i: i64) -> *mut u8 {
    if crate::undef_sentinel::is_substr_undef(v) {
        return crate::undef_sentinel::substr_undef_ptr();
    }
    let cu_len = unsafe { substr_len(v) } as i64;
    if i < 0 || i >= cu_len {
        return crate::undef_sentinel::substr_undef_ptr();
    }
    let parent = unsafe { substr_parent(v) };
    let v_off = unsafe { substr_offset(v) };
    unsafe { __torajs_substr_create(parent as *mut c_void, v_off + i as u64, 1) as *mut u8 }
}

/// `substr.substring(start, end)` — clamps + swaps (no wrap on
/// negatives unlike slice).
///
/// `start` / `end` are JS code-unit indices.
///
/// # Safety
/// `v` is a live `*const Substr`. Returned pointer is a fresh
/// Substr (rc=1) referencing the SAME root parent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_substring(
    v: *const u8,
    start: i64,
    end: i64,
) -> *mut c_void {
    let cu_len = unsafe { substr_len(v) } as i64;
    let mut start = start.max(0);
    let mut end = end.max(0);
    if start > cu_len {
        start = cu_len;
    }
    if end > cu_len {
        end = cu_len;
    }
    if start > end {
        core::mem::swap(&mut start, &mut end);
    }
    let parent = unsafe { substr_parent(v) };
    let v_off = unsafe { substr_offset(v) };
    unsafe {
        __torajs_substr_create(
            parent as *mut c_void,
            v_off + start as u64,
            (end - start) as u64,
        )
    }
}
