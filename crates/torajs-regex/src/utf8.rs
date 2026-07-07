//! Minimal UTF-8 helpers — port of `runtime_regex.c` L89-153.
//!
//! Input strings in tora are always well-formed UTF-8 (JS strings
//! round-trip through tora's Str storage). These primitives are
//! intentionally minimal:
//!
//! - [`utf8_len_for`] inspects the leading byte and reports the
//!   encoded byte length of the code point starting there. Used by
//!   the VM's `OP_ANYCHAR` advance under the u flag.
//! - [`utf8_encode_cp`] encodes a code point into 1-4 bytes. Used
//!   by the parser at `\u{HHHH..}` literal-escape time.
//! - [`utf8_decode_cp`] decodes the leading code point and returns
//!   `(code_point, byte_length)`. Used by the VM's `OP_CLASS` test
//!   under the u flag where the class is unicode-aware.
//!
//! Defensive paths (continuation bytes / invalid leading bytes)
//! report length 1 to keep the matcher's cursor advancing — bun
//! does the same on malformed bytes once the SyntaxError check has
//! passed at compile time.

pub fn utf8_len_for(b: u8) -> usize {
    if b & 0x80 == 0x00 {
        1 // 0xxxxxxx
    } else if b & 0xE0 == 0xC0 {
        2 // 110xxxxx
    } else if b & 0xF0 == 0xE0 {
        3 // 1110xxxx
    } else if b & 0xF8 == 0xF0 {
        4 // 11110xxx
    } else {
        1 // continuation / invalid — defensive
    }
}

pub fn utf8_encode_cp(cp: i32, out: &mut [u8; 4]) -> usize {
    if !(0..=0x10FFFF).contains(&cp) {
        return 0;
    }
    let cp = cp as u32;
    if cp < 0x80 {
        out[0] = cp as u8;
        1
    } else if cp < 0x800 {
        out[0] = 0xC0 | (cp >> 6) as u8;
        out[1] = 0x80 | (cp & 0x3F) as u8;
        2
    } else if cp < 0x10000 {
        out[0] = 0xE0 | (cp >> 12) as u8;
        out[1] = 0x80 | ((cp >> 6) & 0x3F) as u8;
        out[2] = 0x80 | (cp & 0x3F) as u8;
        3
    } else {
        out[0] = 0xF0 | (cp >> 18) as u8;
        out[1] = 0x80 | ((cp >> 12) & 0x3F) as u8;
        out[2] = 0x80 | ((cp >> 6) & 0x3F) as u8;
        out[3] = 0x80 | (cp & 0x3F) as u8;
        4
    }
}

/// Decode the leading code point of `s`. Returns `(code_point, length_in_bytes)`.
///
/// Caller must guarantee `!s.is_empty()` and `s.len() >= utf8_len_for(s[0])`
/// — both invariants hold for the matcher's input (a fully-validated
/// `Str` slice). Invalid bytes degrade to a single-byte advance with the
/// raw byte value as the code point.
pub fn utf8_decode_cp(s: &[u8]) -> (i32, usize) {
    let b = s[0];
    if b & 0x80 == 0 {
        (b as i32, 1)
    } else if b & 0xE0 == 0xC0 {
        ((((b & 0x1F) as i32) << 6) | ((s[1] & 0x3F) as i32), 2)
    } else if b & 0xF0 == 0xE0 {
        (
            (((b & 0x0F) as i32) << 12) | (((s[1] & 0x3F) as i32) << 6) | ((s[2] & 0x3F) as i32),
            3,
        )
    } else if b & 0xF8 == 0xF0 {
        (
            (((b & 0x07) as i32) << 18)
                | (((s[1] & 0x3F) as i32) << 12)
                | (((s[2] & 0x3F) as i32) << 6)
                | ((s[3] & 0x3F) as i32),
            4,
        )
    } else {
        (b as i32, 1) // invalid lead — defensive
    }
}

