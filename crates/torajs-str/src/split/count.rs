//! Token counting for the general split build — how many cells the
//! block has to hold — split out of `split/ops.rs` (file-size hard
//! limit) when the single-pass byte-separator lane landed in
//! `split/byte_sep.rs` (rotation 469). Pure mechanical pull.

/// Count non-overlapping matches of `sep` in `s`. Used to size
/// the split block. Empty `sep` and `sep.len() > s.len()` are
/// handled by the caller — this fn assumes `1 <= sep.len() <= s.len()`.
///
/// `stride` is 1 for Latin-1 / 2 for UTF-16 so candidate positions
/// stay aligned with the haystack's code-unit grid.
#[inline]
fn count_matches(s: &[u8], sep: &[u8], stride: usize) -> u64 {
    // a Latin-1 haystack cut on one byte (the dominant `" "`, `","`,
    // `"\n"`, `";"` shapes) is a byte count
    if stride == 1 && sep.len() == 1 {
        return super::count_byte::count_byte(s, sep[0]);
    }
    if sep.len() == stride {
        // Single code-unit needle, UTF-16 path (or anything where
        // sep.len() matches stride exactly): same logic as the
        // 1-byte path but element comparison is multi-byte.
        let mut hits = 0u64;
        let mut i = 0;
        while i + sep.len() <= s.len() {
            if &s[i..i + sep.len()] == sep {
                hits += 1;
            }
            i += stride;
        }
        return hits;
    }
    let limit = s.len() - sep.len();
    let mut hits = 0u64;
    let mut i = 0;
    while i <= limit {
        if &s[i..i + sep.len()] == sep {
            hits += 1;
            i += sep.len();
        } else {
            i += stride;
        }
    }
    hits
}

/// Compute the output token count for `s.split(sep)`.
/// Special cases:
/// - `sep.len() == 0` → code-unit count of `s` (per-char split)
/// - `sep.len() > s.len()` → `1` (no match, whole-s singleton)
/// - otherwise → `count_matches(s, sep, stride) + 1`
#[inline]
pub fn out_count(s: &[u8], sep: &[u8], stride: usize) -> u64 {
    if sep.is_empty() {
        (s.len() / stride) as u64
    } else if sep.len() > s.len() {
        1
    } else {
        count_matches(s, sep, stride) + 1
    }
}

/// `s.split(sep).length` without building the array.
///
/// The compiler takes this lane when the split product is only ever
/// measured — `s.split(sep).length`, or a binding whose every use is
/// `.length` (`torajs-core` `ast::split_length_only`). Same `(Str,
/// Str)` contract as [`super::ops::__torajs_str_split`] and the same
/// token count as its general build: a Latin-1 haystack can not
/// contain a UTF-16 needle (one token, the whole string), and a
/// Latin-1 needle is widened onto a UTF-16 haystack's code-unit grid.
///
/// # Safety
///
/// Both `s` and `sep` must be valid flat Str heap blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_split_count(s: *const u8, sep: *const u8) -> i64 {
    let (s_payload, _, s_latin1) = unsafe { super::ops::str_view(s) };
    let (sep_payload, _, sep_latin1) = unsafe { super::ops::str_view(sep) };
    // the dominant shape, answered before the general count's frame
    // (it saves a dozen registers) is set up
    if s_latin1 && sep_latin1 && sep_payload.len() == 1 {
        return super::count_byte::count_byte_inline(s_payload, sep_payload[0]) as i64 + 1;
    }
    count_general(s_payload, s_latin1, sep_payload, sep_latin1)
}

/// [`__torajs_str_split_count`] for a separator the compiler saw is
/// one Latin-1 code unit, `byte`: the separator's header is never
/// read, and only the receiver's width decides the count.
///
/// # Safety
///
/// `s` must be a valid flat Str heap block; `byte` is in `0..=255`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_split_count_byte(s: *const u8, byte: i64) -> i64 {
    let (s_payload, _, s_latin1) = unsafe { super::ops::str_view(s) };
    let b = byte as u8;
    if s_latin1 {
        return super::count_byte::count_byte_inline(s_payload, b) as i64 + 1;
    }
    count_wide_byte(s_payload, b)
}

