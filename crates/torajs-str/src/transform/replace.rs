//! Str needle substitution — `s.replace(needle, repl)` /
//! `s.replaceAll(needle, repl)`.
//!
//! **String needle only** (v0 subset; spec accepts a regex needle
//! which v0 does not implement). Byte-level scan matches the
//! pre-rewrite C `__torajs_str_replace` / `_replace_all` behavior
//! bit-for-bit; bun-parity holds for ASCII inputs.
//!
//! Spec corner notes (preserved from C):
//! - **`replace` empty needle** — JS inserts at index 0, returning
//!   `repl + s` (subset matches this).
//! - **`replace` no match** — returns a fresh copy of `s` (so the
//!   caller can drop both inputs uniformly without checking
//!   identity).
//! - **`replaceAll` empty needle** — JS spec throws TypeError; the
//!   v0 subset silently returns a fresh copy of `s` (the typed
//!   pipeline does not generate this case under typical test
//!   inputs).
//! - **`replaceAll` non-overlapping** — after a match at index `i`,
//!   scan resumes at `i + needle.len()` (standard JS behavior).
//!
//! IR-side surface: `__torajs_str_replace` · `__torajs_str_replace_all`,
//! both `(Str, Str, Str) -> Str`; alloc-noalias-whitelisted in
//! `ssa_inkwell::is_alloc_intrinsic`.

use crate::block::StrBlock;
use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use alloc::vec::Vec;
use torajs_rc::HeapHeader;

// ============================================================
// Layout-aware FFI helpers (sub-module-local)
// ============================================================

// `str_len` / `str_bytes` were used by the pre-S2.5 byte-stream
// helpers; post-S2.5 every wrapper above goes through `str_view`.
// Removed to keep the module dead-code-clean.

/// Read a Str's `(payload_bytes, code_unit_count, is_latin1)` view.
/// `payload` length = `length × (1 for Latin-1 | 2 for UTF-16)`.
#[inline]
unsafe fn str_view<'a>(p: *const u8) -> (&'a [u8], u32, bool) {
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

/// Widen a Latin-1 byte payload to a UTF-16 LE byte buffer.
fn widen_latin1_to_utf16(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() * 2);
    for &b in src {
        out.push(b);
        out.push(0);
    }
    out
}

/// Cow-shaped owned-or-borrowed byte view used by replace's
/// per-operand encoding coercion.
enum PayloadBuf<'a> {
    Borrowed(&'a [u8]),
    Owned(Vec<u8>),
}

impl<'a> AsRef<[u8]> for PayloadBuf<'a> {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Borrowed(s) => s,
            Self::Owned(v) => v.as_slice(),
        }
    }
}

#[inline]
fn coerce_payload<'a>(src: &'a [u8], src_is_latin1: bool, out_is_latin1: bool) -> PayloadBuf<'a> {
    if src_is_latin1 == out_is_latin1 {
        PayloadBuf::Borrowed(src)
    } else {
        // Only Latin-1 → UTF-16 widening is meaningful here; the
        // out encoding is `s_latin1 && needle_latin1 && repl_latin1`
        // so any Latin-1 input always agrees with an out=Latin-1
        // result. UTF-16 → Latin-1 narrowing isn't reachable.
        PayloadBuf::Owned(widen_latin1_to_utf16(src))
    }
}

/// Build a fresh Str carrying the requested code-unit length and
/// encoding, then write `byte_count` bytes (≤ payload capacity)
/// via the provided closure.
fn build_result_with<F>(length: u32, byte_count: u32, is_latin1: bool, write: F) -> *mut u8
where
    F: FnOnce(&mut [u8]),
{
    let mut block = StrBlock::alloc_with_encoding(length, is_latin1);
    if byte_count > 0 {
        let dst = unsafe { block.as_bytes_mut(byte_count) };
        write(dst);
    }
    block.into_raw()
}

