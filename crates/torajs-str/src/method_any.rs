//! `any`-receiver String method glue (Any-method-call RFC 20260704
//! C1 + C2) — `s.charAt(i)` / case / indexOf / slice / split / trim
//! where `s` crossed into the `any` world.
//!
//! Called by `__torajs_any_method_call`'s Str arm (torajs-anyvalue)
//! with an already-unboxed heap Str/Substr pointer. Returns raw
//! AnyValue bits — a heap cell's NaN-box encoding IS its pointer
//! bits, so a fresh rc=1 Str/Substr returns as `ptr as u64`
//! (ownership transfers to the caller, matching the boxed-value
//! convention).
//!
//! Substr receivers: the search / slice / split / trim kernels read
//! the owned-Str layout (`str_view`), so a Substr view (same
//! `Tag::Str`, parent+offset layout) materializes to an owned temp
//! first via [`owned_src`] — the same shape C1 shipped for the case
//! tables. Needles/separators handed over by the dispatcher may be
//! Substr cells too (e.g. an element read out of a `split` array),
//! so they go through the same helper.

use torajs_rc::HeapHeader;

use crate::block::__torajs_str_alloc;
use crate::index_any::__torajs_str_index_get;
use crate::lookup_ffi::__torajs_str_index_of_from;
use crate::slice::__torajs_str_slice;
use crate::split::ops::{__torajs_str_split, __torajs_str_split_no_sep};
use crate::str_drop::__torajs_str_drop;
use crate::substr::{FLAG_SUBSTR_INLINE, FLAG_SUBSTR_VIEW};
use crate::substr_methods::__torajs_substr_to_owned;
use crate::transform::case::{__torajs_str_to_lower, __torajs_str_to_upper};
use crate::transform::trim::{__torajs_str_trim, __torajs_str_trim_end, __torajs_str_trim_start};

unsafe extern "C" {
    /// Cross-tier — torajs-arr. Stamps the element-kind field on the
    /// typed array `split` returns (its slots are Substr heap
    /// pointers → `ARR_KIND_HEAP` = 4) so kind-aware any-world reads
    /// (`arr_index_get` / `arr_print_any` / join) stay correct.
    fn __torajs_arr_mark_kind(arr: *mut core::ffi::c_void, chain: u64);
}

/// `ARR_KIND_HEAP` mirror (torajs-rc) — the one kind `split` output
/// slots ever hold.
const KIND_HEAP_CHAIN: u64 = 4;

/// Resolve a possibly-Substr `Tag::Str` cell to an owned-layout
/// source: `(src, tmp)` where `tmp` is NULL for an already-owned Str
/// and the fresh materialized temp otherwise (caller drops it after
/// the kernel call).
unsafe fn owned_src(s: *const u8) -> (*const u8, *mut u8) {
    let header = unsafe { &*(s as *const HeapHeader) };
    if header.flags & (FLAG_SUBSTR_INLINE | FLAG_SUBSTR_VIEW) != 0 {
        let tmp = unsafe { __torajs_substr_to_owned(s) as *mut u8 };
        (tmp as *const u8, tmp)
    } else {
        (s, core::ptr::null_mut())
    }
}

/// Drop a non-NULL materialized temp.
#[inline]
unsafe fn drop_tmp(tmp: *mut u8) {
    if !tmp.is_null() {
        unsafe { __torajs_str_drop(tmp) };
    }
}

/// `s.charAt(idx)` per ES §22.1.3.1 — one-code-unit string, or the
/// EMPTY string for out-of-range (unlike `s[idx]`'s `undefined`).
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_char_at(s: *mut u8, idx: i64) -> u64 {
    unsafe {
        let out = __torajs_str_index_get(s, idx);
        if out.is_null() {
            // OOB: charAt answers "" (a fresh rc=1 empty Str).
            return __torajs_str_alloc(core::ptr::null(), 0) as u64;
        }
        out as u64
    }
}

/// `s.toUpperCase()` / `s.toLowerCase()` per ES §22.1.3.29/30 —
/// `upper != 0` picks the uppercase tables.
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_case(s: *const u8, upper: i64) -> u64 {
    unsafe {
        let (src, tmp) = owned_src(s);
        let out = if upper != 0 {
            __torajs_str_to_upper(src)
        } else {
            __torajs_str_to_lower(src)
        };
        drop_tmp(tmp);
        out as u64
    }
}

/// `s.indexOf(needle, from)` per ES §22.1.3.9 — found code-unit
/// index or `-1`. `includes` reuses this via `>= 0` (the empty-
/// needle clamp answers `from.clamp(0, len)`, non-negative either
/// way).
///
/// # Safety
/// `s` and `needle` are valid heap Str/Substr pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_index_of(
    s: *const u8,
    needle: *const u8,
    from: i64,
) -> i64 {
    unsafe {
        let (src, s_tmp) = owned_src(s);
        let (nn, n_tmp) = owned_src(needle);
        let out = __torajs_str_index_of_from(src, nn, from);
        drop_tmp(n_tmp);
        drop_tmp(s_tmp);
        out
    }
}

/// `s.slice(start, end)` per ES §22.1.3.21 — fresh Str; negative
/// wrap + clamp live in the kernel. The dispatcher encodes a missing
/// `end` as `i64::MAX` (clamps to `len`).
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_slice(s: *const u8, start: i64, end: i64) -> u64 {
    unsafe {
        let (src, tmp) = owned_src(s);
        let out = __torajs_str_slice(src, start, end);
        drop_tmp(tmp);
        out as u64
    }
}

/// `s.split(sep)` per ES §22.1.3.23 — NULL `sep` means "no
/// separator argument" (single-token array). The result block's
/// slots are Substr views into the source; a materialized Substr
/// receiver stays alive through the views' parent refcounts after
/// the temp drop. The fresh array's element kind stamps HEAP so
/// any-world reads dispatch correctly.
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer; `sep` is NULL or one too.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_split(s: *const u8, sep: *const u8) -> u64 {
    unsafe {
        let (src, s_tmp) = owned_src(s);
        let out = if sep.is_null() {
            __torajs_str_split_no_sep(src)
        } else {
            let (sp, sep_tmp) = owned_src(sep);
            let out = __torajs_str_split(src, sp);
            drop_tmp(sep_tmp);
            out
        };
        __torajs_arr_mark_kind(out as *mut core::ffi::c_void, KIND_HEAP_CHAIN);
        drop_tmp(s_tmp);
        out as u64
    }
}

/// `s.trim()` / `trimStart()` / `trimEnd()` per ES §22.1.3.27/28 —
/// `mode`: 0 = both, 1 = start, 2 = end.
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_trim(s: *const u8, mode: i64) -> u64 {
    unsafe {
        let (src, tmp) = owned_src(s);
        let out = match mode {
            1 => __torajs_str_trim_start(src),
            2 => __torajs_str_trim_end(src),
            _ => __torajs_str_trim(src),
        };
        drop_tmp(tmp);
        out as u64
    }
}
