//! View-aware Substr method helpers — port of `runtime_str.c`
//! L1174-1378.
//!
//! 14 helpers covering string-prototype methods that operate on a
//! `Substr` receiver:
//!
//! - `char_code_at` / `eq_str` / `to_owned`
//! - `concat_substr_str` / `concat_str_substr` / `concat_substr_substr`
//! - `starts_with` / `ends_with` / `includes` / `index_of`
//! - `slice` / `substring`
//! - `trim` / `trim_start` / `trim_end`
//!
//! All read bytes via `parent.bytes + offset` (no materialize) and
//! either return primitives or alloc a fresh result Str / Substr.
//! The slice / substring / trim family produces a NEW Substr
//! whose parent is the SAME root parent (drop chain stays depth-1).

use core::ffi::c_void;

use crate::layout::{STR_HDR_SIZE, STR_LEN_OFF};
use crate::substr::{__torajs_substr_create, SUBSTR_LEN_OFF, SUBSTR_OFFSET_OFF, SUBSTR_PARENT_OFF};

#[cfg(not(test))]
unsafe extern "C" {
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
}

#[cfg(test)]
unsafe extern "C" {
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
}

#[inline]
unsafe fn substr_len(v: *const u8) -> u64 {
    unsafe { *(v.add(SUBSTR_LEN_OFF) as *const u64) }
}

#[inline]
unsafe fn substr_offset(v: *const u8) -> u64 {
    unsafe { *(v.add(SUBSTR_OFFSET_OFF) as *const u64) }
}

#[inline]
unsafe fn substr_parent(v: *const u8) -> *mut u8 {
    unsafe { *(v.add(SUBSTR_PARENT_OFF) as *const *mut u8) }
}

/// `(parent.bytes + offset)` — pointer to the first byte of the
/// view.
#[inline]
unsafe fn substr_data(v: *const u8) -> *const u8 {
    unsafe { substr_parent(v).add(STR_HDR_SIZE + substr_offset(v) as usize) }
}

#[inline]
unsafe fn str_len(s: *const u8) -> u64 {
    unsafe { *(s.add(STR_LEN_OFF) as *const u64) }
}

#[inline]
unsafe fn str_data(s: *const u8) -> *const u8 {
    unsafe { s.add(STR_HDR_SIZE) }
}

/// `s.charCodeAt(i)` on a Substr receiver. OOB / negative returns 0.
///
/// # Safety
/// `v` is a live `*const Substr` (rc > 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_char_code_at(v: *const u8, i: i64) -> i64 {
    let len = unsafe { substr_len(v) };
    if i < 0 || (i as u64) >= len {
        return 0;
    }
    unsafe { *substr_data(v).add(i as usize) as i64 }
}

/// Bytewise compare a Substr against an OWNED Str. Returns 1 iff
/// lengths equal AND bytes equal.
///
/// # Safety
/// `v` is a live `*const Substr`, `s` is a live `*const Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_eq_str(v: *const u8, s: *const u8) -> i64 {
    let v_len = unsafe { substr_len(v) };
    let s_len = unsafe { str_len(s) };
    if v_len != s_len {
        return 0;
    }
    if v_len == 0 {
        return 1;
    }
    let v_data = unsafe { substr_data(v) };
    let s_data = unsafe { str_data(s) };
    let eq = unsafe { core::slice::from_raw_parts(v_data, v_len as usize) }
        == unsafe { core::slice::from_raw_parts(s_data, s_len as usize) };
    if eq { 1 } else { 0 }
}

/// Materialize a Substr into a fresh OWNED Str (for crossing fn-call
/// boundaries that expect `Type::Str` — Phase Substr.B).
///
/// # Safety
/// `v` is a live `*const Substr`. Returned pointer is a pooled Str
/// (rc=1).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_to_owned(v: *const u8) -> *mut c_void {
    let len = unsafe { substr_len(v) };
    let p = unsafe { __torajs_str_alloc_pooled(len) };
    if len > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(substr_data(v), p.add(STR_HDR_SIZE), len as usize);
        }
    }
    p as *mut c_void
}

