//! Lookup-family FFI shims — `__torajs_str_*` extern "C" wrappers
//! for the toolchain-emitted call sites: `startsWith` / `endsWith` /
//! `indexOf` / `includes` / `lastIndexOf` / `charCodeAt` /
//! `localeCompare`. Each shim is a thin "decode Str view → call
//! pure-byte op in `lookup.rs` → encode result". Extracted from
//! `lookup.rs` to keep that file under the 500-prod-LOC file-size
//! hard limit (`rules/common/file-size.md`); pure mechanical pull,
//! no semantic change.

use core::cmp::Ordering;

use crate::eq::resolve_payload;
use crate::lookup::{
    align_haystack_needle, clamp_from_to_byte_off, code_unit_compare, index_of_with_stride,
    last_index_of_with_stride, str_len, str_view,
};

/// `s.localeCompare(other)` — three-way ordinal compare, returns
/// `-1` / `0` / `+1`. P11.1-S2.4 stub uses plain byte-ordering on
/// the canonical encoding; locale-aware Unicode collation lands
/// once the normalisation tables come online in P11.5 / P11.6.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_locale_compare(a: *const u8, b: *const u8) -> i64 {
    // Either operand may be a substring VIEW sharing Tag::Str (a
    // split-product slot, `s[i]`, `s.slice(..)`); read each by its
    // own flags, and compare code units, not bytes (see
    // `code_unit_compare`).
    let (aa, a_latin1) = unsafe { resolve_payload(a) };
    let (bb, b_latin1) = unsafe { resolve_payload(b) };
    match code_unit_compare(aa, a_latin1, bb, b_latin1) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// ES §23.1.3.30.2 SortCompare steps 5-8 pre-probe for the USER-
/// comparator lane: undefined elements never reach the comparator —
/// they sort last unconditionally. Returns the SortCompare result
/// (`1` / `-1` / `0`) when either side is undefined (either sentinel
/// repr), or `2` (no undefined — proceed to the comparator call).
/// NULL is JS `null`, an ordinary comparator argument.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_str_sort_undef_pre(a: *const u8, b: *const u8) -> i64 {
    let a_undef = crate::undef_sentinel::is_undef(a) || crate::undef_sentinel::is_substr_undef(a);
    let b_undef = crate::undef_sentinel::is_undef(b) || crate::undef_sentinel::is_substr_undef(b);
    if a_undef || b_undef {
        (a_undef as i64) - (b_undef as i64)
    } else {
        2
    }
}

/// ES §23.1.3.30.2 SortCompare for two Str-slot elements on the
/// default-comparator lane: `undefined` sorts LAST — the check
/// happens BEFORE ToString (steps 5-8), so the sentinel never
/// content-compares as the text "undefined" (RFC 20260707
/// residual). A NULL slot is JS `null`, which ToStrings to "null"
/// and participates normally.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_sort_cmp(a: *const u8, b: *const u8) -> i64 {
    let a_undef = crate::undef_sentinel::is_undef(a) || crate::undef_sentinel::is_substr_undef(a);
    let b_undef = crate::undef_sentinel::is_undef(b) || crate::undef_sentinel::is_substr_undef(b);
    if a_undef || b_undef {
        return (a_undef as i64) - (b_undef as i64);
    }
    // A `Str`-tagged slot can hold a substring VIEW (a split product
    // sorted in place); read each operand by its own flags. Reading a
    // view by the owned layout compared its parent pointer and offset
    // as text and answered `cba` for `"c b a".split(" ").sort()`
    // (rotation 468). Then compare code units, not bytes.
    let (aa, a_latin1) = if a.is_null() {
        (&b"null"[..], true)
    } else {
        unsafe { resolve_payload(a) }
    };
    let (bb, b_latin1) = if b.is_null() {
        (&b"null"[..], true)
    } else {
        unsafe { resolve_payload(b) }
    };
    match code_unit_compare(aa, a_latin1, bb, b_latin1) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// `s.startsWith(needle, pos)` — 1 if matches, 0 otherwise.
