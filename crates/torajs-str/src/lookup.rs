//! Read-only Str lookup operations + extern "C" wrappers.
//!
//! Seven FFI entry points, all pure functions over Str byte
//! payloads (no allocation, no refcount mutation):
//!
//! | extern "C"                                | role                        |
//! |-------------------------------------------|-----------------------------|
//! | [`__torajs_str_locale_compare`]            | `s.localeCompare(other)` (-1/0/1) |
//! | [`__torajs_str_starts_with_from`]          | `s.startsWith(needle, pos)`       |
//! | [`__torajs_str_ends_with_from`]            | `s.endsWith(needle, endPos)`      |
//! | [`__torajs_str_index_of_from`]             | `s.indexOf(needle, fromIdx)`      |
//! | [`__torajs_str_includes_from`]             | `s.includes(needle, fromIdx)`     |
//! | [`__torajs_str_last_index_of_from`]        | `s.lastIndexOf(needle, fromIdx)`  |
//! | [`__torajs_str_last_index_of`]             | `s.lastIndexOf(needle)`           |
//!
//! Each wrapper reads the two Str blocks' lengths + payload byte
//! slices via the `STR_LEN_OFF` / `STR_DATA_OFF` constants and
//! delegates to a pure-Rust core that returns `bool` / `Option
//! <usize>` / `Ordering`. The cores live alongside the wrappers so
//! Rust code can call them directly without going through extern
//! "C".
//!
//! The IR-side `__torajs_str_starts_with` / `_ends_with` /
//! `_index_of` / `_includes` (no `_from` suffix) emitted by
//! `ssa_inkwell::define_str_*` remain LLVM-IR until P3.1-g
//! consolidation; this module handles only the C-defined `*_from`
//! variants plus `_last_index_of` / `_locale_compare`.

use core::cmp::Ordering;

use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use torajs_rc::HeapHeader;

// ============================================================
// Layout-aware FFI helpers
// ============================================================

#[inline]
unsafe fn str_len(p: *const u8) -> u32 {
    unsafe { (p.add(STR_LEN_OFF) as *const u32).read() }
}

/// Read a Str's `(payload_bytes, length, is_latin1)` view. Length
/// is the ES code unit count; payload byte count = `length × (1
/// for Latin-1 | 2 for UTF-16)`. Used by every search FFI to
/// derive both the encoding short-circuit and the stride for
/// byte-aligned scanning.
///
/// # Safety
///
/// `p` must point at a valid Str block whose universal heap
/// header is intact.
#[inline]
pub(crate) unsafe fn str_view<'a>(p: *const u8) -> (&'a [u8], u32, bool) {
    let length = unsafe { (p.add(STR_LEN_OFF) as *const u32).read() };
    let header = unsafe { &*(p as *const HeapHeader) };
    let is_latin1 = (header.flags & STR_FLAG_IS_LATIN1) != 0;
    let byte_cnt = if is_latin1 {
        length as usize
    } else {
        (length as usize) * 2
    };
    let payload = unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), byte_cnt) };
    (payload, length, is_latin1)
}

/// Clamp a JS-side `from` code-unit index into the `[0, length]`
/// range and convert it to a byte offset using the encoding's
/// per-code-unit stride (1 for Latin-1, 2 for UTF-16).
#[inline]
fn clamp_from_to_byte_off(from: i64, length: u32, stride: usize) -> usize {
    let clamped = from.max(0).min(length as i64) as usize;
    clamped * stride
}

