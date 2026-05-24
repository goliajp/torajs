//! `JSON.stringify` string-payload escaper — port of `runtime_str.c`
//! L1653-1696.
//!
//! Wraps `s` in `"…"` and replaces JSON-illegal control bytes / the
//! quote / backslash bytes with their escape sequences. Two-pass:
//! pre-compute output length so the result fits in one
//! pool-aware allocation.
//!
//! ## Mapping
//!
//! | byte         | escape   |
//! |--------------|----------|
//! | `"`          | `\"`     |
//! | `\`          | `\\`     |
//! | `\n`         | `\n`     |
//! | `\r`         | `\r`     |
//! | `\t`         | `\t`     |
//! | `\b`         | `\b`     |
//! | `\f`         | `\f`     |
//! | other < 0x20 | `\u00XX` |
//! | else         | pass     |
//!
//! Byte-level: bytes ≥ 0x20 (including UTF-8 continuation / lead
//! bytes) pass through unchanged. JSON.stringify is supposed to
//! escape lone surrogates per ES2019; that wedge belongs to a
//! later spec-tightening task (matches the pre-port C behavior
//! bit-for-bit).

use crate::alloc::StrBlock;
use crate::layout::{STR_DATA_OFF, STR_LEN_OFF};

const HEX: &[u8; 16] = b"0123456789abcdef";

#[inline]
fn escaped_len(s: &[u8]) -> u64 {
    let mut out: u64 = 2; // surrounding quotes
    for &c in s {
        out += match c {
            b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 0x08 | 0x0c => 2,
            c if c < 0x20 => 6, // \uXXXX
            _ => 1,
        };
    }
    out
}

#[inline]
fn write_escaped(s: &[u8], dst: &mut [u8]) {
    dst[0] = b'"';
    let mut cur = 1usize;
    for &c in s {
        match c {
            b'"' => {
                dst[cur] = b'\\';
                dst[cur + 1] = b'"';
                cur += 2;
            }
            b'\\' => {
                dst[cur] = b'\\';
                dst[cur + 1] = b'\\';
                cur += 2;
            }
            b'\n' => {
                dst[cur] = b'\\';
                dst[cur + 1] = b'n';
                cur += 2;
            }
            b'\r' => {
                dst[cur] = b'\\';
                dst[cur + 1] = b'r';
                cur += 2;
            }
            b'\t' => {
                dst[cur] = b'\\';
                dst[cur + 1] = b't';
                cur += 2;
            }
            0x08 => {
                dst[cur] = b'\\';
                dst[cur + 1] = b'b';
                cur += 2;
            }
            0x0c => {
                dst[cur] = b'\\';
                dst[cur + 1] = b'f';
                cur += 2;
            }
            c if c < 0x20 => {
                dst[cur] = b'\\';
                dst[cur + 1] = b'u';
                dst[cur + 2] = b'0';
                dst[cur + 3] = b'0';
                dst[cur + 4] = HEX[(c >> 4) as usize];
                dst[cur + 5] = HEX[(c & 0xf) as usize];
                cur += 6;
            }
            _ => {
                dst[cur] = c;
                cur += 1;
            }
        }
    }
    dst[cur] = b'"';
}

/// `JSON.stringify(str)` — escape `s`'s payload + surround with
/// `"…"`. Returns a fresh refcount=1 Str block.
///
/// # Safety
///
/// `s` must be a valid Str heap block (non-null, layout per
/// [`crate::layout`]).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_quote_str(s: *const u8) -> *mut u8 {
    let len = unsafe { (s.add(STR_LEN_OFF) as *const u64).read() };
    let bytes = unsafe { core::slice::from_raw_parts(s.add(STR_DATA_OFF), len as usize) };
    let out_len = escaped_len(bytes);
    let mut block = StrBlock::alloc(out_len);
    // SAFETY: block was just allocated with payload capacity `out_len`.
    let dst = unsafe { block.as_bytes_mut(out_len) };
    write_escaped(bytes, dst);
    block.into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_payload_quotes() {
        assert_eq!(escaped_len(b""), 2);
        let mut buf = vec![0u8; 2];
        write_escaped(b"", &mut buf);
        assert_eq!(&buf, b"\"\"");
    }

    #[test]
    fn ascii_passthrough() {
        assert_eq!(escaped_len(b"hello"), 7);
        let mut buf = vec![0u8; 7];
        write_escaped(b"hello", &mut buf);
        assert_eq!(&buf, b"\"hello\"");
    }

    #[test]
    fn quote_and_backslash() {
        let input = br#"a"b\c"#;
        assert_eq!(escaped_len(input), 9);
        let mut buf = vec![0u8; 9];
        write_escaped(input, &mut buf);
        assert_eq!(&buf, br#""a\"b\\c""#);
    }

    #[test]
    fn whitespace_escapes() {
        let input = b"\n\r\t\x08\x0c";
        assert_eq!(escaped_len(input), 12);
        let mut buf = vec![0u8; 12];
        write_escaped(input, &mut buf);
        assert_eq!(&buf, br#""\n\r\t\b\f""#);
    }

    #[test]
    fn control_byte_unicode_escape() {
        let input = b"\x01\x1f";
        assert_eq!(escaped_len(input), 14);
        let mut buf = vec![0u8; 14];
        write_escaped(input, &mut buf);
        // Expected literal 14 bytes: " \ u 0 0 0 1 \ u 0 0 1 f "
        let expected: [u8; 14] = *b"\"\\u0001\\u001f\"";
        assert_eq!(&buf[..], &expected[..]);
    }

    #[test]
    fn high_byte_passthrough() {
        // UTF-8 bytes ≥ 0x80 pass through unchanged — JSON.stringify
        // is spec'd to escape lone surrogates but that wedge is
        // deferred; matches pre-port C behavior.
        let input = b"\xe4\xb8\xad"; // "中" in UTF-8
        assert_eq!(escaped_len(input), 5);
        let mut buf = vec![0u8; 5];
        write_escaped(input, &mut buf);
        assert_eq!(&buf, b"\"\xe4\xb8\xad\"");
    }
}