///
/// P11.1-S2.4 — encoding-aware: empty needle always matches;
/// mismatched encoding flag short-circuits to false under the
/// canonical-encoding invariant; same encoding does a stride-
/// aligned byte compare at `pos × stride`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_starts_with_from(
    s: *const u8,
    sub: *const u8,
    pos: i64,
) -> i64 {
    let (ss, s_len, s_latin1) = unsafe { str_view(s) };
    let (nn, n_len, n_latin1) = unsafe { str_view(sub) };
    if n_len == 0 {
        return 1;
    }
    let Some((haystack, needle, stride)) = align_haystack_needle(ss, s_latin1, nn, n_latin1) else {
        return 0;
    };
    let start = clamp_from_to_byte_off(pos, s_len, stride);
    if start + needle.len() > haystack.len() {
        return 0;
    }
    if &haystack[start..start + needle.len()] == needle.as_ref() {
        1
    } else {
        0
    }
}

/// `s.endsWith(needle, end)` — 1 if matches, 0 otherwise. `end`
/// is the JS code-unit anchor (clamped to `[0, s.length]`); the
/// match window is `s[end - needle.length .. end]`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_ends_with_from(
    s: *const u8,
    sub: *const u8,
    end: i64,
) -> i64 {
    let (ss, s_len, s_latin1) = unsafe { str_view(s) };
    let (nn, n_len, n_latin1) = unsafe { str_view(sub) };
    if n_len == 0 {
        return 1;
    }
    let Some((haystack, needle, stride)) = align_haystack_needle(ss, s_latin1, nn, n_latin1) else {
        return 0;
    };
    let e_byte = clamp_from_to_byte_off(end, s_len, stride);
    if e_byte < needle.len() {
        return 0;
    }
    let off = e_byte - needle.len();
    if &haystack[off..e_byte] == needle.as_ref() {
        1
    } else {
        0
    }
}

/// `s.indexOf(needle, fromIdx)` — found code-unit index or `-1`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_index_of_from(
    s: *const u8,
    sub: *const u8,
    from: i64,
) -> i64 {
    let (ss, s_len, s_latin1) = unsafe { str_view(s) };
    let (nn, _, n_latin1) = unsafe { str_view(sub) };
    if nn.is_empty() {
        return from.max(0).min(s_len as i64);
    }
    let Some((haystack, needle, stride)) = align_haystack_needle(ss, s_latin1, nn, n_latin1) else {
        return -1;
    };
    let start = clamp_from_to_byte_off(from, s_len, stride);
    match index_of_with_stride(haystack, needle.as_ref(), start, stride) {
        Some(byte_off) => (byte_off / stride) as i64,
        None => -1,
    }
}

/// `s.includes(needle, fromIdx)` — 1 if found, 0 otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_includes_from(
    s: *const u8,
    sub: *const u8,
    from: i64,
) -> i64 {
    let (ss, s_len, s_latin1) = unsafe { str_view(s) };
    let (nn, _, n_latin1) = unsafe { str_view(sub) };
    if nn.is_empty() {
        return 1;
    }
    let Some((haystack, needle, stride)) = align_haystack_needle(ss, s_latin1, nn, n_latin1) else {
        return 0;
    };
    let start = clamp_from_to_byte_off(from, s_len, stride);
    if index_of_with_stride(haystack, needle.as_ref(), start, stride).is_some() {
        1
    } else {
        0
    }
}

/// `s.lastIndexOf(needle, fromIdx)` — found code-unit index or `-1`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_last_index_of_from(
    s: *const u8,
    sub: *const u8,
    from: i64,
) -> i64 {
    let (ss, s_len, s_latin1) = unsafe { str_view(s) };
    let (nn, _, n_latin1) = unsafe { str_view(sub) };
    if nn.is_empty() {
        return from.max(0).min(s_len as i64);
    }
    let Some((haystack, needle, stride)) = align_haystack_needle(ss, s_latin1, nn, n_latin1) else {
        return -1;
    };
    let cap = clamp_from_to_byte_off(from, s_len, stride);
    match last_index_of_with_stride(haystack, needle.as_ref(), cap, stride) {
        Some(byte_off) => (byte_off / stride) as i64,
        None => -1,
    }
}