/// `(substr + str)` — single-alloc view-aware concat.
///
/// # Safety
/// `v` is a live `*const Substr`, `s` is a live `*const Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_concat_substr_str(
    v: *const u8,
    s: *const u8,
) -> *mut c_void {
    let v_len = unsafe { substr_len(v) };
    let s_len = unsafe { str_len(s) };
    let p = unsafe { __torajs_str_alloc_pooled(v_len + s_len) };
    let out = unsafe { p.add(STR_HDR_SIZE) };
    if v_len > 0 {
        unsafe { core::ptr::copy_nonoverlapping(substr_data(v), out, v_len as usize) };
    }
    if s_len > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(str_data(s), out.add(v_len as usize), s_len as usize)
        };
    }
    p as *mut c_void
}

/// `(str + substr)` — single-alloc view-aware concat.
///
/// # Safety
/// `s` is a live `*const Str`, `v` is a live `*const Substr`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_concat_str_substr(
    s: *const u8,
    v: *const u8,
) -> *mut c_void {
    let s_len = unsafe { str_len(s) };
    let v_len = unsafe { substr_len(v) };
    let p = unsafe { __torajs_str_alloc_pooled(s_len + v_len) };
    let out = unsafe { p.add(STR_HDR_SIZE) };
    if s_len > 0 {
        unsafe { core::ptr::copy_nonoverlapping(str_data(s), out, s_len as usize) };
    }
    if v_len > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(substr_data(v), out.add(s_len as usize), v_len as usize)
        };
    }
    p as *mut c_void
}

/// `(substr + substr)` — single-alloc view-aware concat.
///
/// # Safety
/// `a` and `b` are live `*const Substr`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_concat_substr_substr(
    a: *const u8,
    b: *const u8,
) -> *mut c_void {
    let a_len = unsafe { substr_len(a) };
    let b_len = unsafe { substr_len(b) };
    let p = unsafe { __torajs_str_alloc_pooled(a_len + b_len) };
    let out = unsafe { p.add(STR_HDR_SIZE) };
    if a_len > 0 {
        unsafe { core::ptr::copy_nonoverlapping(substr_data(a), out, a_len as usize) };
    }
    if b_len > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(substr_data(b), out.add(a_len as usize), b_len as usize)
        };
    }
    p as *mut c_void
}

/// `substr.startsWith(needle: Str)` — view-aware.
///
/// # Safety
/// `v` is a live `*const Substr`, `n` is a live `*const Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_starts_with(v: *const u8, n: *const u8) -> i8 {
    let v_len = unsafe { substr_len(v) };
    let n_len = unsafe { str_len(n) };
    if n_len > v_len {
        return 0;
    }
    if n_len == 0 {
        return 1;
    }
    let v_slice = unsafe { core::slice::from_raw_parts(substr_data(v), n_len as usize) };
    let n_slice = unsafe { core::slice::from_raw_parts(str_data(n), n_len as usize) };
    if v_slice == n_slice { 1 } else { 0 }
}

/// `substr.endsWith(needle: Str)` — view-aware.
///
/// # Safety
/// `v` is a live `*const Substr`, `n` is a live `*const Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_ends_with(v: *const u8, n: *const u8) -> i8 {
    let v_len = unsafe { substr_len(v) };
    let n_len = unsafe { str_len(n) };
    if n_len > v_len {
        return 0;
    }
    if n_len == 0 {
        return 1;
    }
    let tail_start = (v_len - n_len) as usize;
    let v_slice =
        unsafe { core::slice::from_raw_parts(substr_data(v).add(tail_start), n_len as usize) };
    let n_slice = unsafe { core::slice::from_raw_parts(str_data(n), n_len as usize) };
    if v_slice == n_slice { 1 } else { 0 }
}

/// `substr.includes(needle: Str)`.
///
/// # Safety
/// `v` is a live `*const Substr`, `n` is a live `*const Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_includes(v: *const u8, n: *const u8) -> i8 {
    let v_len = unsafe { substr_len(v) };
    let n_len = unsafe { str_len(n) };
    if n_len == 0 {
        return 1;
    }
    if n_len > v_len {
        return 0;
    }
    let v_data = unsafe { substr_data(v) };
    let n_data = unsafe { str_data(n) };
    let v_slice = unsafe { core::slice::from_raw_parts(v_data, v_len as usize) };
    let n_slice = unsafe { core::slice::from_raw_parts(n_data, n_len as usize) };
    if v_slice.windows(n_slice.len()).any(|w| w == n_slice) {
        1
    } else {
        0
    }
}

