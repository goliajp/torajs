//! The base64 and hex codecs behind `Uint8Array`'s six conversion
//! methods (§23.2.2.1-2 `fromBase64` / `fromHex`, §23.2.3.x
//! `toBase64` / `toHex` / `setFromBase64` / `setFromHex`).
//!
//! Bytes in, bytes out — nothing here knows about AnyValue, so the
//! whole decision surface is reachable from unit tests. The wiring
//! that reads the options bag and mints the results is next door.
//!
//! Three things about the decoder are easy to get subtly wrong, and
//! all three are observable:
//!
//! - **A chunk is committed all-or-nothing.** `setFrom*` decodes into
//!   a buffer that may run out, and the spec does not fill it
//!   partially: `'Zm9vYmFy'` into five bytes writes three and reports
//!   `read: 4`, while `'Zm9vYmE='` into five writes all five and
//!   reports `read: 8` — the second chunk yields two bytes there and
//!   three here, and only the one that fits entirely is taken. So the
//!   capacity test is against the chunk's OWN size, not a fixed three.
//!
//! - **`read` counts what was committed, not what was looked at.** It
//!   advances only when a chunk lands, which is why a decode that
//!   stops for capacity reports the index of the last complete chunk.
//!
//! - **The bytes decoded before an error still count.** `setFromBase64`
//!   writes them and then throws, so an error carries its prefix
//!   rather than replacing it.
//!
//! Every number in the doc comments above is a case from test262's
//! `prototype/setFromBase64/target-size.js`.

const STD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const HEX: &[u8; 16] = b"0123456789abcdef";

/// §23.2.2.1 `lastChunkHandling` — what to do with a final chunk that
/// is not four characters long.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LastChunk {
    /// Decode it, and let non-zero overflow bits through.
    Loose,
    /// Require padding, and require the overflow bits to be zero.
    Strict,
    /// Leave it undecoded and report the index before it.
    StopBeforePartial,
}

/// A decode's three answers. `err` is always a SyntaxError message at
/// the call site; `bytes` is what was decoded BEFORE it and still has
/// to be written.
pub(crate) struct Decoded {
    pub read: usize,
    pub bytes: Vec<u8>,
    pub err: Option<&'static str>,
}

impl Decoded {
    fn stop(read: usize, bytes: Vec<u8>) -> Self {
        Self {
            read,
            bytes,
            err: None,
        }
    }
    fn failed(read: usize, bytes: Vec<u8>, err: &'static str) -> Self {
        Self {
            read,
            bytes,
            err: Some(err),
        }
    }
}

/// The five code points §23.2 calls ASCII whitespace. Anything else
/// outside the alphabet is an error, which is why nbsp and U+2028 —
/// whitespace to a human and to `String.prototype.trim` — are not
/// here.
fn is_ws(b: u8) -> bool {
    matches!(b, 0x09 | 0x0A | 0x0C | 0x0D | 0x20)
}

fn skip_ws(s: &[u8], mut i: usize) -> usize {
    while i < s.len() && is_ws(s[i]) {
        i += 1;
    }
    i
}

fn alphabet_index(c: u8, url: bool) -> Option<u8> {
    let table: &[u8; 64] = if url { URL } else { STD };
    table.iter().position(|&t| t == c).map(|p| p as u8)
}

