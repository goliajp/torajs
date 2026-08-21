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

use torajs_rc::HeapHeader;

use crate::block::StrBlock;
use crate::layout::{STR_FLAG_IS_LATIN1, STR_HDR_SIZE, STR_LEN_OFF};
use crate::substr::{SUBSTR_LEN_OFF, SUBSTR_OFFSET_OFF, SUBSTR_PARENT_OFF};

#[cfg(not(test))]
unsafe extern "C" {
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
}

#[cfg(test)]
unsafe extern "C" {
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
}

/// Read a Substr's parent encoding flag. The parent Str carries
/// the canonical encoding (Latin-1 if `IS_LATIN1` bit set on
/// `flags u16 @6`, else UTF-16 LE); the Substr never changes
/// encoding independently.
#[inline]
unsafe fn substr_parent_is_latin1(v: *const u8) -> bool {
    let parent = unsafe { substr_parent(v) } as *const HeapHeader;
    unsafe { (*parent).flags & STR_FLAG_IS_LATIN1 != 0 }
}

/// JS code-unit count of the view (post-P11.1-S5).
#[inline]
pub(crate) unsafe fn substr_len(v: *const u8) -> u64 {
    unsafe { *(v.add(SUBSTR_LEN_OFF) as *const u64) }
}

/// JS code-unit offset into the parent's payload (post-P11.1-S5).
#[inline]
pub(crate) unsafe fn substr_offset(v: *const u8) -> u64 {
    unsafe { *(v.add(SUBSTR_OFFSET_OFF) as *const u64) }
}

#[inline]
pub(crate) unsafe fn substr_parent(v: *const u8) -> *mut u8 {
    unsafe { *(v.add(SUBSTR_PARENT_OFF) as *const *mut u8) }
}

/// `(parent.bytes + cu_offset × parent_stride)` — pointer to the
/// first byte of the view. The byte address depends on the parent
/// encoding's stride (1 for Latin-1, 2 for UTF-16 LE).
#[inline]
unsafe fn substr_data(v: *const u8) -> *const u8 {
    let cu_off = unsafe { substr_offset(v) } as usize;
    let stride = if unsafe { substr_parent_is_latin1(v) } {
        1
    } else {
        2
    };
    unsafe { substr_parent(v).add(STR_HDR_SIZE + cu_off * stride) }
}

// `str_data` (single-line `s.add(STR_HDR_SIZE)`) was used by the
// pre-S2.5 byte-stream concat / search wrappers. Post-S2.5 every
// helper above resolves the data pointer via `str_view`, which
// returns a `&[u8]` already sized to `length × encoding stride`,
// so no caller still needs the bare pointer. Re-introduce on
// demand if a future helper wants the raw header-relative byte
// view.

/// Read a Str's `(payload, length, is_latin1)` view. `length` is
/// the ES code-unit count; `payload` covers
/// `length × (1 | 2)` bytes.
#[inline]
pub(crate) unsafe fn str_view<'a>(s: *const u8) -> (&'a [u8], u32, bool) {
    let length = unsafe { (s.add(STR_LEN_OFF) as *const u32).read() };
    let header = unsafe { &*(s as *const HeapHeader) };
    let is_latin1 = (header.flags & STR_FLAG_IS_LATIN1) != 0;
    let byte_cnt = if is_latin1 {
        length as usize
    } else {
        (length as usize) * 2
    };
    let payload = unsafe { core::slice::from_raw_parts(s.add(STR_HDR_SIZE), byte_cnt) };
    (payload, length, is_latin1)
}

/// Read a Substr's `(payload, cu_len, parent_is_latin1)` view.
/// `cu_len` is the JS code-unit count (post-P11.1-S5);
/// `payload` covers `cu_len × parent_stride` bytes starting at the
/// view's first byte.
#[inline]
pub(crate) unsafe fn substr_view<'a>(v: *const u8) -> (&'a [u8], usize, bool) {
    let cu_len = unsafe { substr_len(v) } as usize;
    let is_latin1 = unsafe { substr_parent_is_latin1(v) };
    let stride = if is_latin1 { 1 } else { 2 };
    let byte_len = cu_len * stride;
    let payload = unsafe { core::slice::from_raw_parts(substr_data(v), byte_len) };
    (payload, cu_len, is_latin1)
}