/// Fresh copy of `s_payload` under the source's own encoding.
/// Mirrors `__torajs_str_alloc`'s shape for callers that already
/// have an encoded payload in-hand.
fn alloc_str_copy(s_payload: &[u8], s_latin1: bool) -> *mut u8 {
    let stride: u32 = if s_latin1 { 1 } else { 2 };
    let byte_cnt = s_payload.len() as u32;
    let length = byte_cnt / stride;
    build_result_with(length, byte_cnt, s_latin1, |dst| {
        dst.copy_from_slice(s_payload)
    })
}

// ============================================================
// Pure-Rust cores
// ============================================================

/// First non-overlapping occurrence of `needle` in `s`. Empty
/// needle returns `Some(0)` (matches JS `replace` insert-at-0
/// semantics). Returns `None` if `needle` exceeds `s` or no match.
///
/// Step 1 (default). Use [`find_first_with_stride`] for the
/// encoding-aware version that keeps candidate positions aligned
/// to the haystack's code-unit grid.
#[inline]
pub fn find_first(s: &[u8], needle: &[u8]) -> Option<usize> {
    find_first_with_stride(s, needle, 1)
}

/// First non-overlapping occurrence of `needle` in `s` with the
/// candidate position stepping by `stride` (1 for Latin-1, 2 for
/// UTF-16 LE so byte offsets stay u16-aligned). Empty needle hits
/// at 0.
#[inline]
pub fn find_first_with_stride(s: &[u8], needle: &[u8], stride: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > s.len() {
        return None;
    }
    let max = s.len() - needle.len();
    let mut i = 0;
    while i <= max {
        if &s[i..i + needle.len()] == needle {
            return Some(i);
        }
        i += stride;
    }
    None
}

/// Count of non-overlapping `needle` matches in `s`. After each
/// match, scan resumes at `match_idx + needle.len()`. Empty
/// needle returns 0 (caller short-circuits to a copy of `s`).
///
/// Latin-1 default; see [`count_non_overlapping_with_stride`] for
/// the encoding-aware path.
#[inline]
pub fn count_non_overlapping(s: &[u8], needle: &[u8]) -> usize {
    count_non_overlapping_with_stride(s, needle, 1)
}

/// Stride-aware non-overlapping match count. Same semantics as
/// [`count_non_overlapping`], with the between-candidate step
/// equal to `stride` so UTF-16 LE scans stay u16-aligned.
#[inline]
pub fn count_non_overlapping_with_stride(s: &[u8], needle: &[u8], stride: usize) -> usize {
    if needle.is_empty() || needle.len() > s.len() {
        return 0;
    }
    let mut hits = 0;
    let mut i = 0;
    let limit = s.len() - needle.len();
    while i <= limit {
        if &s[i..i + needle.len()] == needle {
            hits += 1;
            i += needle.len();
        } else {
            i += stride;
        }
    }
    hits
}

/// Output length for `replace_all`. Computed without underflow
/// (`hits * needle.len() <= s.len()` by construction).
#[inline]
pub fn replace_all_out_len(s_len: u32, hits: u32, n_len: u32, r_len: u32) -> u32 {
    s_len - hits * n_len + hits * r_len
}

/// Single substitution: write `pre + repl + post` into `dst`.
/// Assumes `dst.len() == pre.len() + repl.len() + post.len()`.
/// Used by both `replace` (single hit) and the post-loop tail of
/// `replace_all` if needed.
#[inline]
pub fn splice_into(dst: &mut [u8], pre: &[u8], repl: &[u8], post: &[u8]) {
    debug_assert_eq!(dst.len(), pre.len() + repl.len() + post.len());
    let mut cursor = 0;
    dst[cursor..cursor + pre.len()].copy_from_slice(pre);
    cursor += pre.len();
    dst[cursor..cursor + repl.len()].copy_from_slice(repl);
    cursor += repl.len();
    dst[cursor..cursor + post.len()].copy_from_slice(post);
}

/// Multi-substitution copy pass. Walks `s` with non-overlapping
/// `needle` matches, writing `repl` for each match and the
/// surrounding bytes verbatim. `dst.len()` must equal the
/// post-replace total byte count for the same `(s, needle,
/// repl)` triple.
///
/// Latin-1 default. Use [`replace_all_into_with_stride`] for the
/// encoding-aware version that respects code-unit boundaries.
#[inline]
pub fn replace_all_into(s: &[u8], needle: &[u8], repl: &[u8], dst: &mut [u8]) {
    replace_all_into_with_stride(s, needle, repl, dst, 1);
}

