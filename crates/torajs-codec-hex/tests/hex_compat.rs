//! Byte-identical compatibility tests against the published `hex 0.4`
//! output. We do NOT actually depend on `hex 0.4` (that would defeat
//! the F.1 ship purpose); instead we hardcode the `hex 0.4` reference
//! output for each fixture. If a future polish change to encode /
//! decode diverges, these tests catch it.
//!
//! Fixtures chosen to exercise different shapes:
//!   - empty input (length-0 edge case)
//!   - short ASCII string ("foo")
//!   - full 0..=255 byte alphabet (every digit pair, exhaustive)
//!   - SHA-256 digest of "abc" (the workspace's actual hot use case)

use torajs_codec_hex::{decode, encode};

#[test]
fn encode_compat_hex_0_4_empty() {
    let bytes: &[u8] = b"";
    // hex::encode("") == ""
    assert_eq!(encode(bytes), "");
}

#[test]
fn encode_compat_hex_0_4_basic() {
    // hex::encode(b"foo") == "666f6f"
    assert_eq!(encode(b"foo"), "666f6f");
    // hex::encode([0x00, 0xff, 0xa5]) == "00ffa5"
    assert_eq!(encode([0x00, 0xff, 0xa5]), "00ffa5");
}

#[test]
fn encode_compat_hex_0_4_full_byte_alphabet() {
    // hex::encode((0u8..=255).collect::<Vec<_>>())
    //   produces "000102...feff" — 512 ASCII chars, exhaustively
    //   covering every 8-bit byte's lower-case hex pair.
    let bytes: Vec<u8> = (0u8..=255).collect();
    let got = encode(&bytes);
    assert_eq!(got.len(), 512);
    assert!(got.starts_with("00010203"));
    assert!(got.ends_with("fcfdfeff"));
    // Verify a middle byte: byte 0x80 should encode to "80" at
    // string offset 0x80 * 2 = 256.
    assert_eq!(&got[256..258], "80");
    // Spot-check that no digit ever appears upper-case.
    assert!(
        got.chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
    );
}

#[test]
fn encode_compat_hex_0_4_sha256_abc() {
    // SHA-256("abc") = 0xba78... (well-known NIST test vector)
    let digest: [u8; 32] = [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22,
        0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00,
        0x15, 0xad,
    ];
    // hex::encode(digest) for that vector is exactly this 64-char
    // lower-case string — copied verbatim from the NIST FIPS-180-2
    // appendix C.1.
    assert_eq!(
        encode(digest),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn decode_compat_hex_0_4_roundtrip_full_alphabet() {
    let bytes: Vec<u8> = (0u8..=255).collect();
    let encoded = encode(&bytes);
    let decoded = decode(&encoded).unwrap();
    assert_eq!(decoded, bytes);
}

#[test]
fn decode_compat_hex_0_4_upper_lower_mixed() {
    // hex 0.4 tolerates per-digit case on input. Verify all three.
    let expected = b"foo";
    assert_eq!(decode("666f6f").unwrap(), expected);
    assert_eq!(decode("666F6F").unwrap(), expected);
    assert_eq!(decode("66 6f 6F".replace(' ', "")).unwrap(), expected);
}

#[test]
fn decode_compat_hex_0_4_error_shapes() {
    // hex 0.4's FromHexError::OddLength
    match decode("abc") {
        Err(torajs_codec_hex::FromHexError::OddLength) => (),
        other => panic!("expected OddLength, got {other:?}"),
    }
    // hex 0.4's FromHexError::InvalidHexCharacter { c, index }
    match decode("zz") {
        Err(torajs_codec_hex::FromHexError::InvalidHexCharacter { c, index }) => {
            assert_eq!(c, 'z');
            assert_eq!(index, 0);
        }
        other => panic!("expected InvalidHexCharacter, got {other:?}"),
    }
    // Invalid char at odd offset (index = 1) — verifies the index
    // calculation includes the per-pair offset shift.
    match decode("a!") {
        Err(torajs_codec_hex::FromHexError::InvalidHexCharacter { c, index }) => {
            assert_eq!(c, '!');
            assert_eq!(index, 1);
        }
        other => panic!("expected InvalidHexCharacter, got {other:?}"),
    }
}
