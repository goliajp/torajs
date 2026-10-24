use super::StringLiteral;

#[test]
fn ascii_encodes_as_latin1_byte_identical() {
    let lit = StringLiteral::encode_from_str("abc");
    assert!(lit.is_latin1);
    assert_eq!(lit.bytes, b"abc");
    assert_eq!(lit.length, 3);
}

#[test]
fn latin1_supplement_stays_latin1() {
    // U+00E9 ('é') — Latin-1 supplement, ≤ 0xFF, so still
    // Latin-1 encoded as one byte (0xE9).
    let lit = StringLiteral::encode_from_str("caf\u{00E9}");
    assert!(lit.is_latin1);
    assert_eq!(lit.bytes, [b'c', b'a', b'f', 0xE9]);
    assert_eq!(lit.length, 4);
}

#[test]
fn bmp_above_0xff_encodes_as_utf16_le() {
    // U+4E2D ('中') — BMP, > 0xFF → UTF-16 LE, 2 bytes per
    // code unit.
    let lit = StringLiteral::encode_from_str("\u{4E2D}\u{6587}");
    assert!(!lit.is_latin1);
    assert_eq!(lit.bytes, [0x2D, 0x4E, 0x87, 0x65]);
    assert_eq!(lit.length, 2);
}

#[test]
fn supplementary_plane_encodes_as_surrogate_pair() {
    // U+1F600 ('😀') — supplementary plane.
    // cp - 0x10000 = 0xF600
    // hi = 0xD800 | (0xF600 >> 10) = 0xD800 | 0x3D = 0xD83D
    // lo = 0xDC00 | (0xF600 & 0x3FF) = 0xDC00 | 0x200 = 0xDE00
    let lit = StringLiteral::encode_from_str("\u{1F600}");
    assert!(!lit.is_latin1);
    assert_eq!(lit.bytes, [0x3D, 0xD8, 0x00, 0xDE]);
    assert_eq!(lit.length, 2);
}

#[test]
fn mixed_bmp_and_surrogate_pair() {
    // "A😀" — 'A' is ASCII so any non-ASCII codepoint forces
    // the whole string to UTF-16. The 'A' becomes a single
    // BMP code unit (0x0041 → "\x41\x00"), the emoji becomes
    // a surrogate pair (0xD83D 0xDE00).
    let lit = StringLiteral::encode_from_str("A\u{1F600}");
    assert!(!lit.is_latin1);
    assert_eq!(lit.bytes, [0x41, 0x00, 0x3D, 0xD8, 0x00, 0xDE]);
    assert_eq!(lit.length, 3); // 'A' (1) + surrogate pair (2)
}

#[test]
fn from_latin1_bytes_helper_matches_encode_for_ascii() {
    let a = StringLiteral::from_latin1_bytes(b"hello".to_vec());
    let b = StringLiteral::encode_from_str("hello");
    assert_eq!(a.bytes, b.bytes);
    assert_eq!(a.length, b.length);
    assert_eq!(a.is_latin1, b.is_latin1);
}