/// Stride-aware variant of [`replace_all_into`]. UTF-16 LE inputs
/// pass `stride = 2` so candidate match positions and the verbatim
/// copy of non-match code units stay u16-aligned.
#[inline]
pub fn replace_all_into_with_stride(
    s: &[u8],
    needle: &[u8],
    repl: &[u8],
    dst: &mut [u8],
    stride: usize,
) {
    let mut src_i = 0;
    let mut dst_i = 0;
    let n_len = needle.len();
    if n_len > 0 && s.len() >= n_len {
        let limit = s.len() - n_len;
        while src_i <= limit {
            if &s[src_i..src_i + n_len] == needle {
                dst[dst_i..dst_i + repl.len()].copy_from_slice(repl);
                dst_i += repl.len();
                src_i += n_len;
            } else {
                dst[dst_i..dst_i + stride].copy_from_slice(&s[src_i..src_i + stride]);
                dst_i += stride;
                src_i += stride;
            }
        }
    }
    // Tail bytes (everything past the last possible match position).
    let tail = &s[src_i..];
    dst[dst_i..dst_i + tail.len()].copy_from_slice(tail);
}

// ============================================================
// extern "C" wrappers
// ============================================================

/// `s.replace(needle, repl)` — replace the FIRST occurrence; if
/// no match, return a fresh copy of `s`.
///
/// # Safety
///
/// All three pointers must be valid Str heap blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_replace(
    s: *const u8,
    needle: *const u8,
    repl: *const u8,
) -> *mut u8 {
    let (s_payload, _, s_latin1) = unsafe { str_view(s) };
    let (n_payload, n_len_cu, n_latin1) = unsafe { str_view(needle) };
    let (r_payload, _, r_latin1) = unsafe { str_view(repl) };

    // Canonical short-circuit: a UTF-16 needle cannot occur inside
    // a Latin-1 haystack (the needle's codepoint > 0xFF can't
    // appear). Return a fresh copy of `s` under its own encoding,
    // *except* the empty-needle insert-at-0 case which still has
    // to splice `repl` at the front under the widest of the two
    // input encodings.
    if s_latin1 && !n_latin1 && n_len_cu != 0 {
        return alloc_str_copy(s_payload, s_latin1);
    }

    // Result encoding = widest of the three operands.
    let out_latin1 = s_latin1 && n_latin1 && r_latin1;
    let stride: u32 = if out_latin1 { 1 } else { 2 };
    let s_buf = coerce_payload(s_payload, s_latin1, out_latin1);
    let n_buf = coerce_payload(n_payload, n_latin1, out_latin1);
    let r_buf = coerce_payload(r_payload, r_latin1, out_latin1);
    let s_bytes = s_buf.as_ref();
    let n_bytes = n_buf.as_ref();
    let r_bytes = r_buf.as_ref();

    let Some(found) = find_first_with_stride(s_bytes, n_bytes, stride as usize) else {
        return alloc_str_copy(s_payload, s_latin1);
    };

    let total_byte_cnt = s_bytes.len() + r_bytes.len() - n_bytes.len();
    let length = (total_byte_cnt as u32) / stride;
    build_result_with(length, total_byte_cnt as u32, out_latin1, |dst| {
        splice_into(
            dst,
            &s_bytes[..found],
            r_bytes,
            &s_bytes[found + n_bytes.len()..],
        );
    })
}