/// Widen a Latin-1 byte payload to a UTF-16 LE byte buffer (each
/// input byte becomes a `(byte, 0)` u16 pair). Same shape as the
/// `lookup.rs` widening helper.
fn widen_latin1_to_utf16(src: &[u8]) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec::Vec::with_capacity(src.len() * 2);
    for &b in src {
        out.push(b);
        out.push(0);
    }
    out
}

/// Cow-shaped wrapper so the search helpers can return either a
/// borrowed needle (same-encoding fast path) or an owned widened
/// buffer (Latin-1 needle widened to UTF-16) behind one byte-slice
/// API.
enum NeedleBuf<'a> {
    Borrowed(&'a [u8]),
    Owned(alloc::vec::Vec<u8>),
}

impl<'a> AsRef<[u8]> for NeedleBuf<'a> {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Borrowed(s) => s,
            Self::Owned(v) => v.as_slice(),
        }
    }
}

/// Align (Substr haystack, Str needle) to a common encoding for
/// byte-aligned scanning, or report "impossible match" via `None`.
/// Mirrors `lookup.rs::align_haystack_needle`: identical encoding
/// → borrow both verbatim; Latin-1 haystack + UTF-16 needle →
/// canonical short-circuit `None`; UTF-16 haystack + Latin-1
/// needle → widen needle in-place.
fn align_substr_needle<'h, 'n>(
    haystack: &'h [u8],
    haystack_latin1: bool,
    needle: &'n [u8],
    needle_latin1: bool,
) -> Option<(&'h [u8], NeedleBuf<'n>, usize)> {
    match (haystack_latin1, needle_latin1) {
        (true, true) => Some((haystack, NeedleBuf::Borrowed(needle), 1)),
        (false, false) => Some((haystack, NeedleBuf::Borrowed(needle), 2)),
        (true, false) => None,
        (false, true) => Some((haystack, NeedleBuf::Owned(widen_latin1_to_utf16(needle)), 2)),
    }
}

/// `s.charCodeAt(i)` on a Substr receiver, per ES §22.1.3.2 — the
/// code unit at `i` as a Number, or **NaN** when `i` is out of
/// range (step 5). Mirrors the Str kernel's `f64` ABI; see
/// [`crate::lookup_ffi::__torajs_str_char_code_at`] for why the
/// out-of-range answer cannot ride an integer.
///
/// P11.1-S2.5 — encoding-aware: Latin-1 returns the byte value
/// (0..=255), UTF-16 returns the little-endian u16 at code-unit
/// index `i`. `i` is the JS code-unit index per spec; the byte
/// offset is computed via the parent encoding's stride.
///
/// # Safety
/// `v` is a live `*const Substr` (rc > 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_char_code_at(v: *const u8, i: i64) -> f64 {
    unsafe { substr_code_unit_at(v, i) }.map_or(f64::NAN, f64::from)
}

/// The in-range half of [`__torajs_substr_char_code_at`]: the code
/// unit at `i`, or `None` when `i` is out of range.
///
/// # Safety
/// `v` is a live `*const Substr` (rc > 0).
pub(crate) unsafe fn substr_code_unit_at(v: *const u8, i: i64) -> Option<u16> {
    let cu_len = unsafe { substr_len(v) };
    let is_latin1 = unsafe { substr_parent_is_latin1(v) };
    let stride = if is_latin1 { 1u64 } else { 2u64 };
    if i < 0 || (i as u64) >= cu_len {
        return None;
    }
    let off = (i as u64) * stride;
    let p = unsafe { substr_data(v).add(off as usize) };
    if is_latin1 {
        Some(unsafe { *p } as u16)
    } else {
        let lo = unsafe { *p } as u16;
        let hi = unsafe { *p.add(1) } as u16;
        Some((hi << 8) | lo)
    }
}

