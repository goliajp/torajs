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

use torajs_rc::{
    ANY_METHOD_ANCHOR, ANY_METHOD_BIG, ANY_METHOD_BLINK, ANY_METHOD_BOLD, ANY_METHOD_FIXED,
    ANY_METHOD_FONTCOLOR, ANY_METHOD_FONTSIZE, ANY_METHOD_ITALICS, ANY_METHOD_LINK,
    ANY_METHOD_SMALL, ANY_METHOD_STRIKE, ANY_METHOD_SUB, ANY_METHOD_SUP, HeapHeader,
};

use crate::block::__torajs_str_alloc;
use crate::html::{
    __torajs_str_anchor, __torajs_str_big, __torajs_str_blink, __torajs_str_bold,
    __torajs_str_fixed, __torajs_str_fontcolor, __torajs_str_fontsize, __torajs_str_italics,
    __torajs_str_link, __torajs_str_small, __torajs_str_strike, __torajs_str_sub, __torajs_str_sup,
};
use crate::index_any::__torajs_str_index_get;
use crate::lookup_ffi::{
    __torajs_str_ends_with_from, __torajs_str_index_of_from, __torajs_str_starts_with_from,
};
use crate::slice::__torajs_str_slice;
use crate::split::ops::{__torajs_str_split, __torajs_str_split_no_sep};
use crate::str_drop::__torajs_str_drop;
use crate::substr::{FLAG_SUBSTR_INLINE, FLAG_SUBSTR_VIEW};
use crate::substr_methods::__torajs_substr_to_owned;
use crate::transform::case::{__torajs_str_to_lower, __torajs_str_to_upper};
use crate::transform::replace::{__torajs_str_replace, __torajs_str_replace_all};
use crate::transform::trim::{__torajs_str_trim, __torajs_str_trim_end, __torajs_str_trim_start};

unsafe extern "C" {
    /// Cross-tier — torajs-arr. Stamps the element-kind field on the
    /// typed array `split` returns (its slots are Substr heap
    /// pointers → `ARR_KIND_HEAP` = 4) so kind-aware any-world reads
    /// (`arr_index_get` / `arr_print_any` / join) stay correct.
    fn __torajs_arr_mark_kind(arr: *mut core::ffi::c_void, chain: u64);
    /// Cross-tier — torajs-regex kernels (owned-Str layout inputs;
    /// extern decls only, resolved at `tr build` link time).
    fn __torajs_str_match_regex(
        s: *const core::ffi::c_void,
        re: *const core::ffi::c_void,
    ) -> *mut core::ffi::c_void;
    fn __torajs_str_split_regex(
        s: *const core::ffi::c_void,
        re: *const core::ffi::c_void,
    ) -> *mut core::ffi::c_void;
    fn __torajs_str_replace_regex(
        s: *const core::ffi::c_void,
        re: *const core::ffi::c_void,
        repl: *const core::ffi::c_void,
    ) -> *mut core::ffi::c_void;
    fn __torajs_str_replace_all_regex(
        s: *const core::ffi::c_void,
        re: *const core::ffi::c_void,
        repl: *const core::ffi::c_void,
    ) -> *mut core::ffi::c_void;
}

/// NaN-box `null` bit pattern (torajs-anyvalue `VALUE_NULL` mirror)
/// — `s.match` answers JS null on no-match.
const VALUE_NULL_BOX: u64 = 2;

/// `ARR_KIND_HEAP` mirror (torajs-rc) — the one kind `split` output
/// slots ever hold. Shared with the [`crate::method_any_ext`] slice
/// (matchAll's product).
pub(crate) const KIND_HEAP_CHAIN: u64 = 4;