pub(crate) fn encode_base64(src: &[u8], url: bool, omit_padding: bool) -> Vec<u8> {
    let table: &[u8; 64] = if url { URL } else { STD };
    let mut out: Vec<u8> = Vec::with_capacity(src.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= src.len() {
        let n = ((src[i] as u32) << 16) | ((src[i + 1] as u32) << 8) | src[i + 2] as u32;
        for shift in [18, 12, 6, 0] {
            out.push(table[((n >> shift) & 63) as usize]);
        }
        i += 3;
    }
    match src.len() - i {
        1 => {
            let n = (src[i] as u32) << 16;
            out.push(table[((n >> 18) & 63) as usize]);
            out.push(table[((n >> 12) & 63) as usize]);
            if !omit_padding {
                out.push(b'=');
                out.push(b'=');
            }
        }
        2 => {
            let n = ((src[i] as u32) << 16) | ((src[i + 1] as u32) << 8);
            for shift in [18, 12, 6] {
                out.push(table[((n >> shift) & 63) as usize]);
            }
            if !omit_padding {
                out.push(b'=');
            }
        }
        _ => {}
    }
    out
}

pub(crate) fn encode_hex(src: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(src.len() * 2);
    for &b in src {
        out.push(HEX[(b >> 4) as usize]);
        out.push(HEX[(b & 15) as usize]);
    }
    out
}

/// DecodeBase64Chunk over 2, 3 or 4 alphabet indices. `strict` is the
/// §23.2 "throwOnExtraBits" flag: a two-character chunk carries four
/// bits that no output byte holds and a three-character one carries
/// two, and `lastChunkHandling: "strict"` is the mode that refuses to
/// discard them silently.
fn decode_chunk(chunk: &[u8], strict: bool) -> Result<Vec<u8>, &'static str> {
    const EXTRA: &str = "base64 string has non-zero padding bits";
    let mut out = Vec::with_capacity(3);
    match chunk.len() {
        2 => {
            if strict && chunk[1] & 0x0F != 0 {
                return Err(EXTRA);
            }
            out.push((chunk[0] << 2) | (chunk[1] >> 4));
        }
        3 => {
            if strict && chunk[2] & 0x03 != 0 {
                return Err(EXTRA);
            }
            out.push((chunk[0] << 2) | (chunk[1] >> 4));
            out.push(((chunk[1] & 0x0F) << 4) | (chunk[2] >> 2));
        }
        _ => {
            out.push((chunk[0] << 2) | (chunk[1] >> 4));
            out.push(((chunk[1] & 0x0F) << 4) | (chunk[2] >> 2));
            out.push(((chunk[2] & 0x03) << 6) | chunk[3]);
        }
    }
    Ok(out)
}

/// §23.2 FromBase64. `max_len` caps the OUTPUT; `usize::MAX` is the
/// uncapped `fromBase64` call.
pub(crate) fn decode_base64(s: &[u8], url: bool, handling: LastChunk, max_len: usize) -> Decoded {
    let mut out: Vec<u8> = Vec::new();
    let mut read = 0usize;
    let mut i = 0usize;
    let mut chunk = [0u8; 4];
    let mut cl = 0usize;
    if max_len == 0 {
        return Decoded::stop(0, out);
    }
    loop {
        i = skip_ws(s, i);
        if i == s.len() {
            if cl == 0 {
                return Decoded::stop(s.len(), out);
            }
            return match handling {
                LastChunk::StopBeforePartial => Decoded::stop(read, out),
                LastChunk::Strict => Decoded::failed(read, out, "missing padding"),
                LastChunk::Loose if cl == 1 => Decoded::failed(
                    read,
                    out,
                    "malformed padding: exactly one additional character",
                ),
                LastChunk::Loose => match decode_chunk(&chunk[..cl], false) {
                    Err(e) => Decoded::failed(read, out, e),
                    Ok(b) if out.len() + b.len() > max_len => Decoded::stop(read, out),
                    Ok(b) => {
                        out.extend_from_slice(&b);
                        Decoded::stop(s.len(), out)
                    }
                },
            };
        }
        let c = s[i];
        i += 1;
        if c == b'=' {
            if cl < 2 {
                return Decoded::failed(read, out, "unexpected padding character");
            }
            i = skip_ws(s, i);
            if cl == 2 {
                if i == s.len() {
                    if handling == LastChunk::StopBeforePartial {
                        return Decoded::stop(read, out);
                    }
                    return Decoded::failed(read, out, "malformed padding: only one '='");
                }
                if s[i] != b'=' {
                    return Decoded::failed(read, out, "malformed padding: expected '='");
                }
                i = skip_ws(s, i + 1);
            }
            if i < s.len() {
                return Decoded::failed(read, out, "unexpected character after padding");
            }
            return match decode_chunk(&chunk[..cl], handling == LastChunk::Strict) {
                Err(e) => Decoded::failed(read, out, e),
                Ok(b) if out.len() + b.len() > max_len => Decoded::stop(read, out),
                Ok(b) => {
                    out.extend_from_slice(&b);
                    Decoded::stop(s.len(), out)
                }
            };
        }
        let Some(v) = alphabet_index(c, url) else {
            return Decoded::failed(read, out, "invalid base64 character");
        };
        chunk[cl] = v;
        cl += 1;
        if cl == 4 {
            if out.len() + 3 > max_len {
                return Decoded::stop(read, out);
            }
            let b = decode_chunk(&chunk, false).expect("a four-character chunk always decodes");
            out.extend_from_slice(&b);
            cl = 0;
            read = i;
            if out.len() == max_len {
                return Decoded::stop(read, out);
            }
        }
    }
}