/// `Substr === Str` content compare. Returns 1 iff the view's
/// code units equal the Str's.
///
/// P11.1-S2.5 Round 2 — encoding-aware; same-encoding operands
/// byte-compare over the shared payload stride. Mixed encodings
/// compare per code unit (chunk A2): a view inherits its parent's
/// encoding flag and cannot narrow, so a UTF-16 view can carry
/// all-Latin-1 content that genuinely equals a canonical Latin-1
/// Str.
///
/// # Safety
/// `v` is a live `*const Substr`, `s` is a live `*const Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_eq_str(v: *const u8, s: *const u8) -> i64 {
    // RFC 20260707 chunk 2 (+ residual chunk) — nullish operands
    // (NULL, the Str sentinel, or the Substr-shaped sentinel)
    // compare by identity, never content (see eq.rs). `undefined`
    // equals `undefined` ACROSS reprs: a materialized Substr
    // sentinel (substr_to_owned answers the Str cell) must still
    // equal a Substr-slot sentinel.
    let v_undef = crate::undef_sentinel::is_undef(v) || crate::undef_sentinel::is_substr_undef(v);
    let s_undef = crate::undef_sentinel::is_undef(s) || crate::undef_sentinel::is_substr_undef(s);
    if v.is_null() || s.is_null() || v_undef || s_undef {
        return (v == s || (v_undef && s_undef)) as i64;
    }
    let (v_payload, _v_cu_len, v_latin1) = unsafe { substr_view(v) };
    let (s_payload, _, s_latin1) = unsafe { str_view(s) };
    if v_latin1 == s_latin1 {
        if v_payload.len() != s_payload.len() {
            return 0;
        }
        return (v_payload == s_payload) as i64;
    }
    // Mixed encoding — a view inherits its parent's encoding and
    // cannot narrow, so a UTF-16 view holding all-Latin-1 content
    // (`"\u{6C49}abc".slice(1)`) legitimately compares against a
    // canonical Latin-1 Str. Per-code-unit walk (RFC
    // 20260712-string-proto-cluster chunk A2; the pre-A2 encoding
    // short-circuit answered 0).
    let (narrow, wide) = if v_latin1 {
        (v_payload, s_payload)
    } else {
        (s_payload, v_payload)
    };
    if narrow.len() * 2 != wide.len() {
        return 0;
    }
    for (i, &c) in narrow.iter().enumerate() {
        if u16::from_le_bytes([wide[2 * i], wide[2 * i + 1]]) != c as u16 {
            return 0;
        }
    }
    1
}

/// Materialize a Substr into a fresh OWNED Str (for crossing fn-call
/// boundaries that expect `Type::Str` — Phase Substr.B).
///
/// P11.1-S2.5 — encoding-aware: the result Str carries the parent's
/// encoding flag, so downstream `str_print` / `concat` / etc see a
/// canonical-encoded Str. `length` is derived from the Substr's
/// byte count via the parent encoding stride (1 for Latin-1, 2
/// for UTF-16). Pre-S2 the result was always Latin-1 byte-stream,
/// which caused `console.log(s.charAt(1))` to print garbage when
/// `s` was UTF-16.
///
/// # Safety
/// `v` is a live `*const Substr`. Returned pointer is a pooled Str
/// (rc=1).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_to_owned(v: *const u8) -> *mut c_void {
    // The undefined sentinel materializes as the Str sentinel —
    // identity propagates across the repr change so every Str-lane
    // consumer (eq / json / typeof / nullish probes) keeps seeing
    // `undefined`, not a fresh "undefined" text copy.
    if crate::undef_sentinel::is_substr_undef(v) {
        return crate::undef_sentinel::undef_ptr() as *mut c_void;
    }
    let length = unsafe { substr_len(v) } as u32;
    let is_latin1 = unsafe { substr_parent_is_latin1(v) };
    let stride: u32 = if is_latin1 { 1 } else { 2 };
    let byte_len = length * stride;
    let mut block = StrBlock::alloc_with_encoding(length, is_latin1);
    if byte_len > 0 {
        let dst = unsafe { block.as_bytes_mut(byte_len) };
        let src = unsafe { core::slice::from_raw_parts(substr_data(v), byte_len as usize) };
        dst.copy_from_slice(src);
    }
    block.into_raw() as *mut c_void
}

