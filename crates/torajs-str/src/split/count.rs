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
    // V0.2 P14-S6 — single-byte-needle SIMD fast path. Latin-1
    // haystack with 1-byte separator (the dominant `" "`, `","`,
    // `"\n"`, `";"` shapes) collapses to a byte-equality reduce
    // that LLVM auto-vectorizes to NEON `pcmpeq + popcount` on
    // ARM64. `bench/cases/split-only-100k` (` `-separated short
    // string) lives on this path.
    if stride == 1 && sep.len() == 1 {
        let target = sep[0];
        return s.iter().filter(|&&b| b == target).count() as u64;
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