/// A UTF-16 receiver cut on one Latin-1 unit; kept out of line so the
/// Latin-1 path above sets up no frame.
#[inline(never)]
fn count_wide_byte(s_payload: &[u8], b: u8) -> i64 {
    count_general(s_payload, false, &[b], true)
}

#[inline(never)]
fn count_general(s_payload: &[u8], s_latin1: bool, sep_payload: &[u8], sep_latin1: bool) -> i64 {
    let tokens = match (s_latin1, sep_latin1) {
        (true, false) => 1,
        (false, true) => {
            let widened = super::ops::widen_latin1_to_utf16(sep_payload);
            out_count(s_payload, &widened, 2)
        }
        (true, true) => out_count(s_payload, sep_payload, 1),
        (false, false) => out_count(s_payload, sep_payload, 2),
    };
    tokens as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_count_paths() {
        assert_eq!(out_count(b"abc", b"", 1), 3); // per-char
        assert_eq!(out_count(b"", b"", 1), 0);
        assert_eq!(out_count(b"abc", b"abcd", 1), 1); // sep longer than s
        assert_eq!(out_count(b"abc", b"z", 1), 1); // no match
        assert_eq!(out_count(b"a,b,c", b",", 1), 3);
        assert_eq!(out_count(b"a,,b", b",", 1), 3); // empty token middle
        assert_eq!(out_count(b",abc,", b",", 1), 3); // empty front+back
        assert_eq!(out_count(b"aaaa", b"aa", 1), 3); // non-overlapping
    }

    fn latin1(bytes: &[u8]) -> *mut u8 {
        let mut block = crate::block::StrBlock::alloc_with_encoding(bytes.len() as u32, true);
        unsafe { block.as_bytes_mut(bytes.len() as u32) }.copy_from_slice(bytes);
        block.into_raw()
    }

    fn utf16(s: &str) -> *mut u8 {
        let units: alloc::vec::Vec<u16> = s.encode_utf16().collect();
        let n = units.len() as u32;
        let mut block = crate::block::StrBlock::alloc_with_encoding(n, false);
        let dst = unsafe { block.as_bytes_mut(n * 2) };
        for (i, u) in units.iter().enumerate() {
            dst[i * 2..i * 2 + 2].copy_from_slice(&u.to_le_bytes());
        }
        block.into_raw()
    }

    fn count(s: *mut u8, sep: *mut u8) -> i64 {
        unsafe { __torajs_str_split_count(s, sep) }
    }

    #[test]
    fn entry_on_every_width_pair() {
        // Latin-1 on both sides, one-byte separator: the byte count
        assert_eq!(count(latin1(b""), latin1(b",")), 1);
        assert_eq!(count(latin1(b",a,,b,"), latin1(b",")), 5);
        assert_eq!(count(latin1(b"caf\xe9x\xe9"), latin1(b"\xe9")), 3);
        // Latin-1 longer separator and the empty one
        assert_eq!(count(latin1(b"a, b, c"), latin1(b", ")), 3);
        assert_eq!(count(latin1(b"abc"), latin1(b"")), 3);
        // a UTF-16 receiver with a Latin-1 separator widens it
        assert_eq!(count(utf16("日 本 語"), latin1(b" ")), 3);
        assert_eq!(count(utf16("日\u{e9}本\u{e9}"), latin1(b"\xe9")), 3);
        // a UTF-16 separator: never in a Latin-1 receiver
        assert_eq!(count(latin1(b"a b"), utf16("→")), 1);
        assert_eq!(count(utf16("a→b→c"), utf16("→")), 3);
    }

    #[test]
    fn byte_entry_matches_the_general_entry() {
        let byte = |s: *mut u8, b: u8| unsafe { __torajs_str_split_count_byte(s, i64::from(b)) };
        for (s, b) in [
            (latin1(b""), b','),
            (latin1(b","), b','),
            (latin1(b",a,,b,"), b','),
            (latin1(b"3 4 + 2 * 5 +"), b' '),
            (latin1(b"caf\xe9x\xe9"), 0xe9),
            (utf16("日 本 語"), b' '),
            (utf16("日\u{e9}本\u{e9}"), 0xe9),
            (utf16("α,β,,γ,"), b','),
            (utf16("日本"), b','),
        ] {
            assert_eq!(byte(s, b), count(s, latin1(&[b])));
        }
    }
}