/// Decode the code point ENDING at byte offset `pos` (exclusive) —
/// i.e. the code point occupying `s[pos-len..pos]`. Returns
/// `(code_point, length_in_bytes)`. Used by the reverse Pike VM
/// (lookbehind bodies) to step one code point leftwards under the
/// u flag.
///
/// Caller must guarantee `1 <= pos <= s.len()`. Scans back over at
/// most 3 continuation bytes to the leading byte; malformed
/// sequences (continuation run with no lead in range, or a lead
/// whose declared length doesn't land exactly at `pos`) degrade to
/// a single-byte step with the raw byte value as the code point —
/// same defensive posture as [`utf8_decode_cp`].
pub fn utf8_decode_cp_before(s: &[u8], pos: usize) -> (i32, usize) {
    let last = s[pos - 1];
    if last & 0x80 == 0 {
        return (last as i32, 1);
    }
    let mut start = pos - 1;
    let mut steps = 0;
    while steps < 3 && start > 0 && s[start] & 0xC0 == 0x80 {
        start -= 1;
        steps += 1;
    }
    let lead = s[start];
    if lead & 0xC0 == 0x80 || start + utf8_len_for(lead) != pos {
        return (last as i32, 1); // malformed — defensive
    }
    utf8_decode_cp(&s[start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn len_for_ascii() {
        assert_eq!(utf8_len_for(b'A'), 1);
        assert_eq!(utf8_len_for(0x00), 1);
        assert_eq!(utf8_len_for(0x7F), 1);
    }

    #[test]
    fn len_for_multibyte_leads() {
        assert_eq!(utf8_len_for(0xC2), 2); // 110xxxxx
        assert_eq!(utf8_len_for(0xE0), 3); // 1110xxxx
        assert_eq!(utf8_len_for(0xF0), 4); // 11110xxx
    }

    #[test]
    fn len_for_continuation_byte_is_defensive_one() {
        assert_eq!(utf8_len_for(0x80), 1);
        assert_eq!(utf8_len_for(0xBF), 1);
    }

    #[test]
    fn encode_roundtrip_ascii() {
        let mut buf = [0u8; 4];
        assert_eq!(utf8_encode_cp(b'A' as i32, &mut buf), 1);
        assert_eq!(buf[0], b'A');
    }

    #[test]
    fn encode_roundtrip_latin1() {
        let mut buf = [0u8; 4];
        assert_eq!(utf8_encode_cp(0x00A9, &mut buf), 2); // ©
        assert_eq!(&buf[..2], &[0xC2, 0xA9]);
    }

    #[test]
    fn encode_roundtrip_bmp() {
        let mut buf = [0u8; 4];
        assert_eq!(utf8_encode_cp(0x4E2D, &mut buf), 3); // 中
        assert_eq!(&buf[..3], &[0xE4, 0xB8, 0xAD]);
    }

    #[test]
    fn encode_roundtrip_smp() {
        let mut buf = [0u8; 4];
        assert_eq!(utf8_encode_cp(0x1F600, &mut buf), 4); // 😀
        assert_eq!(&buf[..4], &[0xF0, 0x9F, 0x98, 0x80]);
    }

    #[test]
    fn encode_rejects_out_of_range() {
        let mut buf = [0u8; 4];
        assert_eq!(utf8_encode_cp(-1, &mut buf), 0);
        assert_eq!(utf8_encode_cp(0x110000, &mut buf), 0);
    }

    #[test]
    fn decode_roundtrip_ascii() {
        let (cp, n) = utf8_decode_cp(b"A");
        assert_eq!(cp, b'A' as i32);
        assert_eq!(n, 1);
    }

    #[test]
    fn decode_roundtrip_multibyte() {
        let (cp, n) = utf8_decode_cp(&[0xC2, 0xA9]);
        assert_eq!(cp, 0x00A9);
        assert_eq!(n, 2);
        let (cp, n) = utf8_decode_cp(&[0xE4, 0xB8, 0xAD]);
        assert_eq!(cp, 0x4E2D);
        assert_eq!(n, 3);
        let (cp, n) = utf8_decode_cp(&[0xF0, 0x9F, 0x98, 0x80]);
        assert_eq!(cp, 0x1F600);
        assert_eq!(n, 4);
    }

    #[test]
    fn decode_before_ascii() {
        let s = b"ab";
        assert_eq!(utf8_decode_cp_before(s, 1), (b'a' as i32, 1));
        assert_eq!(utf8_decode_cp_before(s, 2), (b'b' as i32, 1));
    }

    #[test]
    fn decode_before_all_lengths() {
        // "a" + © (2B) + 中 (3B) + 😀 (4B) — decode each cp by its
        // exclusive end offset.
        let s: &[u8] = &[b'a', 0xC2, 0xA9, 0xE4, 0xB8, 0xAD, 0xF0, 0x9F, 0x98, 0x80];
        assert_eq!(utf8_decode_cp_before(s, 1), (b'a' as i32, 1));
        assert_eq!(utf8_decode_cp_before(s, 3), (0x00A9, 2));
        assert_eq!(utf8_decode_cp_before(s, 6), (0x4E2D, 3));
        assert_eq!(utf8_decode_cp_before(s, 10), (0x1F600, 4));
    }

    #[test]
    fn decode_before_malformed_is_single_byte() {
        // Bare continuation byte at the start — no lead in range.
        assert_eq!(utf8_decode_cp_before(&[0x80], 1), (0x80, 1));
        // Continuation whose scanned-back lead declares a length that
        // doesn't end at pos (truncated 4-byte seq read at offset 3).
        assert_eq!(utf8_decode_cp_before(&[0xF0, 0x9F, 0x98], 3), (0x98, 1));
    }

    #[test]
    fn roundtrip_encode_decode_dense() {
        for cp in [
            0i32, 0x7F, 0x80, 0xFF, 0x3FF, 0x800, 0xFFFD, 0x10000, 0x1_0000, 0x10_FFFF,
        ] {
            let mut buf = [0u8; 4];
            let n = utf8_encode_cp(cp, &mut buf);
            assert!(n > 0, "encode failed for cp 0x{cp:X}");
            let (dec, dn) = utf8_decode_cp(&buf[..n]);
            assert_eq!(dec, cp, "roundtrip mismatch at 0x{cp:X}");
            assert_eq!(dn, n);
            assert_eq!(utf8_len_for(buf[0]), n);
        }
    }
}