/// Resolve a possibly-Substr `Tag::Str` cell to an owned-layout
/// source: `(src, tmp)` where `tmp` is NULL for an already-owned Str
/// and the fresh materialized temp otherwise (caller drops it after
/// the kernel call). Shared with the [`crate::method_any_ext`]
/// slice.
pub(crate) unsafe fn owned_src(s: *const u8) -> (*const u8, *mut u8) {
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
pub(crate) unsafe fn drop_tmp(tmp: *mut u8) {
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

/// `s.split(re)` glue for a RegExp separator — ES §22.1.3.23 step 2
/// hands a RegExp separator to `@@split`. The regex kernel answers
/// fresh Str slots (capture groups splice per step 14.c.iii), so the
/// product only needs the heap-chain mark for any-world reads.
///
/// # Safety
/// `s` is a live Str/Substr cell; `re` is a live RegExp cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_split_regex(
    s: *const u8,
    re: *const core::ffi::c_void,
) -> u64 {
    unsafe {
        let (src, tmp) = owned_src(s);
        let out = __torajs_str_split_regex(src as *const core::ffi::c_void, re);
        drop_tmp(tmp);
        __torajs_arr_mark_kind(out, KIND_HEAP_CHAIN);
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

/// `s.match(re)` glue (RFC 20260706-test262-bug-corpus RC-2) — the
/// receiver materializes through [`owned_src`] (the regex kernels
/// read the owned-Str layout), the product is heap-chain-marked so
/// any-world index reads box its Str-cell slots, and no-match
/// answers the boxed JS null.
///
/// # Safety
/// `s` is a live Str/Substr cell; `re` is a live RegExp cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_match(s: *const u8, re: *const core::ffi::c_void) -> u64 {
    unsafe {
        let (src, tmp) = owned_src(s);
        let out = __torajs_str_match_regex(src as *const core::ffi::c_void, re);
        drop_tmp(tmp);
        if out.is_null() {
            VALUE_NULL_BOX
        } else {
            __torajs_arr_mark_kind(out, KIND_HEAP_CHAIN);
            out as u64
        }
    }
}

/// `s.replace(needle, repl)` / `replaceAll` glue for string-pattern
/// arguments — all three operands materialize through [`owned_src`].
///
/// # Safety
/// `s` / `needle` / `repl` are live Str/Substr cells.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_replace(
    s: *const u8,
    needle: *const u8,
    repl: *const u8,
    all: i64,
) -> u64 {
    unsafe {
        let (src, t0) = owned_src(s);
        let (nd, t1) = owned_src(needle);
        let (rp, t2) = owned_src(repl);
        let out = if all != 0 {
            __torajs_str_replace_all(src, nd, rp)
        } else {
            __torajs_str_replace(src, nd, rp)
        };
        drop_tmp(t2);
        drop_tmp(t1);
        drop_tmp(t0);
        out as u64
    }
}

/// `s.replace(re, repl)` / `replaceAll` glue for RegExp-pattern
/// arguments.
///
/// # Safety
/// `s` / `repl` are live Str/Substr cells; `re` is a live RegExp.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_replace_regex(
    s: *const u8,
    re: *const core::ffi::c_void,
    repl: *const u8,
    all: i64,
) -> u64 {
    unsafe {
        let (src, t0) = owned_src(s);
        let (rp, t1) = owned_src(repl);
        let out = if all != 0 {
            __torajs_str_replace_all_regex(
                src as *const core::ffi::c_void,
                re,
                rp as *const core::ffi::c_void,
            )
        } else {
            __torajs_str_replace_regex(
                src as *const core::ffi::c_void,
                re,
                rp as *const core::ffi::c_void,
            )
        };
        drop_tmp(t1);
        drop_tmp(t0);
        out as u64
    }
}

/// `s.startsWith(needle, pos)` per ES §22.1.3.23 — 1/0. Clamping
/// and the empty-needle always-match live in the typed-tier kernel;
/// this glue only materializes Substr views (same shape as the
/// indexOf glue).
///
/// # Safety
/// `s` and `needle` are valid heap Str/Substr pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_starts_with(
    s: *const u8,
    needle: *const u8,
    pos: i64,
) -> i64 {
    unsafe {
        let (src, s_tmp) = owned_src(s);
        let (nn, n_tmp) = owned_src(needle);
        let out = __torajs_str_starts_with_from(src, nn, pos);
        drop_tmp(n_tmp);
        drop_tmp(s_tmp);
        out
    }
}