/// Encoding-aware forward substring search. Returns the byte
/// offset of the first occurrence of `needle` in `haystack[start..]`
/// stepping by `stride` (1 for Latin-1, 2 for UTF-16 LE so all
/// candidate positions are u16-aligned). Empty needle hits at
/// `start`.
fn index_of_with_stride(
    haystack: &[u8],
    needle: &[u8],
    start_byte: usize,
    stride: usize,
) -> Option<usize> {
    if needle.is_empty() {
        return Some(start_byte);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    let end = haystack.len() - needle.len();
    let mut i = start_byte;
    while i <= end {
        if &haystack[i..i + needle.len()] == needle {
            return Some(i);
        }
        i += stride;
    }
    None
}

/// Encoding-aware reverse substring search. Returns the byte
/// offset of the last occurrence of `needle` in
/// `haystack[..=cap_byte]` (i.e. matches whose start ≤ `cap_byte`),
/// stepping the candidate position down by `stride`. Empty
/// needle hits at `min(cap_byte, haystack.len())`.
fn last_index_of_with_stride(
    haystack: &[u8],
    needle: &[u8],
    cap_byte: usize,
    stride: usize,
) -> Option<usize> {
    if needle.is_empty() {
        return Some(cap_byte.min(haystack.len()));
    }
    if needle.len() > haystack.len() {
        return None;
    }
    let max_start = haystack.len() - needle.len();
    let mut i = cap_byte.min(max_start);
    // Snap i back to the closest stride-aligned position ≤ i. For
    // stride=1 this is a no-op; for stride=2 (UTF-16) odd `cap_byte`
    // already shouldn't happen (caller derived it from a code-unit
    // index × 2), but the AND keeps the loop tight against bad
    // inputs.
    i &= !(stride - 1);
    loop {
        if &haystack[i..i + needle.len()] == needle {
            return Some(i);
        }
        if i < stride {
            return None;
        }
        i -= stride;
    }
}

/// Widen a Latin-1 byte payload to a UTF-16 LE byte buffer (each
/// input byte becomes a `(byte, 0)` u16 pair). Used by the search
/// FFI wrappers when haystack is UTF-16 and needle is Latin-1:
/// every Latin-1 codepoint is also a valid BMP UTF-16 code unit
/// with zero high byte, so the search reduces to a byte-aligned
/// scan over the widened needle.
fn widen_latin1_to_utf16(src: &[u8]) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec::Vec::with_capacity(src.len() * 2);
    for &b in src {
        out.push(b);
        out.push(0);
    }
    out
}

/// Cow-style wrapper that lets the encoding-aware search helpers
/// return either a borrowed payload (same-encoding fast path) or
/// an owned widened buffer (Latin-1 needle widened to UTF-16) under
/// a single byte-slice API.
enum PayloadBuf<'a> {
    Borrowed(&'a [u8]),
    Owned(alloc::vec::Vec<u8>),
}

impl<'a> AsRef<[u8]> for PayloadBuf<'a> {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Borrowed(s) => s,
            Self::Owned(v) => v.as_slice(),
        }
    }
}

impl<'a> PayloadBuf<'a> {
    #[inline]
    fn len(&self) -> usize {
        self.as_ref().len()
    }
}

/// Bring haystack + needle to a common encoding for byte-aligned
/// scanning, or report "impossible match" when canonical invariants
/// rule it out.
///
/// Returns `Some((haystack_bytes, needle_buf, stride))` when a
/// byte-aligned scan is feasible, where `stride` is 1 for Latin-1
/// and 2 for UTF-16. Returns `None` only in the asymmetric case
/// **haystack=Latin-1, needle=UTF-16**: a UTF-16 needle implies a
/// codepoint > 0xFF that cannot occur in a Latin-1 haystack under
/// the canonical-encoding invariant — substring search is
/// definitely a miss.
///
/// The opposite asymmetry — Latin-1 needle inside a UTF-16
/// haystack — widens the needle to UTF-16 LE so its byte stream
/// aligns with the haystack's, exploiting the fact that every
/// Latin-1 codepoint is a valid BMP code unit with zero high
/// byte.
fn align_haystack_needle<'h, 'n>(
    haystack: &'h [u8],
    haystack_latin1: bool,
    needle: &'n [u8],
    needle_latin1: bool,
) -> Option<(&'h [u8], PayloadBuf<'n>, usize)> {
    match (haystack_latin1, needle_latin1) {
        (true, true) => Some((haystack, PayloadBuf::Borrowed(needle), 1)),
        (false, false) => Some((haystack, PayloadBuf::Borrowed(needle), 2)),
        (true, false) => None,
        (false, true) => Some((
            haystack,
            PayloadBuf::Owned(widen_latin1_to_utf16(needle)),
            2,
        )),
    }
}

// ============================================================
// Pure-Rust cores
// ============================================================

/// Bytewise comparison per `localeCompare`. ASCII-only `memcmp`
/// equivalent; the v0 implementation does NOT actually consult any
/// locale (matches the C original's comment).
pub fn locale_compare(a: &[u8], b: &[u8]) -> Ordering {
    a.cmp(b)
}

/// `s.startsWith(sub, pos)` — pos may be negative (clamped to 0) or
/// past `s.len()` (clamped to `s.len()`). Empty needle always
/// matches.
pub fn starts_with_from(s: &[u8], sub: &[u8], pos: i64) -> bool {
    let start = pos.max(0).min(s.len() as i64) as usize;
    if sub.is_empty() {
        return true;
    }
    if start + sub.len() > s.len() {
        return false;
    }
    &s[start..start + sub.len()] == sub
}