/// `substr.startsWith(needle: Str)` — view-aware.
///
/// # Safety
/// `v` is a live `*const Substr`, `n` is a live `*const Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_starts_with(v: *const u8, n: *const u8) -> i8 {
    let (v_payload, _, v_latin1) = unsafe { substr_view(v) };
    let (n_payload, n_len, n_latin1) = unsafe { str_view(n) };
    if n_len == 0 {
        return 1;
    }
    let Some((haystack, needle, _stride)) =
        align_substr_needle(v_payload, v_latin1, n_payload, n_latin1)
    else {
        return 0;
    };
    let needle = needle.as_ref();
    if needle.len() > haystack.len() {
        return 0;
    }
    if &haystack[..needle.len()] == needle {
        1
    } else {
        0
    }
}

/// `substr.endsWith(needle: Str)` — view-aware.
///
/// # Safety
/// `v` is a live `*const Substr`, `n` is a live `*const Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_ends_with(v: *const u8, n: *const u8) -> i8 {
    let (v_payload, _, v_latin1) = unsafe { substr_view(v) };
    let (n_payload, n_len, n_latin1) = unsafe { str_view(n) };
    if n_len == 0 {
        return 1;
    }
    let Some((haystack, needle, _stride)) =
        align_substr_needle(v_payload, v_latin1, n_payload, n_latin1)
    else {
        return 0;
    };
    let needle = needle.as_ref();
    if needle.len() > haystack.len() {
        return 0;
    }
    let tail_start = haystack.len() - needle.len();
    if &haystack[tail_start..] == needle {
        1
    } else {
        0
    }
}

/// `substr.includes(needle: Str)`.
///
/// # Safety
/// `v` is a live `*const Substr`, `n` is a live `*const Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_includes(v: *const u8, n: *const u8) -> i8 {
    let (v_payload, _, v_latin1) = unsafe { substr_view(v) };
    let (n_payload, n_len, n_latin1) = unsafe { str_view(n) };
    if n_len == 0 {
        return 1;
    }
    let Some((haystack, needle, stride)) =
        align_substr_needle(v_payload, v_latin1, n_payload, n_latin1)
    else {
        return 0;
    };
    let needle = needle.as_ref();
    if needle.len() > haystack.len() {
        return 0;
    }
    let end = haystack.len() - needle.len();
    let mut i = 0usize;
    while i <= end {
        if &haystack[i..i + needle.len()] == needle {
            return 1;
        }
        i += stride;
    }
    0
}

/// `substr.indexOf(needle: Str)` — `-1` on miss; `0` when needle empty.
///
/// # Safety
/// `v` is a live `*const Substr`, `n` is a live `*const Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_index_of(v: *const u8, n: *const u8) -> i64 {
    let (v_payload, _, v_latin1) = unsafe { substr_view(v) };
    let (n_payload, n_len, n_latin1) = unsafe { str_view(n) };
    if n_len == 0 {
        return 0;
    }
    let Some((haystack, needle, stride)) =
        align_substr_needle(v_payload, v_latin1, n_payload, n_latin1)
    else {
        return -1;
    };
    let needle = needle.as_ref();
    if needle.len() > haystack.len() {
        return -1;
    }
    let end = haystack.len() - needle.len();
    let mut i = 0usize;
    while i <= end {
        if &haystack[i..i + needle.len()] == needle {
            return (i / stride) as i64;
        }
        i += stride;
    }
    -1
}