/// annexB B.2.2 html-method glue — `mid` is the interned
/// `ANY_METHOD_*` id picking the CreateHTML form (attr forms 95-98,
/// plain wraps 99-107); the receiver and a non-NULL attribute value
/// materialize through [`owned_src`] (the html kernel reads the
/// owned-Str layout). NULL `val` denotes the JS `undefined` (the
/// kernel renders the "undefined" text). Ids outside the html
/// family answer 0 — the dispatcher's range guard never sends one.
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer; `val` is NULL or one too.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_html(s: *const u8, mid: i64, val: *const u8) -> u64 {
    unsafe {
        let (src, s_tmp) = owned_src(s);
        let (v, v_tmp) = if val.is_null() {
            (core::ptr::null(), core::ptr::null_mut())
        } else {
            owned_src(val)
        };
        let out = match mid {
            m if m == ANY_METHOD_ANCHOR => __torajs_str_anchor(src, v),
            m if m == ANY_METHOD_FONTCOLOR => __torajs_str_fontcolor(src, v),
            m if m == ANY_METHOD_FONTSIZE => __torajs_str_fontsize(src, v),
            m if m == ANY_METHOD_LINK => __torajs_str_link(src, v),
            m if m == ANY_METHOD_BIG => __torajs_str_big(src),
            m if m == ANY_METHOD_BLINK => __torajs_str_blink(src),
            m if m == ANY_METHOD_BOLD => __torajs_str_bold(src),
            m if m == ANY_METHOD_FIXED => __torajs_str_fixed(src),
            m if m == ANY_METHOD_ITALICS => __torajs_str_italics(src),
            m if m == ANY_METHOD_SMALL => __torajs_str_small(src),
            m if m == ANY_METHOD_STRIKE => __torajs_str_strike(src),
            m if m == ANY_METHOD_SUB => __torajs_str_sub(src),
            m if m == ANY_METHOD_SUP => __torajs_str_sup(src),
            _ => core::ptr::null_mut(),
        };
        drop_tmp(v_tmp);
        drop_tmp(s_tmp);
        out as u64
    }
}

/// NaN-box `undefined` bit pattern (torajs-anyvalue
/// `VALUE_UNDEFINED` mirror) — `s.at(oob)` answers JS undefined.
const VALUE_UNDEF_BOX: u64 = 0x0A;

/// `s.substring(start, end)` per ES §22.1.3.24 — negative clamps to
/// 0 and swapped bounds re-order in the kernel. The dispatcher
/// encodes a missing `end` as `i64::MAX` (clamps to `len`).
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_substring(s: *const u8, start: i64, end: i64) -> u64 {
    unsafe {
        let (src, tmp) = owned_src(s);
        let out = crate::transform::construct::__torajs_str_substring(src, start, end);
        drop_tmp(tmp);
        out as u64
    }
}

/// `s.substr(start, length)` per annexB §B.2.2.1 — negative `start`
/// wraps, `length` clamps to the remainder. Missing `length` rides
/// as `i64::MAX`.
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_substr(s: *const u8, start: i64, length: i64) -> u64 {
    unsafe {
        let (src, tmp) = owned_src(s);
        let out = crate::transform::construct::__torajs_str_substr(src, start, length);
        drop_tmp(tmp);
        out as u64
    }
}

/// `s.at(i)` per ES §22.1.3.1 — negative wraps in the kernel; OOB
/// answers the boxed JS `undefined` (the kernel's immortal undef-Str
/// sentinel translates to the immediate here — an any-world read
/// must not hand out the sentinel cell).
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_at(s: *const u8, i: i64) -> u64 {
    unsafe {
        let (src, tmp) = owned_src(s);
        let out = crate::transform::construct::__torajs_str_at(src, i);
        drop_tmp(tmp);
        if out == crate::undef_sentinel::undef_ptr() {
            VALUE_UNDEF_BOX
        } else {
            out as u64
        }
    }
}

/// `s.charCodeAt(i)` per ES §22.1.3.2 — the code unit, or `-1` for
/// out-of-range (the dispatcher boxes that as the spec NaN; an
/// integer sentinel rather than NaN itself because the `s[i]` index
/// lane wants the same "no such code unit" answer without going
/// through a float).
///
/// # Safety
/// `s` is a valid heap Str/Substr pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_char_code_at(s: *const u8, i: i64) -> i64 {
    unsafe {
        let (src, tmp) = owned_src(s);
        let out = crate::lookup_ffi::code_unit_at(src, i).map_or(-1, i64::from);
        drop_tmp(tmp);
        out
    }
}

/// `s.endsWith(needle, endPos)` per ES §22.1.3.7 — 1/0. The
/// dispatcher encodes a missing `endPos` as `i64::MAX` (the kernel
/// clamps to `s.length`).
///
/// # Safety
/// `s` and `needle` are valid heap Str/Substr pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_any_ends_with(
    s: *const u8,
    needle: *const u8,
    end: i64,
) -> i64 {
    unsafe {
        let (src, s_tmp) = owned_src(s);
        let (nn, n_tmp) = owned_src(needle);
        let out = __torajs_str_ends_with_from(src, nn, end);
        drop_tmp(n_tmp);
        drop_tmp(s_tmp);
        out
    }
}
