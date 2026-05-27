//! Step 8d — compile-time ShortStr u64 encoder.
//!
//! Bit-identical dual of `torajs_anyvalue::nanbox::try_box_short_str`.
//! Inlined inside `torajs-core` (rather than `use`'d from
//! torajs-anyvalue) so this codegen-time path does not pull
//! torajs-anyvalue's rlib into the libtorajs_embed cdylib link,
//! which would otherwise force the compile-driver dylib to also
//! resolve every `pub extern "C"` runtime symbol in torajs-anyvalue
//! (`__torajs_str_alloc_pooled`, `__torajs_throw_*`, the print_*
//! shims, etc.) — symbols only the AOT'd user binary needs, not the
//! compile pipeline.
//!
//! Encoding is part of the NaN-box ABI spec (see
//! `docs/v0.7-Phase3-Step8-sso.md` "Layout — ShortStr encoding"):
//! top16 = `0x0001` + 8-bit len + 5 × 8-bit little-endian payload.
//! Must stay byte-equal with the canonical encoder in
//! `torajs_anyvalue::nanbox`.

/// NaN-box top-16 marker for ShortStr-encoded `AnyValue`. Mirrors
/// `torajs_anyvalue::nanbox::SHORT_STR_TAG`.
pub const SHORT_STR_TAG: u64 = 0x0001_0000_0000_0000;

/// Maximum inline byte payload for ShortStr. Mirrors
/// `torajs_anyvalue::nanbox::SHORT_STR_CAP`.
pub const SHORT_STR_CAP: usize = 5;

/// Try to encode a byte slice as a ShortStr u64 at compile time.
/// Returns `None` when `bytes.len() > SHORT_STR_CAP` — caller falls
/// back to the runtime heap-alloc + `any_box` path.
pub fn encode_short_str_literal(bytes: &[u8]) -> Option<u64> {
    if bytes.len() > SHORT_STR_CAP {
        return None;
    }
    let mut payload: u64 = 0;
    for (i, &b) in bytes.iter().enumerate() {
        payload |= (b as u64) << (i * 8);
    }
    Some(SHORT_STR_TAG | ((bytes.len() as u64) << 40) | payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_literal_encodes_with_len_zero() {
        assert_eq!(encode_short_str_literal(b""), Some(0x0001_0000_0000_0000));
    }

    #[test]
    fn ascii_5_bytes_encodes_full_payload() {
        // "abcde" → top16=0x0001, len=5, bytes little-endian.
        let v = encode_short_str_literal(b"abcde").unwrap();
        assert_eq!(v & 0xFFFF_0000_0000_0000, SHORT_STR_TAG);
        assert_eq!((v >> 40) & 0xFF, 5);
        assert_eq!(v & 0xFF, 0x61); // byte 0 = 'a'
        assert_eq!((v >> 32) & 0xFF, 0x65); // byte 4 = 'e'
    }

    #[test]
    fn six_bytes_falls_through() {
        assert_eq!(encode_short_str_literal(b"abcdef"), None);
    }

    #[test]
    fn multibyte_utf8_fits_when_byte_len_bounded() {
        // "中" = 3 UTF-8 bytes, fits.
        let v = encode_short_str_literal("中".as_bytes()).unwrap();
        assert_eq!((v >> 40) & 0xFF, 3);
        // "中文" = 6 bytes, over cap.
        assert_eq!(encode_short_str_literal("中文".as_bytes()), None);
    }
}
