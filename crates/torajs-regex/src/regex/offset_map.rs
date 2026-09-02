//! Byte-offset ↔ UTF-16 code-unit mapping between the regex
//! engine's transcoded UTF-8 haystack and the JS-visible offset
//! semantics (`.index`, `re.lastIndex`, replace-callback offset —
//! ES §22.2.7.8 measures all of them in code units of the original
//! string). Split from `regex/mod.rs`; re-exported there so callers
//! keep the `super::{byte_to_utf16_units, utf16_units_to_byte}` face.

/// Map a byte offset in the (transcoded) UTF-8 haystack `s` to the
/// UTF-16 code-unit offset that JS spec surfaces demand.
///
/// The regex engine matches on a UTF-8 byte stream ([`haystack`]
/// transcodes Latin-1 / UTF-16 payloads), so every match position it
/// produces is a byte offset into that stream — equal to the code-
/// unit offset only when the haystack is pure ASCII. `is_ascii`
/// callers (the zero-copy [`str_slice_ascii_view`] hit) short-circuit
/// to identity; the transcoded path walks the prefix once, which the
/// O(n) transcode already dominates.
///
/// Unit math per UTF-8 lead byte: 1/2/3-byte sequences encode BMP
/// codepoints (1 UTF-16 unit); 4-byte sequences encode astral
/// codepoints (2 units, a surrogate pair). Continuation bytes count
/// zero.
pub(crate) fn byte_to_utf16_units(s: &[u8], byte_idx: i64, is_ascii: bool) -> i64 {
    if is_ascii || byte_idx <= 0 {
        return byte_idx;
    }
    let end = (byte_idx as usize).min(s.len());
    s[..end]
        .iter()
        .map(|&b| ((b & 0xC0) != 0x80) as i64 + (b >= 0xF0) as i64)
        .sum()
}

/// Inverse of [`byte_to_utf16_units`] — map a user-visible UTF-16
/// code-unit offset (e.g. an assigned `re.lastIndex`) to the byte
/// offset in the transcoded UTF-8 haystack where the search should
/// start.
///
/// Returns `s.len() as i64 + 1` when `units` lies past the end of
/// the string, so the existing `start > slen` out-of-range guards in
/// exec / test / sticky-match fire unchanged. A `units` value landing
/// inside a surrogate pair (an astral codepoint's second unit) rounds
/// forward to the next codepoint boundary — the UTF-8 stream cannot
/// represent a lone trail surrogate start position; patterns that
/// would match one there are already outside the engine's haystack
/// model.
pub(crate) fn utf16_units_to_byte(s: &[u8], units: i64, is_ascii: bool) -> i64 {
    if is_ascii || units <= 0 {
        return units;
    }
    let mut u: i64 = 0;
    let mut i: usize = 0;
    while i < s.len() {
        if u >= units {
            return i as i64;
        }
        let b = s[i];
        let (adv, du) = if b < 0x80 {
            (1, 1)
        } else if b < 0xE0 {
            (2, 1)
        } else if b < 0xF0 {
            (3, 1)
        } else {
            (4, 2)
        };
        // A unit offset inside a surrogate pair (`u` mode, four-byte
        // form) names the code point that contains it — §22.2.7.2
        // step 12.b: the match starts at that character, not after it.
        if u + du > units {
            return i as i64;
        }
        u += du;
        i += adv;
    }
    if u >= units {
        s.len() as i64
    } else {
        s.len() as i64 + 1
    }
}

#[cfg(test)]
mod tests {
    use super::{byte_to_utf16_units, utf16_units_to_byte};

    // "héllo 世界 x𝄞y" as UTF-8:
    //   h(1) é(2) l(1) l(1) o(1) sp(1) 世(3) 界(3) sp(1) x(1) 𝄞(4) y(1)
    // UTF-16 units: h=1 é=1 l=1 l=1 o=1 sp=1 世=1 界=1 sp=1 x=1 𝄞=2 y=1
    const S: &str = "héllo 世界 x𝄞y";

    #[test]
    fn byte_to_units_ascii_identity() {
        let s = b"hello world";
        assert_eq!(byte_to_utf16_units(s, 6, true), 6);
        assert_eq!(byte_to_utf16_units(s, 6, false), 6);
        assert_eq!(byte_to_utf16_units(s, 0, false), 0);
    }

    #[test]
    fn byte_to_units_bmp_and_astral() {
        let s = S.as_bytes();
        // byte 7 = start of "llo 世界 x𝄞y" (h + 2-byte é + l... no:
        // h=0, é=1..3, l=3, l=4, o=5, sp=6, 世=7..10)
        assert_eq!(byte_to_utf16_units(s, 7, false), 6); // before 世
        assert_eq!(byte_to_utf16_units(s, 10, false), 7); // before 界
        assert_eq!(byte_to_utf16_units(s, 14, false), 9); // before x
        assert_eq!(byte_to_utf16_units(s, 15, false), 10); // before 𝄞
        assert_eq!(byte_to_utf16_units(s, 19, false), 12); // before y (𝄞 = 2 units)
        assert_eq!(byte_to_utf16_units(s, s.len() as i64, false), 13);
    }

    #[test]
    fn units_to_byte_roundtrip() {
        let s = S.as_bytes();
        for byte_idx in [0i64, 1, 3, 4, 5, 6, 7, 10, 13, 14, 15, 19, 20] {
            let u = byte_to_utf16_units(s, byte_idx, false);
            assert_eq!(
                utf16_units_to_byte(s, u, false),
                byte_idx,
                "byte {byte_idx}"
            );
        }
    }

    #[test]
    fn units_to_byte_out_of_range_and_mid_pair() {
        let s = S.as_bytes();
        // total units = 13; past-end maps to slen + 1 (out-of-range guard)
        assert_eq!(utf16_units_to_byte(s, 13, false), s.len() as i64);
        assert_eq!(utf16_units_to_byte(s, 14, false), s.len() as i64 + 1);
        // unit 11 lands inside the 𝄞 surrogate pair → the code point
        // that contains it (§22.2.7.2 step 12.b), i.e. 𝄞's own start
        assert_eq!(utf16_units_to_byte(s, 11, false), 15);
        // ascii identity passes values through untouched
        assert_eq!(utf16_units_to_byte(b"abc", 2, true), 2);
        assert_eq!(utf16_units_to_byte(s, 0, false), 0);
    }
}
