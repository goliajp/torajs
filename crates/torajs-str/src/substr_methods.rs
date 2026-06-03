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

use torajs_rc::{__torajs_rc_inc, HeapHeader};

use crate::block::StrBlock;
use crate::layout::{STR_FLAG_IS_LATIN1, STR_HDR_SIZE, STR_LEN_OFF};
use crate::substr::{
    __torajs_substr_create, FLAG_SUBSTR_INLINE, SUBSTR_LEN_OFF, SUBSTR_OFFSET_OFF,
    SUBSTR_PARENT_OFF,
};

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
unsafe fn substr_len(v: *const u8) -> u64 {
    unsafe { *(v.add(SUBSTR_LEN_OFF) as *const u64) }
}

/// JS code-unit offset into the parent's payload (post-P11.1-S5).
#[inline]
unsafe fn substr_offset(v: *const u8) -> u64 {
    unsafe { *(v.add(SUBSTR_OFFSET_OFF) as *const u64) }
}

#[inline]
unsafe fn substr_parent(v: *const u8) -> *mut u8 {
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
unsafe fn str_view<'a>(s: *const u8) -> (&'a [u8], u32, bool) {
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
unsafe fn substr_view<'a>(v: *const u8) -> (&'a [u8], usize, bool) {
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

/// `s.charCodeAt(i)` on a Substr receiver. OOB / negative returns 0.
///
/// P11.1-S2.5 — encoding-aware: Latin-1 returns the byte value
/// (0..=255), UTF-16 returns the little-endian u16 at code-unit
/// index `i`. `i` is the JS code-unit index per spec; the byte
/// offset is computed via the parent encoding's stride. The
/// Substr's `len@8` field is a byte count over the parent's
/// payload (S5 follow-up converts it to code units); the bounds
/// check converts to code units via the stride.
///
/// # Safety
/// `v` is a live `*const Substr` (rc > 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_char_code_at(v: *const u8, i: i64) -> i64 {
    let cu_len = unsafe { substr_len(v) };
    let is_latin1 = unsafe { substr_parent_is_latin1(v) };
    let stride = if is_latin1 { 1u64 } else { 2u64 };
    if i < 0 || (i as u64) >= cu_len {
        return 0;
    }
    let off = (i as u64) * stride;
    let p = unsafe { substr_data(v).add(off as usize) };
    if is_latin1 {
        unsafe { *p as i64 }
    } else {
        let lo = unsafe { *p } as u16;
        let hi = unsafe { *p.add(1) } as u16;
        ((hi << 8) | lo) as i64
    }
}

/// `Substr === Str` content compare. Returns 1 iff the view's
/// code units equal the Str's, under the canonical-encoding
/// invariant.
///
/// P11.1-S2.5 Round 2 — encoding-aware. Same canonical
/// short-circuit as `__torajs_str_eq`: a Substr inherits its
/// parent's encoding flag, so a Substr with parent-Latin-1 can
/// never equal a UTF-16 Str (and vice versa). When encodings
/// match, the comparison is a byte-equal over the shared payload
/// stride.
///
/// # Safety
/// `v` is a live `*const Substr`, `s` is a live `*const Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_eq_str(v: *const u8, s: *const u8) -> i64 {
    let (v_payload, _v_cu_len, v_latin1) = unsafe { substr_view(v) };
    let (s_payload, _, s_latin1) = unsafe { str_view(s) };
    if v_latin1 != s_latin1 {
        return 0;
    }
    if v_payload.len() != s_payload.len() {
        return 0;
    }
    if v_payload.is_empty() {
        return 1;
    }
    if v_payload == s_payload { 1 } else { 0 }
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