/// `s.endsWith(sub, end)` — end may be negative (clamped to 0) or
/// past `s.len()` (clamped to `s.len()`). Empty needle always
/// matches. The match window is `s[end - sub.len()..end]`.
pub fn ends_with_from(s: &[u8], sub: &[u8], end: i64) -> bool {
    let e = end.max(0).min(s.len() as i64) as usize;
    if sub.is_empty() {
        return true;
    }
    if e < sub.len() {
        return false;
    }
    let off = e - sub.len();
    &s[off..e] == sub
}

/// `s.indexOf(sub, from)` — forward scan starting at clamped `from`.
/// Returns `None` if the needle is not found (the C wrapper maps
/// this to `-1`). An empty needle matches at the start position
/// per ES spec.
pub fn index_of_from(s: &[u8], sub: &[u8], from: i64) -> Option<usize> {
    let start = from.max(0).min(s.len() as i64) as usize;
    if sub.is_empty() {
        return Some(start);
    }
    if sub.len() > s.len() {
        return None;
    }
    let end = s.len() - sub.len();
    for i in start..=end {
        if &s[i..i + sub.len()] == sub {
            return Some(i);
        }
    }
    None
}

/// `s.includes(sub, from)` — same scan as `indexOf` but returns
/// `bool`.
#[inline]
pub fn includes_from(s: &[u8], sub: &[u8], from: i64) -> bool {
    index_of_from(s, sub, from).is_some()
}

/// `s.lastIndexOf(sub, from)` — reverse scan, clamped `from`.
/// Empty needle clamps to `max(0, min(from, s.len()))`; non-empty
/// needle starts at `min(from, s.len() - sub.len())` and walks
/// backwards.
pub fn last_index_of_from(s: &[u8], sub: &[u8], from: i64) -> Option<usize> {
    let s_len = s.len() as i64;
    if sub.is_empty() {
        // Empty needle: clamp `from` into `[0, s.len()]`.
        let end = s_len;
        return Some(if from > end {
            end as usize
        } else if from < 0 {
            0
        } else {
            from as usize
        });
    }
    if sub.len() > s.len() {
        return None;
    }
    let max_i = (s.len() - sub.len()) as i64;
    let start = if from > max_i { max_i } else { from };
    if start < 0 {
        return None;
    }
    let mut i = start as i64;
    while i >= 0 {
        let idx = i as usize;
        if &s[idx..idx + sub.len()] == sub {
            return Some(idx);
        }
        i -= 1;
    }
    None
}

/// `s.lastIndexOf(needle)` — no-arg variant; equivalent to
/// `last_index_of_from(s, needle, s.len() as i64)` but with a
/// dedicated body so the empty-needle path returns `s.len()`
/// matching the spec.
pub fn last_index_of(s: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(s.len());
    }
    if needle.len() > s.len() {
        return None;
    }
    let mut i = (s.len() - needle.len()) as i64;
    while i >= 0 {
        let idx = i as usize;
        if &s[idx..idx + needle.len()] == needle {
            return Some(idx);
        }
        i -= 1;
    }
    None
}

// ============================================================
// extern "C" wrappers — preserve the pre-rewrite ABI bit-for-bit
// ============================================================

/// `s.localeCompare(other)` — returns -1, 0, or 1. Mirrors C
/// `__torajs_str_locale_compare`.
///
/// P11.1-S2.4 — only a bytewise comparison; encoding awareness
/// here would require codepoint-level ordering (Unicode collation)
/// which is out of v0 scope. Latin-1 vs UTF-16 operands compare
/// by their raw payload bytes, which intentionally puts every
/// Latin-1 string ordered-before every UTF-16 string (Latin-1
/// payloads are shorter / different first-byte distribution).
/// A spec-correct collation lands when the case-folding /
/// normalisation tables come online in P11.5 / P11.6.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_locale_compare(a: *const u8, b: *const u8) -> i64 {
    let (aa, _, _) = unsafe { str_view(a) };
    let (bb, _, _) = unsafe { str_view(b) };
    match locale_compare(aa, bb) {
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

// ============================================================
// No-`_from` 2-arg wrappers (P3.1-g.3, 2026-05-23) — port of the
// formerly IR-emitted `define_str_{prefix_suffix_check,index_of,
// includes}` builders in ssa_inkwell. Each is a thin call onto
// the corresponding `_from` core; default `pos` is 0 for the
// search-from-start family and `s.len()` for `ends_with` (the
// natural "scan to end" anchor).
// ============================================================

/// `s.startsWith(needle)` — equivalent to `starts_with_from(s, n, 0)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_starts_with(s: *const u8, n: *const u8) -> i64 {
    unsafe { __torajs_str_starts_with_from(s, n, 0) }
}