/// `s.lastIndexOf(needle)` — found code-unit index or `-1`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_last_index_of(s: *const u8, needle: *const u8) -> i64 {
    let (ss, s_len, s_latin1) = unsafe { str_view(s) };
    let (nn, _, n_latin1) = unsafe { str_view(needle) };
    if nn.is_empty() {
        return s_len as i64;
    }
    let Some((haystack, needle, stride)) = align_haystack_needle(ss, s_latin1, nn, n_latin1) else {
        return -1;
    };
    let cap = haystack.len().saturating_sub(needle.len());
    match last_index_of_with_stride(haystack, needle.as_ref(), cap, stride) {
        Some(byte_off) => (byte_off / stride) as i64,
        None => -1,
    }
}

// No-`_from` 2-arg wrappers — port of the formerly IR-emitted
// `define_str_{prefix_suffix_check,index_of,includes}` builders in
// ssa_inkwell. Each is a thin call onto the corresponding `_from`
// core; default `pos` is 0 for the search-from-start family and
// `s.len()` for `ends_with` (the natural "scan to end" anchor).

/// `s.startsWith(needle)` — equivalent to `starts_with_from(s, n, 0)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_starts_with(s: *const u8, n: *const u8) -> i64 {
    unsafe { __torajs_str_starts_with_from(s, n, 0) }
}

/// `s.endsWith(needle)` — equivalent to `ends_with_from(s, n, s.len())`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_ends_with(s: *const u8, n: *const u8) -> i64 {
    let s_len = unsafe { str_len(s) } as i64;
    unsafe { __torajs_str_ends_with_from(s, n, s_len) }
}

/// `s.indexOf(needle)` — equivalent to `index_of_from(s, n, 0)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_index_of(s: *const u8, n: *const u8) -> i64 {
    unsafe { __torajs_str_index_of_from(s, n, 0) }
}

/// `s.includes(needle)` — equivalent to `includes_from(s, n, 0)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_includes(s: *const u8, n: *const u8) -> i64 {
    unsafe { __torajs_str_includes_from(s, n, 0) }
}

/// `s.charCodeAt(i)` per ES §22.1.3.2 — the UTF-16 code unit at
/// index `i` as a Number, or **NaN** when `i` is out of range
/// (step 5). NaN is why the ABI is `f64` and not `i64`: the
/// out-of-range answer is not representable as an integer, and the
/// pre-r464 `0` made `"abc".charCodeAt(9)` collide with a real
/// NUL code unit.
///
/// P11.1-S2.4 — encoding-aware: Latin-1 returns the byte value
/// (0..=255 maps 1:1 to a code unit), UTF-16 returns the little-
/// endian u16 at byte offset `i × 2`. Lone surrogates are returned
/// as-is per ES spec.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_char_code_at(s: *const u8, i: i64) -> f64 {
    unsafe { code_unit_at(s, i) }.map_or(f64::NAN, f64::from)
}

/// The in-range half of [`__torajs_str_char_code_at`]: the code unit
/// at `i`, or `None` when `i` is out of range. Callers that carry
/// their own out-of-range answer (`-1` for the Any-tier glue,
/// `None` for the `s[i]` index lane) read this directly rather than
/// round-tripping through NaN.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
pub(crate) unsafe fn code_unit_at(s: *const u8, i: i64) -> Option<u16> {
    let (payload, length, is_latin1) = unsafe { str_view(s) };
    if i < 0 || i >= length as i64 {
        return None;
    }
    if is_latin1 {
        Some(payload[i as usize] as u16)
    } else {
        let off = (i as usize) * 2;
        let lo = payload[off] as u16;
        let hi = payload[off + 1] as u16;
        Some((hi << 8) | lo)
    }
}
