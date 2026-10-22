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
}