/// Build a result Str holding `(a_payload, a_latin1)` followed by
/// `(b_payload, b_latin1)` under the widest-of-inputs encoding,
/// widening the Latin-1 side to UTF-16 LE when the encodings
/// disagree. Mirrors `concat.rs::__torajs_str_concat` for the
/// `(substr, str)` / `(str, substr)` / `(substr, substr)` mixes.
fn build_concat_result(
    a_payload: &[u8],
    a_latin1: bool,
    b_payload: &[u8],
    b_latin1: bool,
) -> *mut c_void {
    let out_latin1 = a_latin1 && b_latin1;
    let stride: u32 = if out_latin1 { 1 } else { 2 };
    let a_byte_cnt = if a_latin1 == out_latin1 {
        a_payload.len()
    } else {
        a_payload.len() * 2
    };
    let b_byte_cnt = if b_latin1 == out_latin1 {
        b_payload.len()
    } else {
        b_payload.len() * 2
    };
    let total_byte_cnt = a_byte_cnt + b_byte_cnt;
    let length = (total_byte_cnt as u32) / stride;
    let mut block = StrBlock::alloc_with_encoding(length, out_latin1);
    if total_byte_cnt == 0 {
        return block.into_raw() as *mut c_void;
    }
    let dst = unsafe { block.as_bytes_mut(total_byte_cnt as u32) };
    if !a_payload.is_empty() {
        if a_latin1 == out_latin1 {
            dst[..a_byte_cnt].copy_from_slice(a_payload);
        } else {
            // a is Latin-1, out is UTF-16 — widen.
            for (i, &b) in a_payload.iter().enumerate() {
                dst[i * 2] = b;
                dst[i * 2 + 1] = 0;
            }
        }
    }
    if !b_payload.is_empty() {
        let b_slot = &mut dst[a_byte_cnt..a_byte_cnt + b_byte_cnt];
        if b_latin1 == out_latin1 {
            b_slot.copy_from_slice(b_payload);
        } else {
            for (i, &b) in b_payload.iter().enumerate() {
                b_slot[i * 2] = b;
                b_slot[i * 2 + 1] = 0;
            }
        }
    }
    block.into_raw() as *mut c_void
}

/// `(substr + str)` — single-alloc view-aware concat.
///
/// P11.1-S2.5 Round 2 — encoding-aware via `build_concat_result`.
///
/// # Safety
/// `v` is a live `*const Substr`, `s` is a live `*const Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_concat_substr_str(
    v: *const u8,
    s: *const u8,
) -> *mut c_void {
    let (v_payload, _, v_latin1) = unsafe { substr_view(v) };
    let (s_payload, _, s_latin1) = unsafe { str_view(s) };
    build_concat_result(v_payload, v_latin1, s_payload, s_latin1)
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
    let (s_payload, _, s_latin1) = unsafe { str_view(s) };
    let (v_payload, _, v_latin1) = unsafe { substr_view(v) };
    build_concat_result(s_payload, s_latin1, v_payload, v_latin1)
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
    let (a_payload, _, a_latin1) = unsafe { substr_view(a) };
    let (b_payload, _, b_latin1) = unsafe { substr_view(b) };
    build_concat_result(a_payload, a_latin1, b_payload, b_latin1)
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

/// `substr.slice(start, end)` — view-of-view. Negative indices wrap;
/// `start > end` clamps to empty.
///
/// `start` / `end` are JS code-unit indices. Post-P11.1-S5 both the
/// view's `(offset, len)` and `__torajs_substr_create`'s args are
/// code units, so no byte/stride conversion is needed here.
///
/// # Safety
/// `v` is a live `*const Substr`. Returned pointer is a fresh
/// Substr (rc=1) referencing the SAME root parent.
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

#[inline]
fn substr_is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

/// Compute trim bounds for Substr `v` and return the
/// `(parent, code_unit_offset, code_unit_len)` triple that the
/// `_trim_*` family hands to [`__torajs_substr_create`] / writes
/// into the `_trim_into` stack buffer. `do_start` / `do_end` toggle
/// which sides scan (`_trim_start` skips end, `_trim_end` skips
/// start, `_trim` does both).
///
/// Encoding-aware (P11.1-S5): Latin-1 = byte-stride scan; UTF-16 LE
/// = u16-stride scan with high-byte-zero + low-byte-in-WS predicate.
/// Byte indices from the scan are divided by the parent's stride to
/// yield code-unit `(lo, hi)`.
#[inline]
unsafe fn trim_cu_bounds(v: *const u8, do_start: bool, do_end: bool) -> (*mut c_void, u64, u64) {
    let (payload, _cu_len, is_latin1) = unsafe { substr_view(v) };
    let v_off = unsafe { substr_offset(v) };
    let parent = unsafe { substr_parent(v) } as *mut c_void;
    let byte_len = payload.len();
    let (lo_byte, hi_byte) = if is_latin1 {
        let mut lo = 0usize;
        if do_start {
            while lo < byte_len && substr_is_ws(payload[lo]) {
                lo += 1;
            }
        }
        let mut hi = byte_len;
        if do_end {
            while hi > lo && substr_is_ws(payload[hi - 1]) {
                hi -= 1;
            }
        }
        (lo, hi)
    } else {
        let mut lo = 0usize;
        if do_start {
            while lo + 1 < byte_len && payload[lo + 1] == 0 && substr_is_ws(payload[lo]) {
                lo += 2;
            }
        }
        let mut hi = byte_len;
        if do_end {
            while hi >= lo + 2 && payload[hi - 1] == 0 && substr_is_ws(payload[hi - 2]) {
                hi -= 2;
            }
        }
        (lo, hi)
    };
    let stride = if is_latin1 { 1 } else { 2 };
    let cu_lo = (lo_byte / stride) as u64;
    let cu_hi = (hi_byte / stride) as u64;
    (parent, v_off + cu_lo, cu_hi - cu_lo)
}

/// `substr.trim()` — narrow leading + trailing ASCII whitespace.
/// Encoding-aware via [`trim_cu_bounds`]; the returned Substr
/// references the SAME root parent.
///
/// # Safety
/// `v` is a live `*const Substr`. Returned pointer is a fresh
/// Substr (rc=1).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_trim(v: *const u8) -> *mut c_void {
    let (parent, off, len) = unsafe { trim_cu_bounds(v, true, true) };
    unsafe { __torajs_substr_create(parent, off, len) }
}