/// `s.replaceAll(needle, repl)` — replace every non-overlapping
/// occurrence. Empty needle returns a fresh copy of `s` (subset
/// silent divergence from spec which throws TypeError).
///
/// # Safety
///
/// All three pointers must be valid Str heap blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_replace_all(
    s: *const u8,
    needle: *const u8,
    repl: *const u8,
) -> *mut u8 {
    let (s_payload, _, s_latin1) = unsafe { str_view(s) };
    let (n_payload, n_len_cu, n_latin1) = unsafe { str_view(needle) };
    let (r_payload, _, r_latin1) = unsafe { str_view(repl) };

    if n_len_cu == 0 {
        return alloc_str_copy(s_payload, s_latin1);
    }

    // Latin-1 haystack + UTF-16 needle ⇒ no match; canonical short
    // circuit returns a fresh copy of `s`.
    if s_latin1 && !n_latin1 {
        return alloc_str_copy(s_payload, s_latin1);
    }

    let out_latin1 = s_latin1 && n_latin1 && r_latin1;
    let stride: u32 = if out_latin1 { 1 } else { 2 };
    let s_buf = coerce_payload(s_payload, s_latin1, out_latin1);
    let n_buf = coerce_payload(n_payload, n_latin1, out_latin1);
    let r_buf = coerce_payload(r_payload, r_latin1, out_latin1);
    let s_bytes = s_buf.as_ref();
    let n_bytes = n_buf.as_ref();
    let r_bytes = r_buf.as_ref();

    let hits = count_non_overlapping_with_stride(s_bytes, n_bytes, stride as usize);
    if hits == 0 {
        return alloc_str_copy(s_payload, s_latin1);
    }

    let total_byte_cnt = s_bytes.len() + (hits * r_bytes.len()) - (hits * n_bytes.len());
    let length = (total_byte_cnt as u32) / stride;
    build_result_with(length, total_byte_cnt as u32, out_latin1, |dst| {
        replace_all_into_with_stride(s_bytes, n_bytes, r_bytes, dst, stride as usize);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{vec, vec::Vec};

    #[test]
    fn find_first_basic() {
        assert_eq!(find_first(b"hello", b"ll"), Some(2));
        assert_eq!(find_first(b"hello", b"o"), Some(4));
        assert_eq!(find_first(b"hello", b"z"), None);
        assert_eq!(find_first(b"abc", b"abc"), Some(0));
        assert_eq!(find_first(b"", b""), Some(0));
        assert_eq!(find_first(b"abc", b""), Some(0));
        assert_eq!(find_first(b"", b"x"), None);
        assert_eq!(find_first(b"abc", b"abcd"), None);
    }

    #[test]
    fn count_non_overlapping_basic() {
        assert_eq!(count_non_overlapping(b"abcabc", b"b"), 2);
        assert_eq!(count_non_overlapping(b"aaaa", b"aa"), 2); // non-overlap
        assert_eq!(count_non_overlapping(b"aaa", b"aa"), 1);
        assert_eq!(count_non_overlapping(b"abc", b"z"), 0);
        assert_eq!(count_non_overlapping(b"", b"x"), 0);
        assert_eq!(count_non_overlapping(b"abc", b""), 0); // empty needle → 0
    }

    #[test]
    fn out_len_basic() {
        assert_eq!(replace_all_out_len(10, 2, 3, 1), 6); // shrink
        assert_eq!(replace_all_out_len(10, 2, 1, 3), 14); // grow
        assert_eq!(replace_all_out_len(10, 1, 5, 5), 10); // same size
        assert_eq!(replace_all_out_len(6, 3, 2, 2), 6); // full coverage same-size
    }

    #[test]
    fn splice_into_basic() {
        let mut dst = [0u8; 8];
        splice_into(&mut dst, b"foo", b"XY", b"bar");
        assert_eq!(&dst, b"fooXYbar");
        let mut dst2 = [0u8; 5];
        splice_into(&mut dst2, b"", b"XY", b"bar");
        assert_eq!(&dst2, b"XYbar");
    }

    #[test]
    fn replace_all_into_grow_and_shrink() {
        let mut out = vec![0u8; 6];
        replace_all_into(b"aaaa", b"aa", b"b", &mut out[..2]);
        assert_eq!(&out[..2], b"bb");
        let mut out2 = vec![0u8; replace_all_out_len(3, 3, 1, 2) as usize];
        replace_all_into(b"aaa", b"a", b"BB", &mut out2);
        assert_eq!(&out2, b"BBBBBB");
    }

    // ============================================================
    // FFI round-trip tests
    // ============================================================

    use crate::block::__torajs_str_free;

    fn make_str(payload: &[u8]) -> *mut u8 {
        let mut b = StrBlock::alloc(payload.len() as u32);
        let dst = unsafe { b.as_bytes_mut(payload.len() as u32) };
        dst.copy_from_slice(payload);
        b.into_raw()
    }

    fn read_str(p: *const u8) -> Vec<u8> {
        let (payload, _, _) = unsafe { str_view(p) };
        payload.to_vec()
    }

    #[test]
    fn ffi_replace_first_occurrence_only() {
        let s = make_str(b"hello world");
        let n = make_str(b"world");
        let r = make_str(b"there");
        let out = unsafe { __torajs_str_replace(s, n, r) };
        assert_eq!(read_str(out), b"hello there");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(n) };
        unsafe { __torajs_str_free(r) };
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn ffi_replace_only_first() {
        let s = make_str(b"abcabc");
        let n = make_str(b"b");
        let r = make_str(b"X");
        let out = unsafe { __torajs_str_replace(s, n, r) };
        assert_eq!(read_str(out), b"aXcabc");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(n) };
        unsafe { __torajs_str_free(r) };
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn ffi_replace_not_found_returns_copy() {
        let s = make_str(b"abc");
        let n = make_str(b"z");
        let r = make_str(b"X");
        let out = unsafe { __torajs_str_replace(s, n, r) };
        assert_eq!(read_str(out), b"abc");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(n) };
        unsafe { __torajs_str_free(r) };
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn ffi_replace_empty_needle_inserts_at_zero() {
        let s = make_str(b"abc");
        let n = make_str(b"");
        let r = make_str(b"X");
        let out = unsafe { __torajs_str_replace(s, n, r) };
        assert_eq!(read_str(out), b"Xabc");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(n) };
        unsafe { __torajs_str_free(r) };
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn ffi_replace_all_grows_and_shrinks() {
        let s_grow = make_str(b"aaa");
        let n_g = make_str(b"a");
        let r_g = make_str(b"BB");
        let out_g = unsafe { __torajs_str_replace_all(s_grow, n_g, r_g) };
        assert_eq!(read_str(out_g), b"BBBBBB");

        let s_shr = make_str(b"BBBB");
        let n_s = make_str(b"BB");
        let r_s = make_str(b"x");
        let out_s = unsafe { __torajs_str_replace_all(s_shr, n_s, r_s) };
        assert_eq!(read_str(out_s), b"xx");

        unsafe { __torajs_str_free(s_grow) };
        unsafe { __torajs_str_free(n_g) };
        unsafe { __torajs_str_free(r_g) };
        unsafe { __torajs_str_free(out_g) };
        unsafe { __torajs_str_free(s_shr) };
        unsafe { __torajs_str_free(n_s) };
        unsafe { __torajs_str_free(r_s) };
        unsafe { __torajs_str_free(out_s) };
    }

    #[test]
    fn ffi_replace_all_non_overlapping() {
        let s = make_str(b"aaaa");
        let n = make_str(b"aa");
        let r = make_str(b"b");
        let out = unsafe { __torajs_str_replace_all(s, n, r) };
        assert_eq!(read_str(out), b"bb");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(n) };
        unsafe { __torajs_str_free(r) };
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn ffi_replace_all_no_match_copies() {
        let s = make_str(b"abc");
        let n = make_str(b"z");
        let r = make_str(b"X");
        let out = unsafe { __torajs_str_replace_all(s, n, r) };
        assert_eq!(read_str(out), b"abc");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(n) };
        unsafe { __torajs_str_free(r) };
        unsafe { __torajs_str_free(out) };
    }

    #[test]
    fn ffi_replace_all_empty_needle_silently_copies() {
        let s = make_str(b"abc");
        let n = make_str(b"");
        let r = make_str(b"X");
        let out = unsafe { __torajs_str_replace_all(s, n, r) };
        assert_eq!(read_str(out), b"abc");
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(n) };
        unsafe { __torajs_str_free(r) };
        unsafe { __torajs_str_free(out) };
    }
}
