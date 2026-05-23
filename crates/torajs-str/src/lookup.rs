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

use crate::layout::{STR_DATA_OFF, STR_LEN_OFF};

// ============================================================
// Layout-aware FFI helpers
// ============================================================

#[inline]
unsafe fn str_len(p: *const u8) -> u64 {
    unsafe { (p.add(STR_LEN_OFF) as *const u64).read() }
}

#[inline]
unsafe fn str_bytes<'a>(p: *const u8, len: u64) -> &'a [u8] {
    unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), len as usize) }
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_locale_compare(a: *const u8, b: *const u8) -> i64 {
    let aa = unsafe { str_bytes(a, str_len(a)) };
    let bb = unsafe { str_bytes(b, str_len(b)) };
    match locale_compare(aa, bb) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// `s.startsWith(needle, pos)` — 1 if matches, 0 otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_starts_with_from(
    s: *const u8,
    sub: *const u8,
    pos: i64,
) -> i64 {
    let ss = unsafe { str_bytes(s, str_len(s)) };
    let nn = unsafe { str_bytes(sub, str_len(sub)) };
    if starts_with_from(ss, nn, pos) { 1 } else { 0 }
}

/// `s.endsWith(needle, end)` — 1 if matches, 0 otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_ends_with_from(
    s: *const u8,
    sub: *const u8,
    end: i64,
) -> i64 {
    let ss = unsafe { str_bytes(s, str_len(s)) };
    let nn = unsafe { str_bytes(sub, str_len(sub)) };
    if ends_with_from(ss, nn, end) { 1 } else { 0 }
}

/// `s.indexOf(needle, fromIdx)` — found index or `-1`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_index_of_from(
    s: *const u8,
    sub: *const u8,
    from: i64,
) -> i64 {
    let ss = unsafe { str_bytes(s, str_len(s)) };
    let nn = unsafe { str_bytes(sub, str_len(sub)) };
    match index_of_from(ss, nn, from) {
        Some(i) => i as i64,
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
    let ss = unsafe { str_bytes(s, str_len(s)) };
    let nn = unsafe { str_bytes(sub, str_len(sub)) };
    if includes_from(ss, nn, from) { 1 } else { 0 }
}

/// `s.lastIndexOf(needle, fromIdx)` — found index or `-1`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_last_index_of_from(
    s: *const u8,
    sub: *const u8,
    from: i64,
) -> i64 {
    let ss = unsafe { str_bytes(s, str_len(s)) };
    let nn = unsafe { str_bytes(sub, str_len(sub)) };
    match last_index_of_from(ss, nn, from) {
        Some(i) => i as i64,
        None => -1,
    }
}

/// `s.lastIndexOf(needle)` — found index or `-1`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_last_index_of(s: *const u8, needle: *const u8) -> i64 {
    let ss = unsafe { str_bytes(s, str_len(s)) };
    let nn = unsafe { str_bytes(needle, str_len(needle)) };
    match last_index_of(ss, nn) {
        Some(i) => i as i64,
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

    use crate::alloc::StrBlock;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn make_str(bytes: &[u8]) -> StrBlock {
        let mut block = StrBlock::alloc(bytes.len() as u64);
        unsafe {
            block
                .as_bytes_mut(bytes.len() as u64)
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