/// `s.endsWith(needle)` — equivalent to `ends_with_from(s, n, s.len())`.
/// The IR builder used `s_len - n_len` as the implicit anchor but
/// the `_from` core's `end` parameter is the s.len()-style anchor
/// before clamping, so passing the full `s.len()` is correct.
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

/// `s.charCodeAt(i)` — UTF-16 code unit at index `i` zero-
/// extended to i64. OOB (i < 0 or i >= s.length) returns 0
/// (M6.1 stub: TS spec is NaN but the v0 SSA layer can't return
/// NaN-as-i64). Port of `ssa_inkwell::define_str_char_code_at`
/// (P3.1-g.4, 2026-05-23).
///
/// P11.1-S2.4 — encoding-aware: Latin-1 returns the byte value
/// (0..=255 maps 1:1 to a code unit), UTF-16 returns the little-
/// endian u16 at byte offset `i × 2`. Lone surrogates are
/// returned as-is per ES spec. `codePointAt` (which combines
/// surrogate pairs) lands as a sibling intrinsic in the next
/// sub-step alongside the check.rs / ssa_lower split.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_char_code_at(s: *const u8, i: i64) -> i64 {
    let (payload, length, is_latin1) = unsafe { str_view(s) };
    if i < 0 || i >= length as i64 {
        return 0;
    }
    if is_latin1 {
        payload[i as usize] as i64
    } else {
        let off = (i as usize) * 2;
        let lo = payload[off] as u16;
        let hi = payload[off + 1] as u16;
        ((hi << 8) | lo) as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pure-core tests — no Str layout involved.

    #[test]
    fn locale_compare_ordering() {
        assert_eq!(locale_compare(b"abc", b"abc"), Ordering::Equal);
        assert_eq!(locale_compare(b"abc", b"abd"), Ordering::Less);
        assert_eq!(locale_compare(b"abd", b"abc"), Ordering::Greater);
        assert_eq!(locale_compare(b"ab", b"abc"), Ordering::Less);
        assert_eq!(locale_compare(b"abc", b"ab"), Ordering::Greater);
    }

    #[test]
    fn starts_with_basic() {
        assert!(starts_with_from(b"hello world", b"hello", 0));
        assert!(!starts_with_from(b"hello world", b"world", 0));
        assert!(starts_with_from(b"hello world", b"world", 6));
        assert!(starts_with_from(b"abc", b"", 0));
        assert!(starts_with_from(b"abc", b"", 5)); // empty + past-end still matches
        assert!(!starts_with_from(b"abc", b"abcd", 0)); // needle longer than s
    }

    #[test]
    fn starts_with_negative_pos_clamps_to_zero() {
        assert!(starts_with_from(b"hello", b"hello", -10));
        assert!(starts_with_from(b"hello", b"he", -1));
    }

    #[test]
    fn ends_with_basic() {
        assert!(ends_with_from(b"hello world", b"world", 11));
        assert!(!ends_with_from(b"hello world", b"hello", 11));
        assert!(ends_with_from(b"hello", b"", 3));
        assert!(ends_with_from(b"abc", b"abc", 3));
        assert!(!ends_with_from(b"abc", b"abc", 2)); // window is "ab"
    }

    #[test]
    fn ends_with_end_clamps_to_len() {
        assert!(ends_with_from(b"hello", b"lo", 100));
        assert!(!ends_with_from(b"hello", b"lo", -1)); // clamps to 0; window empty
    }

    #[test]
    fn index_of_basic() {
        assert_eq!(index_of_from(b"hello world", b"world", 0), Some(6));
        assert_eq!(index_of_from(b"hello world", b"world", 7), None);
        assert_eq!(index_of_from(b"hello world", b"xyz", 0), None);
        assert_eq!(index_of_from(b"aaa", b"a", 1), Some(1));
        assert_eq!(index_of_from(b"abc", b"", 2), Some(2)); // empty needle at start pos
    }

    #[test]
    fn index_of_needle_longer_than_haystack() {
        assert_eq!(index_of_from(b"ab", b"abc", 0), None);
    }

    #[test]
    fn includes_mirrors_index_of() {
        assert!(includes_from(b"hello world", b"world", 0));
        assert!(!includes_from(b"hello world", b"xyz", 0));
        assert!(includes_from(b"hello world", b"", 0));
    }

    #[test]
    fn last_index_of_basic() {
        assert_eq!(last_index_of(b"hello world", b"o"), Some(7));
        assert_eq!(last_index_of(b"hello world", b"xyz"), None);
        assert_eq!(last_index_of(b"aaa", b"a"), Some(2));
        assert_eq!(last_index_of(b"abc", b""), Some(3)); // empty needle → s.len()
    }

    #[test]
    fn last_index_of_from_basic() {
        assert_eq!(last_index_of_from(b"hello", b"l", 5), Some(3));
        assert_eq!(last_index_of_from(b"hello", b"l", 2), Some(2));
        assert_eq!(last_index_of_from(b"hello", b"l", 1), None);
        assert_eq!(last_index_of_from(b"abc", b"", 2), Some(2));
        assert_eq!(last_index_of_from(b"abc", b"", 100), Some(3));
        assert_eq!(last_index_of_from(b"abc", b"", -5), Some(0));
    }

    #[test]
    fn last_index_of_from_needle_longer_than_s() {
        assert_eq!(last_index_of_from(b"ab", b"abc", 0), None);
    }

    // ============================================================
    // FFI wrapper tests — exercise the layout-aware path.
    // ============================================================

    use crate::block::StrBlock;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn make_str(bytes: &[u8]) -> StrBlock {
        let mut block = StrBlock::alloc(bytes.len() as u32);
        unsafe {
            block
                .as_bytes_mut(bytes.len() as u32)
                .copy_from_slice(bytes)
        };
        block
    }

    #[test]
    fn ffi_index_of_from_matches_core() {
        let _g = TEST_LOCK.lock().unwrap();
        crate::pool::clear_for_test();
        let s = make_str(b"hello world");
        let n = make_str(b"world");
        let r = unsafe { __torajs_str_index_of_from(s.0.as_ptr(), n.0.as_ptr(), 0) };
        assert_eq!(r, 6);
        let r2 = unsafe { __torajs_str_index_of_from(s.0.as_ptr(), n.0.as_ptr(), 7) };
        assert_eq!(r2, -1);
        s.free_pool_aware();
        n.free_pool_aware();
    }

    #[test]
    fn ffi_locale_compare_signs() {
        let _g = TEST_LOCK.lock().unwrap();
        crate::pool::clear_for_test();
        let a = make_str(b"abc");
        let b = make_str(b"abd");
        let r = unsafe { __torajs_str_locale_compare(a.0.as_ptr(), b.0.as_ptr()) };
        assert_eq!(r, -1);
        let r2 = unsafe { __torajs_str_locale_compare(b.0.as_ptr(), a.0.as_ptr()) };
        assert_eq!(r2, 1);
        let r3 = unsafe { __torajs_str_locale_compare(a.0.as_ptr(), a.0.as_ptr()) };
        assert_eq!(r3, 0);
        a.free_pool_aware();
        b.free_pool_aware();
    }

    #[test]
    fn ffi_starts_with_from_matches_core() {
        let _g = TEST_LOCK.lock().unwrap();
        crate::pool::clear_for_test();
        let s = make_str(b"hello world");
        let n = make_str(b"world");
        let r = unsafe { __torajs_str_starts_with_from(s.0.as_ptr(), n.0.as_ptr(), 6) };
        assert_eq!(r, 1);
        let r2 = unsafe { __torajs_str_starts_with_from(s.0.as_ptr(), n.0.as_ptr(), 0) };
        assert_eq!(r2, 0);
        s.free_pool_aware();
        n.free_pool_aware();
    }

    #[test]
    fn ffi_last_index_of_no_match_returns_neg1() {
        let _g = TEST_LOCK.lock().unwrap();
        crate::pool::clear_for_test();
        let s = make_str(b"hello");
        let n = make_str(b"xyz");
        let r = unsafe { __torajs_str_last_index_of(s.0.as_ptr(), n.0.as_ptr()) };
        assert_eq!(r, -1);
        s.free_pool_aware();
        n.free_pool_aware();
    }
}
