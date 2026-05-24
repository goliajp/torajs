//! `torajs-codec-hex` — F.1 (2026-05-25) Phase 2 F 族 first crate.
//!
//! Metal-level minimal hex encode/decode. Replaces the `hex 0.4`
//! community dep per torajs vision priority #4 (0 deps). Aligned
//! with deps-tree v0.1 F.1 spec: "trigger 10 (F 族启动)".
//!
//! Lean surface: only the encode/decode lower-case variants the
//! workspace uses (`hex::encode(bytes) -> String`,
//! `hex::decode(&str) -> Result<Vec<u8>, FromHexError>`). Upper-
//! case + streaming variants intentionally absent — add when a
//! caller needs them.
//!
//! # Algorithm
//!
//! Encode: for each input byte, emit two ASCII hex digits (`b / 16`
//! and `b & 0xf` indexed into `b"0123456789abcdef"`). Output length
//! is `2 * input.len()`.
//!
//! Decode: input length must be even; for each byte pair `(hi, lo)`,
//! decode each ASCII hex digit to its 4-bit value (`0-9` / `a-f` /
//! `A-F`, all three ranges accepted on input). Output length is
//! `input.len() / 2`. Returns `FromHexError::OddLength` if input has
//! odd byte count, `FromHexError::InvalidHexCharacter { c, index }`
//! on the first non-hex byte.
//!
//! # Compatibility
//!
//! Output is byte-identical to `hex 0.4`'s `encode` / `decode` for
//! the surface we expose. Tests in this crate verify across the
//! 256-byte alphabet + several SHA-256 digest fixtures.

#![forbid(unsafe_code)]

use std::fmt;

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Encode `bytes` as a lower-case hex `String`.
///
/// Drop-in replacement for `hex::encode(bytes)`. Allocates one
/// `String` of exactly `2 * bytes.len()` bytes; no intermediate
/// buffers.
pub fn encode<T: AsRef<[u8]>>(bytes: T) -> String {
    let input = bytes.as_ref();
    let mut out = String::with_capacity(input.len() * 2);
    for &b in input {
        out.push(HEX_DIGITS[(b >> 4) as usize] as char);
        out.push(HEX_DIGITS[(b & 0xf) as usize] as char);
    }
    out
}

/// Errors returned by [`decode`].
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum FromHexError {
    /// Input length was not a multiple of 2.
    OddLength,
    /// Encountered a non-hex byte at the given position. `c` is the
    /// raw byte (as `char`) and `index` is its byte offset.
    InvalidHexCharacter { c: char, index: usize },
}

impl fmt::Display for FromHexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FromHexError::OddLength => write!(f, "Odd number of digits"),
            FromHexError::InvalidHexCharacter { c, index } => {
                write!(f, "Invalid character {c:?} at position {index}")
            }
        }
    }
}

impl std::error::Error for FromHexError {}

#[inline]
fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Decode a lower-or-mixed-case hex string into a `Vec<u8>`.
///
/// Drop-in replacement for `hex::decode(s)`. Accepts both lower-
/// case and upper-case input on each digit independently (matches
/// `hex 0.4` semantics). Returns the first error encountered.
pub fn decode<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>, FromHexError> {
    let bytes = input.as_ref();
    if bytes.len() % 2 != 0 {
        return Err(FromHexError::OddLength);
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for (i, pair) in bytes.chunks(2).enumerate() {
        let hi = hex_val(pair[0]).ok_or(FromHexError::InvalidHexCharacter {
            c: pair[0] as char,
            index: i * 2,
        })?;
        let lo = hex_val(pair[1]).ok_or(FromHexError::InvalidHexCharacter {
            c: pair[1] as char,
            index: i * 2 + 1,
        })?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_empty() {
        assert_eq!(encode(b""), "");
    }

    #[test]
    fn encode_basic() {
        assert_eq!(encode(b"foo"), "666f6f");
        assert_eq!(encode([0x00, 0xff, 0xa5]), "00ffa5");
    }

    #[test]
    fn encode_full_byte_alphabet() {
        let bytes: Vec<u8> = (0u8..=255).collect();
        let encoded = encode(&bytes);
        assert_eq!(encoded.len(), 512);
        // first byte 0x00 → "00", last byte 0xff → "ff"
        assert!(encoded.starts_with("00"));
        assert!(encoded.ends_with("ff"));
        // SHA-256 ascii hex is exactly this routine on a 32-byte digest
        let middle_chunk: String = encoded[2 * 0x80..2 * 0x82].into();
        assert_eq!(middle_chunk, "8081");
    }

    #[test]
    fn decode_empty() {
        assert_eq!(decode("").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn decode_basic_lowercase() {
        assert_eq!(decode("666f6f").unwrap(), b"foo");
    }

    #[test]
    fn decode_basic_uppercase() {
        assert_eq!(decode("666F6F").unwrap(), b"foo");
    }

    #[test]
    fn decode_mixed_case() {
        assert_eq!(decode("66 6F 6f".replace(' ', "")).unwrap(), b"foo");
    }

    #[test]
    fn decode_odd_length() {
        assert_eq!(decode("abc"), Err(FromHexError::OddLength));
    }

    #[test]
    fn decode_invalid_char() {
        assert_eq!(
            decode("zz"),
            Err(FromHexError::InvalidHexCharacter { c: 'z', index: 0 })
        );
        assert_eq!(
            decode("a!"),
            Err(FromHexError::InvalidHexCharacter { c: '!', index: 1 })
        );
    }

    #[test]
    fn roundtrip_full_alphabet() {
        let bytes: Vec<u8> = (0u8..=255).collect();
        let encoded = encode(&bytes);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded, bytes);
    }

    #[test]
    fn roundtrip_sha256_digest_fixture() {
        // SHA-256 of "abc" — well-known constant
        let digest: [u8; 32] = [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ];
        let hex = encode(digest);
        assert_eq!(
            hex,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(decode(&hex).unwrap(), digest.to_vec());
    }
}