/// `substr.indexOf(needle: Str)` — `-1` on miss; `0` when needle empty.
///
/// # Safety
/// `v` is a live `*const Substr`, `n` is a live `*const Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_index_of(v: *const u8, n: *const u8) -> i64 {
    let v_len = unsafe { substr_len(v) };
    let n_len = unsafe { str_len(n) };
    if n_len == 0 {
        return 0;
    }
    if n_len > v_len {
        return -1;
    }
    let v_data = unsafe { substr_data(v) };
    let n_data = unsafe { str_data(n) };
    let v_slice = unsafe { core::slice::from_raw_parts(v_data, v_len as usize) };
    let n_slice = unsafe { core::slice::from_raw_parts(n_data, n_len as usize) };
    v_slice
        .windows(n_slice.len())
        .position(|w| w == n_slice)
        .map(|i| i as i64)
        .unwrap_or(-1)
}

/// `substr.slice(start, end)` — view-of-view. Negative indices wrap;
/// `start > end` clamps to empty.
///
/// # Safety
/// `v` is a live `*const Substr`. Returned pointer is a fresh
/// Substr (rc=1) referencing the SAME root parent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_slice(v: *const u8, start: i64, end: i64) -> *mut c_void {
    let v_len = unsafe { substr_len(v) } as i64;
    let mut s = if start < 0 { v_len + start } else { start };
    let mut e = if end < 0 { v_len + end } else { end };
    if s < 0 {
        s = 0;
    }
    if e < 0 {
        e = 0;
    }
    if s > v_len {
        s = v_len;
    }
    if e > v_len {
        e = v_len;
    }
    if s > e {
        s = e;
    }
    let parent = unsafe { substr_parent(v) };
    let v_off = unsafe { substr_offset(v) };
    unsafe { __torajs_substr_create(parent as *mut c_void, v_off + s as u64, (e - s) as u64) }
}

/// `substr.substring(start, end)` — clamps + swaps (no wrap on
/// negatives unlike slice).
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
    let v_len = unsafe { substr_len(v) } as i64;
    let mut start = start.max(0);
    let mut end = end.max(0);
    if start > v_len {
        start = v_len;
    }
    if end > v_len {
        end = v_len;
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

#[inline]
fn substr_is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

/// `substr.trim()` — narrow leading + trailing whitespace.
///
/// # Safety
/// `v` is a live `*const Substr`. Returned pointer is a fresh
/// Substr (rc=1) referencing the SAME root parent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_trim(v: *const u8) -> *mut c_void {
    let v_len = unsafe { substr_len(v) };
    let v_off = unsafe { substr_offset(v) };
    let parent = unsafe { substr_parent(v) };
    let base = unsafe { (parent as *const u8).add(STR_HDR_SIZE + v_off as usize) };
    let slice = unsafe { core::slice::from_raw_parts(base, v_len as usize) };
    let mut lo = 0u64;
    while lo < v_len && substr_is_ws(slice[lo as usize]) {
        lo += 1;
    }
    let mut hi = v_len;
    while hi > lo && substr_is_ws(slice[(hi - 1) as usize]) {
        hi -= 1;
    }
    unsafe { __torajs_substr_create(parent as *mut c_void, v_off + lo, hi - lo) }
}

/// `substr.trimStart()`.
///
/// # Safety
/// See [`__torajs_substr_trim`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_trim_start(v: *const u8) -> *mut c_void {
    let v_len = unsafe { substr_len(v) };
    let v_off = unsafe { substr_offset(v) };
    let parent = unsafe { substr_parent(v) };
    let base = unsafe { (parent as *const u8).add(STR_HDR_SIZE + v_off as usize) };
    let slice = unsafe { core::slice::from_raw_parts(base, v_len as usize) };
    let mut lo = 0u64;
    while lo < v_len && substr_is_ws(slice[lo as usize]) {
        lo += 1;
    }
    unsafe { __torajs_substr_create(parent as *mut c_void, v_off + lo, v_len - lo) }
}

/// `substr.trimEnd()`.
///
/// # Safety
/// See [`__torajs_substr_trim`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_trim_end(v: *const u8) -> *mut c_void {
    let v_len = unsafe { substr_len(v) };
    let v_off = unsafe { substr_offset(v) };
    let parent = unsafe { substr_parent(v) };
    let base = unsafe { (parent as *const u8).add(STR_HDR_SIZE + v_off as usize) };
    let slice = unsafe { core::slice::from_raw_parts(base, v_len as usize) };
    let mut hi = v_len;
    while hi > 0 && substr_is_ws(slice[(hi - 1) as usize]) {
        hi -= 1;
    }
    unsafe { __torajs_substr_create(parent as *mut c_void, v_off, hi) }
}