/// §23.2 FromHex. An odd length returns AT ONCE — step 5 answers the
/// record before the decode loop is reached, so nothing is written
/// (`new Uint8Array(4).setFromHex("aabbc")` leaves all four zero,
/// where an illegal character mid-string leaves the pairs before it).
pub(crate) fn decode_hex(s: &[u8], max_len: usize) -> Decoded {
    let mut out: Vec<u8> = Vec::new();
    if s.len() % 2 == 1 {
        return Decoded::failed(0, out, "string should be an even number of hex characters");
    }
    let end = s.len();
    let mut i = 0usize;
    while i < end {
        if out.len() == max_len {
            return Decoded::stop(i, out);
        }
        let (hi, lo) = (hex_digit(s[i]), hex_digit(s[i + 1]));
        let (Some(hi), Some(lo)) = (hi, lo) else {
            return Decoded::failed(i, out, "invalid hex character");
        };
        out.push((hi << 4) | lo);
        i += 2;
    }
    Decoded::stop(i, out)
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(s: &str, h: LastChunk, max: usize) -> (usize, Vec<u8>, Option<&'static str>) {
        let d = decode_base64(s.as_bytes(), false, h, max);
        (d.read, d.bytes, d.err)
    }

    #[test]
    fn rfc4648_vectors_round_trip() {
        for (text, bytes) in [
            ("", &[][..]),
            ("Zg==", &[102][..]),
            ("Zm8=", &[102, 111][..]),
            ("Zm9v", &[102, 111, 111][..]),
            ("Zm9vYg==", &[102, 111, 111, 98][..]),
            ("Zm9vYmE=", &[102, 111, 111, 98, 97][..]),
            ("Zm9vYmFy", &[102, 111, 111, 98, 97, 114][..]),
        ] {
            let d = decode_base64(text.as_bytes(), false, LastChunk::Loose, usize::MAX);
            assert_eq!((d.bytes.as_slice(), d.err), (bytes, None), "decode {text}");
            assert_eq!(encode_base64(bytes, false, false), text.as_bytes(), "encode");
        }
    }

    #[test]
    fn hex_vectors_round_trip() {
        for (text, bytes) in [
            ("", &[][..]),
            ("66", &[102][..]),
            ("666f6f626172", &[102, 111, 111, 98, 97, 114][..]),
        ] {
            let d = decode_hex(text.as_bytes(), usize::MAX);
            assert_eq!((d.bytes.as_slice(), d.err), (bytes, None));
            assert_eq!(encode_hex(bytes), text.as_bytes());
        }
        // uppercase decodes, and an odd tail still yields its prefix
        assert_eq!(decode_hex(b"666F", usize::MAX).bytes, vec![102, 111]);
        // Step 5 answers before the loop, so an odd length decodes
        // NOTHING; an illegal character keeps the pairs before it.
        let d = decode_hex(b"aabbc", usize::MAX);
        assert_eq!((d.read, d.bytes, d.err.is_some()), (0, vec![], true));
        let d = decode_hex(b"aabbcz", usize::MAX);
        assert_eq!((d.bytes, d.err.is_some()), (vec![170, 187], true));
    }

    /// test262 `fromBase64/last-chunk-handling.js`, every row.
    #[test]
    fn last_chunk_handling() {
        let want = [101u8, 120, 97, 102];
        let three = [101u8, 120, 97];
        for h in [
            LastChunk::Loose,
            LastChunk::Strict,
            LastChunk::StopBeforePartial,
        ] {
            assert_eq!(dec("ZXhhZg==", h, usize::MAX).1, want, "padded {h:?}");
        }
        assert_eq!(dec("ZXhhZg", LastChunk::Loose, usize::MAX).1, want);
        assert_eq!(
            dec("ZXhhZg", LastChunk::StopBeforePartial, usize::MAX).1,
            three
        );
        assert!(dec("ZXhhZg", LastChunk::Strict, usize::MAX).2.is_some());
        // non-zero padding bits: only strict refuses them
        assert_eq!(dec("ZXhhZh==", LastChunk::Loose, usize::MAX).1, want);
        assert_eq!(
            dec("ZXhhZh==", LastChunk::StopBeforePartial, usize::MAX).1,
            want
        );
        assert!(dec("ZXhhZh==", LastChunk::Strict, usize::MAX).2.is_some());
        assert_eq!(dec("ZXhhZh", LastChunk::Loose, usize::MAX).1, want);
        // partial padding
        assert!(dec("ZXhhZg=", LastChunk::Loose, usize::MAX).2.is_some());
        assert_eq!(
            dec("ZXhhZg=", LastChunk::StopBeforePartial, usize::MAX).1,
            three
        );
        // excess padding is an error in every mode
        for h in [
            LastChunk::Loose,
            LastChunk::Strict,
            LastChunk::StopBeforePartial,
        ] {
            assert!(dec("ZXhhZg===", h, usize::MAX).2.is_some(), "excess {h:?}");
        }
    }

    /// test262 `prototype/setFromBase64/target-size.js`, every row —
    /// the capacity test is against the chunk's own size.
    #[test]
    fn capacity_commits_whole_chunks() {
        assert_eq!(dec("Zm9vYmFy", LastChunk::Loose, 5).0, 4);
        assert_eq!(dec("Zm9vYmFy", LastChunk::Loose, 5).1.len(), 3);
        assert_eq!(dec("Zm9vYmE=", LastChunk::Loose, 4).0, 4);
        assert_eq!(dec("Zm9vYmE=", LastChunk::Loose, 4).1.len(), 3);
        assert_eq!(dec("Zm9vYmFy", LastChunk::Loose, 6).0, 8);
        assert_eq!(dec("Zm9vYmE=", LastChunk::Loose, 5).0, 8);
        assert_eq!(dec("Zm9vYmE=", LastChunk::Loose, 5).1.len(), 5);
        assert_eq!(dec("Zm9vYmE", LastChunk::Loose, 5).0, 7);
        assert_eq!(dec("Zm9vYmE", LastChunk::Loose, 5).1.len(), 5);
        assert_eq!(dec("Zm9vYmE=", LastChunk::StopBeforePartial, 5).0, 8);
        assert_eq!(dec("Zm9vYmE", LastChunk::StopBeforePartial, 5).0, 4);
        assert_eq!(dec("Zm9vYmFy", LastChunk::Loose, 7).0, 8);
        // hex stops on the same rule
        let d = decode_hex(b"aabbcc", 2);
        assert_eq!((d.read, d.bytes), (4, vec![170, 187]));
    }

    /// test262 `prototype/setFromBase64/writes-up-to-error.js` — the
    /// prefix survives the error.
    #[test]
    fn error_carries_its_prefix() {
        let d = decode_base64(b"MjYyZm.9v", false, LastChunk::Loose, 5);
        assert_eq!((d.bytes.as_slice(), d.err.is_some()), (&[50, 54, 50][..], true));
        let d = decode_base64(b"MjYyZg", false, LastChunk::Strict, 5);
        assert_eq!((d.bytes.as_slice(), d.err.is_some()), (&[50, 54, 50][..], true));
        let d = decode_base64(b"MjYyZg===", false, LastChunk::Loose, 5);
        assert_eq!((d.bytes.as_slice(), d.err.is_some()), (&[50, 54, 50][..], true));
    }

    /// test262 `fromBase64/alphabet.js` + `whitespace.js` +
    /// `illegal-characters.js`.
    #[test]
    fn alphabets_whitespace_and_rejects() {
        let want = [199u8, 239, 242];
        assert_eq!(
            decode_base64(b"x+/y", false, LastChunk::Loose, usize::MAX).bytes,
            want
        );
        assert!(
            decode_base64(b"x+/y", true, LastChunk::Loose, usize::MAX)
                .err
                .is_some()
        );
        assert_eq!(
            decode_base64(b"x-_y", true, LastChunk::Loose, usize::MAX).bytes,
            want
        );
        assert!(
            decode_base64(b"x-_y", false, LastChunk::Loose, usize::MAX)
                .err
                .is_some()
        );
        for ws in [b"Z g==", b"Z\tg==", b"Z\ng==", b"Z\x0Cg==", b"Z\rg=="] {
            let d = decode_base64(ws, false, LastChunk::Loose, usize::MAX);
            assert_eq!((d.bytes.as_slice(), d.err), (&[102][..], None));
        }
        // nbsp and U+2028 are whitespace to a reader and not to §23.2
        for bad in ["Zm.9v", "Zm9v^", "Zg==&", "Zg\u{00A0}==", "Zg\u{2028}=="] {
            assert!(
                decode_base64(bad.as_bytes(), false, LastChunk::Loose, usize::MAX)
                    .err
                    .is_some(),
                "{bad} should be rejected"
            );
        }
    }

    /// test262 `prototype/toBase64/omit-padding.js`.
    #[test]
    fn encode_padding_and_alphabet() {
        assert_eq!(encode_base64(&[199, 239], false, false), b"x+8=");
        assert_eq!(encode_base64(&[199, 239], false, true), b"x+8");
        assert_eq!(encode_base64(&[255], false, true), b"/w");
        assert_eq!(encode_base64(&[199, 239], true, false), b"x-8=");
        assert_eq!(encode_base64(&[255], true, true), b"_w");
        assert_eq!(encode_base64(&[255], false, false), b"/w==");
    }
}