/// `substr.trimStart()` — encoding-aware via [`trim_cu_bounds`].
///
/// # Safety
/// See [`__torajs_substr_trim`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_trim_start(v: *const u8) -> *mut c_void {
    let (parent, off, len) = unsafe { trim_cu_bounds(v, true, false) };
    unsafe { __torajs_substr_create(parent, off, len) }
}

/// `substr.trimEnd()` — encoding-aware via [`trim_cu_bounds`].
///
/// # Safety
/// See [`__torajs_substr_trim`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_trim_end(v: *const u8) -> *mut c_void {
    let (parent, off, len) = unsafe { trim_cu_bounds(v, false, true) };
    unsafe { __torajs_substr_create(parent, off, len) }
}

/// Stack-write variant of [`__torajs_substr_trim`] — writes the
/// trimmed view into a caller-provided 32-byte buffer instead of
/// heap-allocating a fresh `SubstrBlock`. The buffer carries
/// [`FLAG_SUBSTR_INLINE`] so a subsequent `__torajs_substr_drop`
/// follows the INLINE branch (dec parent rc only, no pool push,
/// no free); the symmetric `rc_inc` is performed here.
///
/// P11.1-S5 — `len@8` / `offset@24` are written in JS code units
/// via [`trim_cu_bounds`].
///
/// # Safety
///
/// `v` must be a live `*const Substr`. `out_buf` must be a writable
/// 32-byte aligned region (typically a caller `alloca [32 x i8]`).
/// Caller MUST eventually invoke `__torajs_substr_drop(out_buf)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_trim_into(v: *const u8, out_buf: *mut u8) {
    let (parent, off, len) = unsafe { trim_cu_bounds(v, true, true) };
    // Header u64: refcount=0 @0..4, type_tag=Tag::Str=0 @4..6,
    // flags=FLAG_SUBSTR_INLINE @6..8 packed little-endian.
    let header_u64 = (FLAG_SUBSTR_INLINE as u64) << 48;
    unsafe {
        (out_buf as *mut u64).write(header_u64);
        (out_buf.add(SUBSTR_LEN_OFF) as *mut u64).write(len);
        (out_buf.add(SUBSTR_PARENT_OFF) as *mut *mut c_void).write(parent);
        (out_buf.add(SUBSTR_OFFSET_OFF) as *mut u64).write(off);
    }
    unsafe { __torajs_rc_inc(parent) };
}
