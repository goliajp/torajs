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

use crate::alloc::StrBlock;
use crate::layout::{STR_DATA_OFF, STR_LEN_OFF};

// ============================================================
// Layout-aware FFI helpers (sub-module-local)
// ============================================================

#[inline]
unsafe fn str_len(p: *const u8) -> u64 {
    unsafe { (p.add(STR_LEN_OFF) as *const u64).read() }
}

#[inline]
unsafe fn str_bytes<'a>(p: *const u8, len: u64) -> &'a [u8] {
    unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), len as usize) }
}

#[inline]
fn alloc_str(payload: &[u8]) -> *mut u8 {
    let out_len = payload.len() as u64;
    let mut block = StrBlock::alloc(out_len);
    if !payload.is_empty() {
        let dst = unsafe { block.as_bytes_mut(out_len) };
        dst.copy_from_slice(payload);
    }
    block.into_raw()
}

// ============================================================
// Pure-Rust cores
// ============================================================

/// First non-overlapping occurrence of `needle` in `s`. Empty
/// needle returns `Some(0)` (matches JS `replace` insert-at-0
/// semantics). Returns `None` if `needle` exceeds `s` or no match.
#[inline]
pub fn find_first(s: &[u8], needle: &[u8]) -> Option<usize> {
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
        i += 1;
    }
    None
}

/// Count of non-overlapping `needle` matches in `s`. After each
/// match, scan resumes at `match_idx + needle.len()`. Empty
/// needle returns 0 (caller short-circuits to a copy of `s`).
#[inline]
pub fn count_non_overlapping(s: &[u8], needle: &[u8]) -> usize {
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
            i += 1;
        }
    }
    hits
}

/// Output length for `replace_all`. Computed without underflow
/// (`hits * needle.len() <= s.len()` by construction).
#[inline]
pub fn replace_all_out_len(s_len: u64, hits: u64, n_len: u64, r_len: u64) -> u64 {
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
/// surrounding bytes verbatim. `dst.len()` must equal
/// [`replace_all_out_len`]'s result for the same `(s, needle,
/// repl)` triple.
#[inline]
pub fn replace_all_into(s: &[u8], needle: &[u8], repl: &[u8], dst: &mut [u8]) {
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
                dst[dst_i] = s[src_i];
                dst_i += 1;
                src_i += 1;
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
    let s_len = unsafe { str_len(s) };
    let n_len = unsafe { str_len(needle) };
    let r_len = unsafe { str_len(repl) };
    let s_bytes = unsafe { str_bytes(s, s_len) };
    let n_bytes = unsafe { str_bytes(needle, n_len) };
    let r_bytes = unsafe { str_bytes(repl, r_len) };

    let Some(found) = find_first(s_bytes, n_bytes) else {
        return alloc_str(s_bytes);
    };

    let out_len = s_len - n_len + r_len;
    let mut block = StrBlock::alloc(out_len);
    let dst = unsafe { block.as_bytes_mut(out_len) };
    splice_into(
        dst,
        &s_bytes[..found],
        r_bytes,
        &s_bytes[found + n_bytes.len()..],
    );
    block.into_raw()
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
    let s_len = unsafe { str_len(s) };
    let n_len = unsafe { str_len(needle) };
    let r_len = unsafe { str_len(repl) };
    let s_bytes = unsafe { str_bytes(s, s_len) };
    let n_bytes = unsafe { str_bytes(needle, n_len) };
    let r_bytes = unsafe { str_bytes(repl, r_len) };

    if n_len == 0 {
        return alloc_str(s_bytes);
    }
    let hits = count_non_overlapping(s_bytes, n_bytes) as u64;
    if hits == 0 {
        return alloc_str(s_bytes);
    }
    let out_len = replace_all_out_len(s_len, hits, n_len, r_len);
    let mut block = StrBlock::alloc(out_len);
    let dst = unsafe { block.as_bytes_mut(out_len) };
    replace_all_into(s_bytes, n_bytes, r_bytes, dst);
    block.into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    use crate::alloc::__torajs_str_free;

    fn make_str(payload: &[u8]) -> *mut u8 {
        let mut b = StrBlock::alloc(payload.len() as u64);
        let dst = unsafe { b.as_bytes_mut(payload.len() as u64) };
        dst.copy_from_slice(payload);
        b.into_raw()
    }

    fn read_str(p: *const u8) -> Vec<u8> {
        let len = unsafe { str_len(p) };
        unsafe { str_bytes(p, len) }.to_vec()
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
